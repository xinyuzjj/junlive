<script setup lang="ts">
/**
 * 移动端播放页 —— 独立实现，不复用 PC 的 Room.vue。
 *
 * PC 版是「顶部信息栏 + 左播放器 + 右 288px 弹幕栏」，手机上这么排两边都废。
 * 移动版：
 *   播放器  固定 16:9 贴顶，黑底，不占满屏（否则弹幕没地方看）
 *   弹幕    在下方，占满剩余高度、可滚动
 *   信息    压成一行（主播名 + 在线数 + 关注），点开才展开分区/房间号
 *   线路    画质做成底部弹出选择器，不用 PC 的下拉菜单
 */
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  getRoom,
  setStreamRenew,
  startDanmaku,
  stopDanmaku,
  thumbUrl,
  type DanmakuMsg,
  type PlayUrl,
  type RoomDetail,
} from "../api";
import { listen } from "@tauri-apps/api/event";
import Player from "../components/Player.vue";
import { store } from "../store";

const props = defineProps<{ platform: string; id: string }>();
const router = useRouter();

const detail = ref<RoomDetail | null>(null);
const current = ref<PlayUrl | null>(null);
const msgs = ref<DanmakuMsg[]>([]);
const loading = ref(true);
const error = ref("");
const showQ = ref(false);
const showInfo = ref(false);
const listRef = ref<HTMLElement | null>(null);
/** 播放器舞台：手势只挂在它上面，下方弹幕列表在另一层，滚动不受影响 */
const stageRef = ref<HTMLElement | null>(null);

/**
 * 移动端全屏。
 *
 * 手机上要的是「**横屏铺满整个屏幕 + 弹幕浮在画面上面**」，不是 PC 那种
 * 对播放器容器全屏。实现上给 .mr 加个 full 类：舞台变成 fixed 铺满，
 * 视频 object-fit: contain 保持比例，弹幕层改成浮层覆盖在画面上，
 * 并显示一个能点的退出按钮（手机上没有 ESC 键，必须有可见的退出入口）。
 */
const full = ref(false);
/** 是否横屏。全屏时竖屏状态要提示用户转过来 —— 手机看直播横着画面才足 */
const landscape = ref(false);
/** 全屏时控件是否可见（沉浸式：几秒后自动隐藏，点一下再出来） */
const ctrlOn = ref(true);
let ctrlTimer: number | null = null;

function syncOrient() {
  landscape.value = window.innerWidth > window.innerHeight;
}

function pokeCtrl() {
  ctrlOn.value = true;
  if (ctrlTimer) window.clearTimeout(ctrlTimer);
  ctrlTimer = window.setTimeout(() => {
    if (full.value) ctrlOn.value = false;
  }, 3000);
}

async function toggleFull() {
  full.value = !full.value;
  if (full.value) {
    pokeCtrl();
    // 锁横屏：这是手机看直播的主场景，横着画面才足。
    // 失败也不影响（iOS Safari 不支持 lock；部分安卓要用户手势后才行）
    try {
      const so = screen.orientation as unknown as { lock?: (o: string) => Promise<void> };
      await so?.lock?.("landscape");
    } catch {
      /* 锁不住就靠用户自己转，下面会给提示 */
    }
  } else {
    if (ctrlTimer) window.clearTimeout(ctrlTimer);
    try {
      (screen.orientation as unknown as { unlock?: () => void }).unlock?.();
    } catch {
      /* 忽略 */
    }
  }
  syncOrient();
}

/* ------------------------------------------------------------------ 手势控制
 * 手机上最顺手的操作方式：
 *   左半屏上下滑 → 亮度（仅移动端，原理见 applyBright）
 *   右半屏上下滑 → 音量
 *   横滑         → 进度（直播没有时间轴，给提示）
 * 滑动时画面中央弹一个小浮层显示当前数值，松手 1 秒后消失。
 * 监听只挂在 .mr-stage（画面区域）上，下面的弹幕列表是另一层，滚动不受影响。
 */
