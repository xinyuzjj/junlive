<script setup lang="ts">
/**
 * 回放专用播放器。
 *
 * 为什么不复用 `Player.vue`：那个是为**直播**写的 —— 里面塞满了直播才需要的东西
 * （线路切换、清晰度跟随、断流续接、延迟追帧、实时弹幕追加……），
 * 而回放是 VOD 语义：
 *   · 有确定的总时长，进度条能随便拖
 *   · 有完整的历史弹幕（按时间轴飘，不是来一条加一条）
 *   · 有 AI 看点，应该画在进度条上当章节刻度
 *   · 可以倍速
 * 硬塞进直播播放器只会让两边都别扭，所以单独一个。
 */
import Hls from "hls.js";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { PlayUrl, ReplayDanmaku, ReplayHighlight, ReplayQuality } from "../api";

const props = defineProps<{
  /** 播放地址（带签名的 m3u8，已包成本地代理地址） */
  play: PlayUrl | null;
  /** 整份历史弹幕，按 `time`（视频内秒数）飘 */
  danmaku?: ReplayDanmaku[];
  /** AI 看点，画在进度条上当刻度 */
  highlights?: ReplayHighlight[];
  /** 标题（左上角显示） */
  title?: string;
  /** 各段的名字（一场直播被切成多段，用来做分段菜单） */
  segments?: string[];
  /** 当前在第几段（0 基） */
  partIndex?: number;
}>();

const emit = defineEmits<{
  /** 这一段播完了（上层用来续下一段） */
  (e: "ended"): void;
  /** 飘出了一条弹幕（上层拿去填弹幕列表） */
  (e: "dm", m: ReplayDanmaku): void;
  /** 点了左上角返回 */
  (e: "back"): void;
  /** 切到第 n 段 */
  (e: "part", n: number): void;
}>();

const wrapRef = ref<HTMLElement | null>(null);
const videoRef = ref<HTMLVideoElement | null>(null);

const playing = ref(false);
const cur = ref(0);
const dur = ref(0);
const bufEnd = ref(0);
const status = ref("");
const err = ref("");
const cssFull = ref(false);
const showCtrl = ref(true);
const dmOn = ref(localStorage.getItem("junlive.dm.on") !== "0");
/** 倍速档位，跟常见视频站一致 */
const SPEEDS = [1, 1.25, 1.5, 2, 0.75, 0.5];
const speed = ref(1);
const volume = ref(Number(localStorage.getItem("junlive.volume") ?? "1"));
const muted = ref(false);
/** 鼠标悬停在进度条上的位置（秒），用来显示预览时间 */
const hoverT = ref<number | null>(null);
/** 当前打开的菜单 */
const menu = ref<"" | "q" | "seg" | "hl" | "dm">("");

/** 弹幕外观配置（用户自己调，存 localStorage） */
interface DmCfg {
  /** 字号 px */
  size: number;
  /** 不透明度 0~1 */
  opacity: number;
  /** 飘过屏幕耗时（秒），越小越快 */
  dur: number;
  /** 占播放器高度的百分比（防止挡住画面主体） */
  area: number;
}
const DM_DEF: DmCfg = { size: 16, opacity: 1, dur: 9, area: 50 };
const dmCfg = ref<DmCfg>({ ...DM_DEF });
try {
  Object.assign(dmCfg.value, JSON.parse(localStorage.getItem("junlive.dm.cfg") || "{}"));
} catch {
  /* 坏数据就用默认 */
}
function setDm(patch: Partial<DmCfg>) {
  Object.assign(dmCfg.value, patch);
  localStorage.setItem("junlive.dm.cfg", JSON.stringify(dmCfg.value));
}
/**
 * 当前清晰度 id。
 *
 * 初值要直接从 props 取 —— 组件挂载时 `play` 已经是好的了，
 * `watch(props.play)` 不带 immediate 不会触发，初值空着的话
 * 按钮会一直显示占位文字「清晰度」而不是当前档位名。
 */