const gTip = ref<{ icon: string; text: string } | null>(null);
/** 模拟亮度值（不是系统亮度，见 applyBright） */
const bright = ref(1);
const BRIGHT_MIN = 0.4;
const BRIGHT_MAX = 1.3;
/** 小于这个总位移当点按处理 —— 手指轻微抖动不该触发手势 */
const GESTURE_MIN = 12;

let gStartX = 0;
let gStartY = 0;
/** 手势开始时的基准值：音量 / 亮度 / 播放进度 */
let gStartVal = 0;
let gSeekTo = 0;
let gTipTimer: number | null = null;
type GKind = "none" | "volume" | "bright" | "progress" | "live" | "ignore";
let gKind: GKind = "none";

function stageVideo(): HTMLVideoElement | null {
  return stageRef.value?.querySelector("video") ?? null;
}

/** 移动端判定：桌面窗口下没有「亮度」这个概念，亮度手势直接跳过 */
function isMobile(): boolean {
  return window.innerWidth <= 820;
}

function showTip(icon: string, text: string) {
  gTip.value = { icon, text };
  if (gTipTimer) {
    window.clearTimeout(gTipTimer);
    gTipTimer = null;
  }
}

/** 松手后延迟收起浮层（松手 1 秒后自动消失） */
function hideTipSoon() {
  if (gTipTimer) window.clearTimeout(gTipTimer);
  gTipTimer = window.setTimeout(() => {
    gTip.value = null;
    gTipTimer = null;
  }, 1000);
}