const curQid = ref(props.play?.qualities?.[0]?.id ?? "");
let hls: Hls | null = null;
let hideTimer: number | undefined;
let retried = 0;

const fmt = (s: number) => {
  const t = Math.max(0, Math.floor(s || 0));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const ss = t % 60;
  const p = (x: number) => String(x).padStart(2, "0");
  return h ? `${h}:${p(m)}:${p(ss)}` : `${p(m)}:${p(ss)}`;
};

const pct = (t: number) => (dur.value > 0 ? `${(t / dur.value) * 100}%` : "0%");
const playedPct = computed(() => pct(cur.value));
const bufPct = computed(() => pct(bufEnd.value));

/** 可选清晰度（回放才有；直播走另一套） */
const quals = computed(() => props.play?.qualities ?? []);

/** 章节（AI 看点）：按时间铺在进度条上方，点一下跳过去 */
const chapters = computed(() =>
  [...(props.highlights ?? [])].sort((a, b) => a.time - b.time),
);
/** 当前章节的下标（-1 = 还没到第一个） */
const curChap = computed(() => {
  const cs = chapters.value;
  let k = -1;
  for (let i = 0; i < cs.length; i++) {
    if (cs[i].time <= cur.value + 0.5) k = i;
    else break;
  }
  return k;
});

/* ---------------- 播放内核 ---------------- */

function destroy() {
  if (hls) {
    hls.destroy();
    hls = null;
  }
}

/**
 * 挂载播放源。
 *
 * `keepTime`：切清晰度时用 —— 换源会重建 hls，默认会跳回 0 秒，
 * 所以要先把进度记下来、清单加载完再塞回去。
 */
function attach(p: PlayUrl | null, keepTime = false) {
  const v = videoRef.value;
  if (!v || !p) return;
  const resume = keepTime ? v.currentTime : 0;
  destroy();
  err.value = "";
  status.value = "正在加载…";
  // 回放的地址是 m3u8（HLS），走 hls.js
  const src = p.proxy || p.url;
  if (Hls.isSupported()) {
    const h = new Hls({
      // VOD 语义：不开低延迟、不做追帧，让 hls.js 自己按普通点播处理
      enableWorker: true,
      maxBufferLength: 60,
      maxMaxBufferLength: 180,
    });
    hls = h;
    h.loadSource(src);
    h.attachMedia(v);
    h.on(Hls.Events.MANIFEST_PARSED, () => {
      status.value = "";
      if (resume > 1) v.currentTime = resume;
      void v.play().catch(() => {});
    });
    h.on(Hls.Events.ERROR, (_e, data) => {
      if (!data.fatal) return;
      if (data.type === Hls.ErrorTypes.NETWORK_ERROR && retried < 3) {
        // 回放地址不会过期，网络抖动重试即可
        retried++;
        h.startLoad();
        return;
      }
      err.value = "播放失败：" + (data.details || data.type);
      status.value = "";
    });
  } else {
    v.src = src;
    void v.play().catch(() => {});
  }
}

watch(
  () => props.play,
  (p) => {
    retried = 0;
    // 换段/首次进：清晰度重置成最高档（后端默认给的就是最高档）
    curQid.value = p?.qualities?.[0]?.id ?? "";
    attach(p);
  },
);

/** 切清晰度：换源但保住当前进度 */
function pickQuality(q: ReplayQuality) {
  menu.value = "";
  if (q.id === curQid.value) return;
  curQid.value = q.id;
  attach({ url: q.url, proxy: q.proxy, format: "hls", quality: q.name }, true);
}