function fmtSec(t: number): string {
  if (!isFinite(t) || t < 0) t = 0;
  const m = Math.floor(t / 60);
  const s = Math.floor(t % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

/**
 * 模拟亮度。
 *
 * ⚠️ 这不是系统亮度：Tauri 桌面端 / 浏览器都没有「调屏幕背光」的 API，
 * 真机上想调系统亮度必须额外引 Tauri 插件，而本项目不引入任何新依赖。
 * 所以这里用 CSS filter: brightness() 作用在 <video> 上做近似 ——
 * 只是把画面本身调亮/调暗，改不了屏幕背光，也管不到系统其它界面。
 * 这是刻意的近似，不是 bug。桌面窗口下不做亮度手势（桌面没这个概念）。
 */
function applyBright(v: number) {
  bright.value = Math.min(BRIGHT_MAX, Math.max(BRIGHT_MIN, v));
  const el = stageVideo();
  if (el) el.style.filter = `brightness(${bright.value})`;
}

function onStageTouchStart(e: TouchEvent) {
  const t = e.touches[0];
  if (!t) return;
  gStartX = t.clientX;
  gStartY = t.clientY;
  gKind = "none";
}

function onStageTouchMove(e: TouchEvent) {
  const t = e.touches[0];
  if (!t) return;
  const dx = t.clientX - gStartX;
  const dy = t.clientY - gStartY;

  // 首次超过阈值时才判定手势类型，此后这一轮触摸锁定该类型
  if (gKind === "none") {
    if (Math.abs(dx) < GESTURE_MIN && Math.abs(dy) < GESTURE_MIN) return;
    const v = stageVideo();
    if (!v) return;
    if (Math.abs(dx) > Math.abs(dy)) {
      // 横向 → 进度。直播没有可拖的时间轴，给提示就够，别假装能拖
      const live = !isFinite(v.duration) || v.duration <= 0;
      if (live) {
        gKind = "live";
        showTip("🚫", "直播不支持拖动");
        return;
      }
      gKind = "progress";
      gStartVal = v.currentTime;
      gSeekTo = v.currentTime;
    } else if (gStartX < window.innerWidth / 2) {
      // 起点在左半边 → 亮度；桌面跳过并且不显示浮层
      if (!isMobile()) {
        gKind = "ignore";
        return;
      }
      gKind = "bright";
      gStartVal = bright.value;
    } else {
      // 起点在右半边 → 音量
      gKind = "volume";
      gStartVal = Math.min(1, Math.max(0, v.volume));
    }
  }

  if (gKind === "volume" || gKind === "bright") {
    const h = stageRef.value?.clientHeight || window.innerHeight;
    // 往上滑是增大，所以取 -dy；滑半个屏高对应调满/调到底
    const ratio = -dy / Math.max(1, h * 0.5);
    if (gKind === "volume") {
      const nv = Math.min(1, Math.max(0, gStartVal + ratio));
      const vv = stageVideo();
      if (vv) {
        vv.volume = nv;
        vv.muted = nv === 0;
      }
      showTip(nv === 0 ? "🔇" : "🔊", `音量 ${Math.round(nv * 100)}%`);
    } else {
      applyBright(gStartVal + ratio * (BRIGHT_MAX - BRIGHT_MIN));
      const pct = Math.round(((bright.value - BRIGHT_MIN) / (BRIGHT_MAX - BRIGHT_MIN)) * 100);
      showTip("☀", `亮度 ${pct}%`);
    }
  } else if (gKind === "progress") {
    const v = stageVideo();
    if (!v) return;
    const w = stageRef.value?.clientWidth || window.innerWidth;
    const dur = v.duration || 0;
    // 横滑满整宽 ≈ 拖动 min(时长, 300s)，避免长片一划就飞到结尾
    const span = Math.min(dur, 300);
    gSeekTo = Math.min(dur, Math.max(0, gStartVal + (dx / Math.max(1, w)) * span));
    showTip("⏩", `${fmtSec(gSeekTo)} / ${fmtSec(dur)}`);
  }
}

function onStageTouchEnd() {
  // 进度手势松手才真正跳转 —— 滑动途中只更新预览，避免反复 seek 卡顿
  if (gKind === "progress") {
    const v = stageVideo();
    if (v && isFinite(gSeekTo)) v.currentTime = gSeekTo;
  }
  gKind = "none";
  hideTipSoon();
}

let unlisten: (() => void) | null = null;

/** 观看人数是字符串（各平台单位不一），统一转成「万」显示 */
function fmt(raw: string | undefined): string {
  const n = Number(raw || 0);
  if (!n) return "";
  return n >= 10000 ? (n / 10000).toFixed(1).replace(/\.0$/, "") + "万" : String(n);
}

function scrollBottom() {
  const el = listRef.value;
  if (el) el.scrollTop = el.scrollHeight;
}

/**
 * 封面海报加载失败兜底：把破图藏起来，露出舞台黑底，
 * 避免加载中/未开播时出现破图图标。
 */
function coverErr(e: Event) {
  const el = e.target as HTMLImageElement;
  el.style.visibility = "hidden";
}

function pick(p: PlayUrl) {
  current.value = p;
  showQ.value = false;
  if (detail.value) setStreamRenew(props.platform, detail.value.room_id, p.quality).catch(() => {});
}

async function onRefresh() {
  try {
    const d = await getRoom(props.platform, detail.value?.room_id || decodeURIComponent(props.id));
    detail.value = d;
    if (!d.plays.length) return;
    const q = current.value?.quality;
    current.value = { ...(d.plays.find((p) => p.quality === q) ?? d.plays[0]) };
    setStreamRenew(props.platform, d.room_id, current.value.quality).catch(() => {});
  } catch {
    /* 维持原样，播放器会显示错误 */
  }
}

function toggleFollow() {
  const d = detail.value;
  if (!d) return;
  store.toggleFollow(d); // detail 本身就是 Room 形状，store 只读 platform / room_id
}

onMounted(async () => {
  syncOrient();
  window.addEventListener("resize", syncOrient);
  window.addEventListener("orientationchange", syncOrient);
  let rid = decodeURIComponent(props.id);
  try {
    const d = await getRoom(props.platform, rid);
    detail.value = d;
    rid = d.room_id || rid;
    current.value = d.plays[0] ?? null;
    if (!d.live) error.value = "该主播当前未开播";
    else if (!d.plays.length) error.value = "没有解析到可用的播放地址";
    if (d.room_id) setStreamRenew(props.platform, d.room_id, current.value?.quality ?? "").catch(() => {});
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }

  try {
    unlisten = await listen<DanmakuMsg>("danmaku", (e) => {
      msgs.value.push(e.payload);
      if (msgs.value.length > 400) msgs.value.splice(0, 150);
      scrollBottom();
    });
    await startDanmaku(props.platform, rid);
  } catch {
    /* 弹幕失败不影响看播 */
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", syncOrient);
  window.removeEventListener("orientationchange", syncOrient);
  if (ctrlTimer) window.clearTimeout(ctrlTimer);
  if (gTipTimer) window.clearTimeout(gTipTimer);
  unlisten?.();
  stopDanmaku().catch(() => {});
  setStreamRenew("", "", "").catch(() => {});
});
</script>

<template>
  <div class="mr" :class="{ full, landscape }" @click="full && pokeCtrl()">
    <!-- 播放器：固定 16:9 贴顶。
         手势（亮度/音量/进度）挂在这一层，只作用于画面区域 -->
    <div
      ref="stageRef"
      class="mr-stage"
      @touchstart="onStageTouchStart"
      @touchmove="onStageTouchMove"
      @touchend="onStageTouchEnd"
      @touchcancel="onStageTouchEnd"
    >
      <!-- 封面海报：拉流还没出画面时（加载中 / 未开播 / 出错）先用缩略图垫底，
           走 thumbUrl 只拉 400px 的小图省流量；播放器一有画面就会盖住它 -->
      <img
        v-if="detail?.cover"
        class="mr-cover"
        :src="thumbUrl(props.platform, detail.cover, 400)"
        referrerpolicy="no-referrer"
        alt=""
        @error="coverErr"
      />
      <Player
        v-if="current"
        :play="current"
        :plays="detail?.plays ?? []"
        :danmaku="msgs"
        :platform="props.platform"
        :room-id="props.id"
        @pick="pick"
        @refresh="onRefresh"
      />
      <div v-if="loading" class="mr-veil">加载中…</div>
      <div v-else-if="error" class="mr-veil err">{{ error }}</div>
    </div>

    <!-- 信息条：一行，点开才展开 -->
    <div class="mr-bar" @click="showInfo = !showInfo">
      <button class="mr-back" @click.stop="router.back()">‹</button>
      <div class="mr-meta">
        <div class="mr-name">{{ detail?.streamer || "-" }}</div>
        <div class="mr-sub">
          <span v-if="detail?.online">👁 {{ fmt(detail.online) }}</span>
          <span v-if="detail?.area" class="mr-cat">{{ detail.area }}</span>
        </div>
      </div>
      <button class="mr-q" @click.stop="showQ = true">{{ current?.quality || "画质" }} ▾</button>

      <!-- 全屏：手机上看直播最常见的操作，必须放在一行里能直接点到 -->
      <button class="mr-fs" :title="full ? '退出全屏' : '全屏（横屏铺满）'" @click.stop="toggleFull">
        <svg viewBox="0 0 24 24" width="19" height="19" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <template v-if="!full">
            <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />
          </template>
          <template v-else>
            <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" />
          </template>
        </svg>
      </button>

      <button class="mr-fav" :class="{ on: detail && store.isFollowed(props.platform, detail.room_id) }" @click.stop="toggleFollow">
        {{ detail && store.isFollowed(props.platform, detail.room_id) ? "★" : "☆" }}
      </button>
    </div>

    <!-- 全屏时的退出入口。手机上没有 ESC 键，必须给一个看得见、点得到的按钮。
         沉浸式：3 秒后自动淡出（不挡画面），点屏幕任意位置再出来。 -->
    <button v-if="full" class="mr-exit" :class="{ gone: !ctrlOn }" @click.stop="toggleFull">
      ✕ 退出全屏
    </button>

    <!-- 手势浮层：fixed 定位、不参与布局、不挡触摸（pointer-events:none）。
         z-index 8050 —— 高于全屏舞台(8000)，低于退出按钮(8100)，退出键始终可点 -->
    <div v-if="gTip" class="mr-gesture">
      <span class="mr-gesture-i">{{ gTip.icon }}</span>
      <span class="mr-gesture-t">{{ gTip.text }}</span>
    </div>

    <!-- 竖屏提示：全屏锁横屏失败时（iOS / 系统锁了旋转），提示用户手动转过来 -->
    <div v-if="full && !landscape" class="mr-rotate">
      <span class="mr-rotate-i">📱↻</span>
      <span>横过来看，画面更足</span>
    </div>

    <div v-if="showInfo" class="mr-info">
      <div class="mr-info-t">{{ detail?.title }}</div>
      <div class="mr-info-r">房间号 {{ detail?.room_id }} · {{ props.platform }}</div>
    </div>

    <!-- 弹幕：占满剩余高度 -->
    <div ref="listRef" class="mr-dm">
      <div v-if="!msgs.length" class="mr-dm-empty">正在连接弹幕…</div>
      <div v-for="(m, i) in msgs" :key="i" class="mr-dm-row">
        <span class="mr-dm-u" :style="{ color: m.color }">{{ m.user }}</span>
        <span class="mr-dm-t">{{ m.text }}</span>
      </div>
    </div>

    <!-- 画质选择：底部弹出 -->
    <div v-if="showQ" class="mr-sheet" @click.self="showQ = false">
      <div class="mr-sheet-in">
        <div class="mr-sheet-head">选择画质</div>
        <button
          v-for="p in detail?.plays ?? []"
          :key="p.url"
          class="mr-sheet-item"
          :class="{ on: p.quality === current?.quality }"
          @click="pick(p)"
        >
          {{ p.quality }}<span v-if="p.quality === current?.quality" class="tick">✓</span>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.mr {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
  overflow: hidden;
}

/* 16:9 贴顶，不占满屏 —— 否则下面的弹幕就没地方看了 */
.mr-stage {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  flex: none;
  background: #000;
}
/* 封面海报：铺满舞台，垫在播放器 / 加载遮罩下面 */
.mr-cover {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.mr-veil {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  font-size: 13px;
  background: rgba(0, 0, 0, 0.5);
}
.mr-veil.err {
  color: #ffb4b4;
  padding: 0 20px;
  text-align: center;
}

.mr-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.mr-back {
  width: 34px;
  height: 34px;
  border: 0;
  background: none;
  font-size: 24px;
  line-height: 1;
  color: var(--fg);
  flex-shrink: 0;
}
.mr-meta {
  flex: 1;
  min-width: 0;
}
.mr-name {
  font-size: 14px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mr-sub {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--fg-dim);
}
.mr-q {
  height: 32px;
  padding: 0 10px;
  border: 1px solid var(--border-2);
  border-radius: 6px;
  background: var(--chip);
  color: var(--fg-2);
  font-size: 12px;
  flex-shrink: 0;
}
.mr-fav {
  width: 38px;
  height: 34px;
  border: 0;
  background: none;
  font-size: 20px;
  color: var(--fg-dim);
  flex-shrink: 0;
}
.mr-fav.on {
  color: #f5a623;
}
/* 全屏按钮：手机上一行里的高频操作，热区按 44px 标准做 */
.mr-fs {
  width: 38px;
  height: 38px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: var(--chip);
  color: var(--fg-2);
  flex-shrink: 0;
}
.mr-fs:active {
  background: var(--chip-hover);
}

/* ---------- 全屏（横屏铺满） ----------
   手机要的是「整个屏幕都是画面 + 弹幕浮在上面」，不是 PC 那种对容器全屏。
   做法：整个 .mr 变成全屏定位，舞台铺满，常规的信息条/弹幕列表隐藏，
   弹幕交给 Player 组件自带的飞屏层（它本来就浮在视频上面）。 */
.mr.full {
  position: fixed;
  inset: 0;
  z-index: 8000;
  background: #000;
}
.mr.full .mr-stage {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  aspect-ratio: auto;
}
.mr.full .mr-bar,
.mr.full .mr-info,
.mr.full .mr-dm {
  display: none; /* 全屏时只看画面，滚动弹幕列表先收起来 */
}

/* 横屏全屏：把播放器容器强制铺满 viewport（100vw × 100vh）。
   Player 自带的飞屏弹幕层是相对播放器 inset:0 的，只有容器铺满它才跟着铺满整屏。
   为什么只在横屏铺满：手机横屏时 16:9 画面刚好吃掉整屏，弹幕能横跨整个画面；
   纵屏若也铺满，16:9 会被拉扁、或因 object-fit: contain 留出很宽的黑边，
   弹幕飘在黑边上很突兀。所以纵屏保持现状（画面 16:9 居中、上下留黑）。 */
.mr.full.landscape .mr-stage :deep(.player) {
  width: 100vw;
  height: 100vh;
  border-radius: 0;
}

.mr-exit {
  position: fixed;
  left: 12px;
  top: calc(12px + env(safe-area-inset-top, 0));
  z-index: 8100;
  height: 36px;
  padding: 0 14px;
  border: 0;
  border-radius: 18px;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  font-size: 13px;
  transition: opacity 0.25s;
}
/* 沉浸式：3 秒后淡出，不挡画面 */
.mr-exit.gone {
  opacity: 0;
  pointer-events: none;
}

/* 手势浮层：画面中央的小提示。fixed 定位不参与布局，pointer-events:none 不拦触摸，
   z-index 8050 夹在全屏舞台(8000)与退出按钮(8100)之间，退出键不会被它盖住。 */
.mr-gesture {
  position: fixed;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  z-index: 8050;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 14px 22px;
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.62);
  color: var(--fg);
  font-size: 13px;
  pointer-events: none;
  user-select: none;
}
.mr-gesture-i {
  font-size: 26px;
  line-height: 1;
}
.mr-gesture-t {
  font-variant-numeric: tabular-nums;
}

/* 竖屏提示（全屏锁横屏失败时出现） */
.mr-rotate {
  position: fixed;
  inset: 0;
  z-index: 8150;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  background: rgba(0, 0, 0, 0.78);
  color: #fff;
  font-size: 14px;
}
.mr-rotate-i {
  font-size: 40px;
  animation: mr-rot 1.8s ease-in-out infinite;
}
@keyframes mr-rot {
  0%, 40% { transform: rotate(0deg); }
  60%, 100% { transform: rotate(90deg); }
}

.mr-info {
  padding: 8px 12px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.mr-info-t {
  font-size: 13px;
  color: var(--fg-2);
}
.mr-info-r {
  margin-top: 2px;
  font-size: 11px;
  color: var(--fg-dim);
}

.mr-dm {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  padding: 6px 12px 12px;
  font-size: 14px;
  line-height: 1.55;
}
.mr-dm-empty {
  padding: 20px;
  text-align: center;
  color: var(--fg-dim);
  font-size: 13px;
}
.mr-dm-row {
  display: flex;
  gap: 6px;
  padding: 2px 0;
  word-break: break-word;
}
.mr-dm-u {
  flex-shrink: 0;
  opacity: 0.9;
}
.mr-dm-t {
  color: var(--fg-2);
}

.mr-sheet {
  position: fixed;
  inset: 0;
  z-index: 9000;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: flex-end;
}
.mr-sheet-in {
  width: 100%;
  background: var(--panel);
  border-radius: 14px 14px 0 0;
  padding: 8px 0 calc(8px + env(safe-area-inset-bottom, 0));
  max-height: 60vh;
  overflow-y: auto;
}
.mr-sheet-head {
  padding: 10px 18px;
  font-size: 13px;
  color: var(--fg-dim);
}
.mr-sheet-item {
  display: flex;
  align-items: center;
  width: 100%;
  height: 48px;
  padding: 0 18px;
  border: 0;
  background: none;
  font-size: 15px;
  color: var(--fg);
}
.mr-sheet-item.on {
  color: var(--brand);
  font-weight: 600;
}
.mr-sheet-item .tick {
  margin-left: auto;
}
</style>