onMounted(() => {
  if (!curQid.value) curQid.value = props.play?.qualities?.[0]?.id ?? "";
  attach(props.play);
  window.addEventListener("keydown", onKey);
  document.addEventListener("fullscreenchange", onFsChange);
});
onBeforeUnmount(() => {
  destroy();
  if (hideTimer) window.clearTimeout(hideTimer);
  if (dmRaf) cancelAnimationFrame(dmRaf);
  window.removeEventListener("keydown", onKey);
  document.removeEventListener("fullscreenchange", onFsChange);
});

/* ---------------- 控制 ---------------- */

function toggle() {
  const v = videoRef.value;
  if (!v) return;
  if (v.paused) void v.play().catch(() => {});
  else v.pause();
}

function seek(t: number) {
  const v = videoRef.value;
  if (v) v.currentTime = Math.max(0, Math.min(t, dur.value || t));
}

function toggleMute() {
  const v = videoRef.value;
  if (!v) return;
  v.muted = !v.muted;
  muted.value = v.muted;
}

function cycleSpeed() {
  const i = SPEEDS.indexOf(speed.value);
  speed.value = SPEEDS[(i + 1) % SPEEDS.length];
  const v = videoRef.value;
  if (v) v.playbackRate = speed.value;
}

function fullscreen() {
  const el = wrapRef.value;
  if (!el) return;
  if (document.fullscreenElement) void document.exitFullscreen();
  else void el.requestFullscreen().catch(() => {});
}
function onFsChange() {
  cssFull.value = !!document.fullscreenElement;
}

/** 进度条点击/拖动定位 */
function seekAt(e: MouseEvent) {
  const el = e.currentTarget as HTMLElement;
  const r = el.getBoundingClientRect();
  const ratio = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
  seek(ratio * dur.value);
}
function hoverAt(e: MouseEvent) {
  const el = e.currentTarget as HTMLElement;
  const r = el.getBoundingClientRect();
  const ratio = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
  hoverT.value = ratio * dur.value;
}

/** 控制栏：动一下鼠标就显示，2.6 秒后（在播时）自动收起 */
function poke() {
  showCtrl.value = true;
  if (hideTimer) window.clearTimeout(hideTimer);
  hideTimer = window.setTimeout(() => {
    if (playing.value) showCtrl.value = false;
  }, 2600);
}

function onKey(e: KeyboardEvent) {
  const tag = (e.target as HTMLElement)?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return;
  if (e.key === "Escape" && cssFull.value) {
    cssFull.value = false;
    return;
  }
  switch (e.key) {
    case " ":
    case "k":
      e.preventDefault();
      toggle();
      break;
    case "ArrowLeft":
      e.preventDefault();
      seek(cur.value - 5);
      break;
    case "ArrowRight":
      e.preventDefault();
      seek(cur.value + 5);
      break;
    case "ArrowUp":
      e.preventDefault();
      setVolume(Math.min(1, volume.value + 0.05));
      break;
    case "ArrowDown":
      e.preventDefault();
      setVolume(Math.max(0, volume.value - 0.05));
      break;
    case "m":
      toggleMute();
      break;
    case "f":
      fullscreen();
      break;
    default:
      break;
  }
}

function toggleDm() {
  dmOn.value = !dmOn.value;
  localStorage.setItem("junlive.dm.on", dmOn.value ? "1" : "0");
}

function setVolume(v: number) {
  volume.value = v;
  const el = videoRef.value;
  if (el) {
    el.volume = v;
    el.muted = v === 0;
  }
  localStorage.setItem("junlive.volume", String(v));
}

/* ---------------- 视频事件 ---------------- */

function onTimeUpdate() {
  const v = videoRef.value;
  if (!v) return;
  cur.value = v.currentTime;
  bufEnd.value = v.buffered.length ? v.buffered.end(v.buffered.length - 1) : 0;
  pumpDm();
}

function onLoaded() {
  const v = videoRef.value;
  if (!v) return;
  dur.value = isFinite(v.duration) ? v.duration : 0;
  v.volume = volume.value;
  v.playbackRate = speed.value;
}

/* ---------------- 历史弹幕 ---------------- */

interface Flying {
  id: number;
  text: string;
  color: string;
  top: number;
  dur: number;
}
const flying = ref<Flying[]>([]);
let dmSeq = 0;
let dmTrack = 0;
let dmPending: Flying[] = [];
let dmRaf = 0;
const DM_MAX = 80;

function flushDm() {
  dmRaf = 0;
  if (!dmPending.length) return;
  const next = flying.value.concat(dmPending);
  dmPending = [];
  flying.value = next.length > DM_MAX ? next.slice(next.length - DM_MAX) : next;
}

function flyDm(text: string, color: string) {
  if (!dmOn.value || !text.trim()) return;
  const h = wrapRef.value?.clientHeight || 400;
  const cfg = dmCfg.value;
  // 行高跟着字号走，字号调大后不会叠在一起
  const lineH = Math.round(cfg.size * 1.7);
  const tracks = Math.max(1, Math.floor((h * (cfg.area / 100)) / lineH));
  dmTrack = (dmTrack + 1) % tracks;
  const id = ++dmSeq;
  const durS = cfg.dur;
  dmPending.push({ id, text, color, top: dmTrack * lineH + 8, dur: durS });
  if (!dmRaf) dmRaf = requestAnimationFrame(flushDm);
  window.setTimeout(() => {
    flying.value = flying.value.filter((f) => f.id !== id);
  }, durS * 1000 + 300);
}

/** 弹幕按时间轴来，播放到哪就飘到哪 */
const dmList = computed(() =>
  [...(props.danmaku ?? [])].sort((a, b) => a.time - b.time),
);
let dmIdx = 0;
/** 上一次的播放位置，用来识别「跳转」 */
let lastT = 0;

watch(
  () => props.danmaku,
  () => {
    dmIdx = 0;
    lastT = 0;
    flying.value = [];
  },
);

/**
 * 按进度把弹幕飘出来。
 *
 * 关键点：**跳转（前进或后退）时要直接把游标挪过去，不能把中间那堆补飘出来**。
 * `timeupdate` 每 250ms 一次，正常播放位移很小；一旦位移超过 2 秒就是跳转，
 * 比如从第 5 秒拖到第 5083 秒 —— 中间有几百上千条，补飘的话会瞬间刷屏
 * （实测跳一次能飘满 120 条、侧栏涨到 400 多）。
 */
function pumpDm() {
  const list = dmList.value;
  if (!list.length) return;
  const t = cur.value;

  if (Math.abs(t - lastT) > 2) {
    let i = 0;
    while (i < list.length && list[i].time < t) i++;
    dmIdx = i;
    lastT = t;
    return;
  }
  lastT = t;

  let fired = 0;
  while (dmIdx < list.length && list[dmIdx].time <= t && fired < 6) {
    const m = list[dmIdx++];
    fired++;
    flyDm(m.text, m.color);
    emit("dm", m);
  }
}

defineExpose({ seek, toggle, fullscreen });
</script>

<template>
  <div
    ref="wrapRef"
    class="rpp"
    :class="{ hide: !showCtrl, full: cssFull }"
    @mousemove="poke"
  >
    <video
      ref="videoRef"
      class="rpp-vid"
      playsinline
      @click="toggle"
      @dblclick="fullscreen"
      @play="
        playing = true;
        poke();
      "
      @pause="playing = false"
      @timeupdate="onTimeUpdate"
      @loadedmetadata="onLoaded"
      @durationchange="onLoaded"
      @ended="emit('ended')"
      @waiting="status = status || '缓冲中…'"
      @playing="status = ''"
      @error="err = '视频加载失败'"
    ></video>

    <!-- 弹幕层 -->
    <div class="rpp-dm" :style="{ height: dmCfg.area + '%' }">
      <span
        v-for="f in flying"
        :key="f.id"
        class="rpp-dm-item"
        :style="{
          top: f.top + 'px',
          color: f.color || '#fff',
          fontSize: dmCfg.size + 'px',
          opacity: dmCfg.opacity,
          animationDuration: f.dur + 's',
        }"
        >{{ f.text }}</span
      >
    </div>

    <!-- 暂停时的中央播放键。
         除了好看，还能兜住「浏览器拦截自动播放」——那时画面全黑，
         没有这个键用户根本不知道要点哪。 -->
    <div v-if="!playing && !status && !err" class="rpp-big" @click="toggle">
      <span class="rpp-big-ico">▶</span>
    </div>

    <div v-if="status || err" class="rpp-veil">
      <div v-if="!err" class="rpp-spin"></div>
      <span>{{ err || status }}</span>
    </div>

    <!-- 顶部：标题 / 分段位置 / 返回 -->
    <div v-show="showCtrl" class="rpp-top">
      <button class="rpp-back" @click="emit('back')">← 回放列表</button>
      <span v-if="title" class="rpp-title ellipsis" :title="title">{{ title }}</span>
      <span class="rpp-spacer"></span>
      <slot name="top"></slot>
    </div>

    <!-- 控制栏 -->
    <div v-show="showCtrl" class="rpp-ctrl">
      <div
        class="rpp-track"
        @click="seekAt"
        @mousemove="hoverAt"
        @mouseleave="hoverT = null"
      >
        <div class="rpp-buf" :style="{ width: bufPct }"></div>
        <div class="rpp-played" :style="{ width: playedPct }"></div>
        <!-- AI 看点：画成刻度，点一下直接跳过去 -->
        <span
          v-for="(h, i) in highlights ?? []"
          :key="i"
          class="rpp-mark"
          :style="{ left: pct(h.time) }"
          :title="h.title"
          @click.stop="seek(h.time)"
        ></span>
        <div class="rpp-knob" :style="{ left: playedPct }"></div>
        <div
          v-if="hoverT !== null"
          class="rpp-tip"
          :style="{ left: pct(hoverT) }"
        >
          {{ fmt(hoverT) }}
        </div>
      </div>

      <div class="rpp-row">
        <button class="rpp-ico" :title="playing ? '暂停 (空格)' : '播放 (空格)'" @click="toggle">
          {{ playing ? "❚❚" : "▶" }}
        </button>
        <button class="rpp-ico" :title="muted ? '取消静音 (M)' : '静音 (M)'" @click="toggleMute">
          {{ muted || volume === 0 ? "🔇" : "🔊" }}
        </button>
        <input
          class="rpp-vol"
          type="range"
          min="0"
          max="1"
          step="0.02"
          :value="volume"
          @input="setVolume(Number(($event.target as HTMLInputElement).value))"
        />
        <span class="rpp-time">{{ fmt(cur) }} / {{ fmt(dur) }}</span>
        <span class="rpp-spacer"></span>
        <button class="rpp-ico rpp-txt" title="倍速" @click="cycleSpeed">
          {{ speed }}x
        </button>
        <button
          class="rpp-ico rpp-txt"
          :class="{ off: !dmOn }"
          title="弹幕开关（右键打开弹幕设置）"
          @click="toggleDm"
          @contextmenu.prevent="menu = menu === 'dm' ? '' : 'dm'"
        >
          弹
        </button>
        <button
          class="rpp-ico rpp-txt"
          title="弹幕设置"
          @click="menu = menu === 'dm' ? '' : 'dm'"
        >
          弹幕设置
        </button>
        <button
          v-if="chapters.length"
          class="rpp-ico rpp-txt"
          title="AI 看点"
          @click="menu = menu === 'hl' ? '' : 'hl'"
        >
          看点 {{ chapters.length }}
        </button>
        <button
          v-if="quals.length"
          class="rpp-ico rpp-txt"
          title="清晰度"
          @click="menu = menu === 'q' ? '' : 'q'"
        >
          {{ quals.find((q) => q.id === curQid)?.name || "清晰度" }}
        </button>
        <button
          v-if="(segments ?? []).length > 1"
          class="rpp-ico rpp-txt"
          title="选择第几段"
          @click="menu = menu === 'seg' ? '' : 'seg'"
        >
          第 {{ (partIndex ?? 0) + 1 }}/{{ (segments ?? []).length }} 段
        </button>
        <button class="rpp-ico" title="全屏 (F)" @click="fullscreen">⛶</button>
      </div>

      <!-- 弹出菜单 -->
      <div v-if="menu === 'q'" class="rpp-menu">
        <div
          v-for="q in quals"
          :key="q.id"
          class="rpp-mi"
          :class="{ on: q.id === curQid }"
          @click="pickQuality(q)"
        >
          <span>{{ q.name }}</span>
          <span class="rpp-mi-b">{{ Math.round((q.bitrate || 0) / 1000) }}K</span>
        </div>
      </div>
      <div v-if="menu === 'hl'" class="rpp-menu rpp-menu-seg">
        <div
          v-for="(h, i) in chapters"
          :key="i"
          class="rpp-mi rpp-mi-hl"
          :class="{ on: i === curChap }"
          :title="h.desc || h.title"
          @click="
            menu = '';
            seek(h.time);
          "
        >
          <span class="rpp-hl-t">{{ fmt(h.time) }}</span>
          <span class="ellipsis">{{ h.title }}</span>
        </div>
      </div>
      <div v-if="menu === 'dm'" class="rpp-menu rpp-menu-dm">
        <div class="rpp-mrow">
          <span class="rpp-mk">字号</span>
          <span class="rpp-mopts">
            <b
              v-for="o in [
                { l: '小', v: 13 },
                { l: '中', v: 16 },
                { l: '大', v: 19 },
                { l: '特大', v: 23 },
              ]"
              :key="o.v"
              :class="{ on: dmCfg.size === o.v }"
              @click="setDm({ size: o.v })"
              >{{ o.l }}</b
            >
          </span>
        </div>
        <div class="rpp-mrow">
          <span class="rpp-mk">不透明</span>
          <span class="rpp-mopts">
            <b
              v-for="o in [
                { l: '100%', v: 1 },
                { l: '80%', v: 0.8 },
                { l: '60%', v: 0.6 },
                { l: '40%', v: 0.4 },
              ]"
              :key="o.v"
              :class="{ on: dmCfg.opacity === o.v }"
              @click="setDm({ opacity: o.v })"
              >{{ o.l }}</b
            >
          </span>
        </div>
        <div class="rpp-mrow">
          <span class="rpp-mk">速度</span>
          <span class="rpp-mopts">
            <b
              v-for="o in [
                { l: '慢', v: 13 },
                { l: '中', v: 9 },
                { l: '快', v: 6 },
                { l: '极快', v: 4 },
              ]"
              :key="o.v"
              :class="{ on: dmCfg.dur === o.v }"
              @click="setDm({ dur: o.v })"
              >{{ o.l }}</b
            >
          </span>
        </div>
        <div class="rpp-mrow">
          <span class="rpp-mk">显示区域</span>
          <span class="rpp-mopts">
            <b
              v-for="o in [
                { l: '1/4', v: 25 },
                { l: '半屏', v: 50 },
                { l: '3/4', v: 75 },
                { l: '全屏', v: 100 },
              ]"
              :key="o.v"
              :class="{ on: dmCfg.area === o.v }"
              @click="setDm({ area: o.v })"
              >{{ o.l }}</b
            >
          </span>
        </div>
        <div class="rpp-mrow rpp-mrow-end">
          <span
            class="rpp-mreset"
            @click="setDm({ ...DM_DEF })"
            >恢复默认</span
          >
        </div>
      </div>
      <div v-if="menu === 'seg'" class="rpp-menu rpp-menu-seg">
        <div
          v-for="(sg, i) in segments ?? []"
          :key="i"
          class="rpp-mi"
          :class="{ on: i === (partIndex ?? 0) }"
          :title="sg"
          @click="
            menu = '';
            if (i !== (partIndex ?? 0)) emit('part', i);
          "
        >
          <span class="ellipsis">第 {{ i + 1 }} 段 · {{ sg }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.rpp {
  position: relative;
  width: 100%;
  height: 100%;
  background: #000;
  overflow: hidden;
  user-select: none;
}
.rpp-vid {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
  background: #000;
}

/* ---------------- 弹幕 ---------------- */
.rpp-dm {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  overflow: hidden;
  pointer-events: none;
}
.rpp-dm-item {
  position: absolute;
  left: 100%;
  white-space: nowrap;
  font-size: 15px;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.9);
  animation-name: rpp-fly;
  animation-timing-function: linear;
  animation-fill-mode: forwards;
  will-change: transform;
}
@keyframes rpp-fly {
  from {
    transform: translateX(0);
  }
  to {
    transform: translateX(calc(-100vw - 100%));
  }
}

/* 中央播放键 */
.rpp-big {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.rpp-big-ico {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 62px;
  height: 62px;
  padding-left: 5px;
  font-size: 22px;
  color: #fff;
  background: rgba(0, 0, 0, 0.5);
  border-radius: 50%;
  backdrop-filter: blur(2px);
  transition: transform 0.15s, background 0.15s;
}
.rpp-big:hover .rpp-big-ico {
  transform: scale(1.08);
  background: var(--brand);
}

/* ---------------- 遮罩 ---------------- */
.rpp-veil {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  font-size: 13px;
  color: #e6e9ef;
  background: rgba(0, 0, 0, 0.45);
}
.rpp-spin {
  width: 26px;
  height: 26px;
  border: 2px solid rgba(255, 255, 255, 0.25);
  border-top-color: var(--brand);
  border-radius: 50%;
  animation: rpp-spin 0.8s linear infinite;
}
@keyframes rpp-spin {
  to {
    transform: rotate(360deg);
  }
}

/* ---------------- 顶部 ---------------- */
.rpp-top {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: linear-gradient(rgba(0, 0, 0, 0.6), transparent);
  transition: opacity 0.2s;
}
.rpp-back {
  flex: none;
  height: 28px;
  padding: 0 12px;
  font-size: 12.5px;
  color: #fff;
  background: rgba(0, 0, 0, 0.5);
  border: 0;
  border-radius: 999px;
  cursor: pointer;
}
.rpp-back:hover {
  background: rgba(0, 0, 0, 0.75);
}
.rpp-title {
  min-width: 0;
  font-size: 13px;
  color: #fff;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8);
}
.rpp-spacer {
  flex: 1;
  min-width: 0;
}

/* ---------------- 弹出菜单 ---------------- */
.rpp-menu {
  position: absolute;
  right: 12px;
  bottom: 44px;
  min-width: 150px;
  max-height: 260px;
  overflow-y: auto;
  padding: 4px;
  background: rgba(20, 20, 22, 0.96);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 8px;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.45);
}
.rpp-menu-seg {
  min-width: 240px;
  max-width: 340px;
}
.rpp-mi {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 6px 9px;
  font-size: 12.5px;
  color: #e6e9ef;
  border-radius: 5px;
  cursor: pointer;
}
.rpp-mi:hover {
  background: rgba(255, 255, 255, 0.12);
}
.rpp-mi.on {
  color: var(--brand);
  font-weight: 600;
}
.rpp-mi-b {
  flex: none;
  font-size: 11px;
  color: #8a8f99;
}
.rpp-mi-hl {
  justify-content: flex-start;
  gap: 8px;
}
.rpp-hl-t {
  flex: none;
  color: #ffd666;
  font-variant-numeric: tabular-nums;
}
/* 弹幕设置 */
.rpp-menu-dm {
  min-width: 250px;
  padding: 8px 10px;
}
.rpp-mrow {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 0;
}
.rpp-mrow-end {
  justify-content: flex-end;
  padding-top: 2px;
}
.rpp-mk {
  flex: none;
  width: 52px;
  font-size: 12px;
  color: #9aa0aa;
}
.rpp-mopts {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}
.rpp-mopts b {
  padding: 3px 9px;
  font-size: 12px;
  font-weight: 400;
  color: #cfd4dc;
  background: rgba(255, 255, 255, 0.08);
  border-radius: 5px;
  cursor: pointer;
}
.rpp-mopts b:hover {
  background: rgba(255, 255, 255, 0.18);
}
.rpp-mopts b.on {
  color: #fff;
  background: var(--brand);
}
.rpp-mreset {
  font-size: 11.5px;
  color: #8a8f99;
  cursor: pointer;
}
.rpp-mreset:hover {
  color: var(--brand);
}

/* ---------------- 控制栏 ---------------- */
.rpp-ctrl {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  padding: 0 12px 8px;
  background: linear-gradient(transparent, rgba(0, 0, 0, 0.72));
  transition: opacity 0.2s;
}
.rpp.hide .rpp-ctrl,
.rpp.hide .rpp-top {
  opacity: 0;
  pointer-events: none;
}
.rpp-track {
  position: relative;
  height: 16px;
  margin-bottom: 2px;
  cursor: pointer;
}
/* 轨道本体做成细线，hover 时加粗（跟主流播放器一致） */
.rpp-track::before {
  content: "";
  position: absolute;
  left: 0;
  right: 0;
  top: 6px;
  height: 4px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.24);
  transition: height 0.12s, top 0.12s;
}
.rpp-track:hover::before {
  top: 5px;
  height: 6px;
}
.rpp-buf,
.rpp-played {
  position: absolute;
  top: 6px;
  left: 0;
  height: 4px;
  border-radius: 2px;
  transition: height 0.12s, top 0.12s;
}
.rpp-buf {
  background: rgba(255, 255, 255, 0.4);
}
.rpp-played {
  background: var(--brand);
}
.rpp-track:hover .rpp-buf,
.rpp-track:hover .rpp-played {
  top: 5px;
  height: 6px;
}
.rpp-knob {
  position: absolute;
  top: 8px;
  width: 11px;
  height: 11px;
  margin-left: -5.5px;
  border-radius: 50%;
  background: #fff;
  opacity: 0;
  transition: opacity 0.12s;
}
.rpp-track:hover .rpp-knob {
  opacity: 1;
}
/* AI 看点刻度 */
.rpp-mark {
  position: absolute;
  top: 3px;
  width: 3px;
  height: 10px;
  margin-left: -1.5px;
  border-radius: 1px;
  background: #ffd666;
  cursor: pointer;
}
.rpp-mark:hover {
  background: #fff;
  height: 14px;
  top: 1px;
}
.rpp-tip {
  position: absolute;
  bottom: 20px;
  transform: translateX(-50%);
  padding: 2px 7px;
  font-size: 11.5px;
  color: #fff;
  background: rgba(0, 0, 0, 0.8);
  border-radius: 4px;
  pointer-events: none;
  font-variant-numeric: tabular-nums;
}
.rpp-row {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #fff;
}
.rpp-ico {
  flex: none;
  min-width: 26px;
  height: 26px;
  padding: 0 6px;
  font-size: 13px;
  color: #fff;
  background: transparent;
  border: 0;
  border-radius: 6px;
  cursor: pointer;
}
.rpp-ico:hover {
  background: rgba(255, 255, 255, 0.16);
}
.rpp-txt {
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.rpp-txt.off {
  opacity: 0.45;
}
.rpp-vol {
  flex: none;
  width: 76px;
  accent-color: var(--brand);
}
.rpp-time {
  flex: none;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.7);
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
