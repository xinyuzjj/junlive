<script setup lang="ts">
import { listen } from "@tauri-apps/api/event";
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  getRoom,
  startDanmaku,
  stopDanmaku,
  setStreamRenew,
  type DanmakuMsg,
  type PlayUrl,
  type RoomDetail,
} from "../api";
import { store } from "../store";
import { chatSrc, isEmbedPlatform } from "../embed";
import {
  getReplayDanmaku,
  getReplayHighlights,
  getReplayParts,
  getReplays,
  resolveReplay,
  type Replay,
  type ReplayDanmaku,
  type ReplayHighlight,
} from "../api";
import Player from "../components/Player.vue";
import ReplayPlayer from "../components/ReplayPlayer.vue";

const props = defineProps<{ platform: string; id: string }>();
const router = useRouter();

/**
 * YouTube 弹幕走官方 live_chat iframe。
 *
 * 自建弹幕这条路走不通：YouTube 对 live chat 接口同样有 PO token / bot 校验，
 * 官方 iframe 自己处理，且不需要登录就能看（只读）。
 * embed_domain 必须和父页面同域，否则 YouTube 会拒绝渲染。
 */
// 油管聊天 iframe：只在**确实在直播**时显示，并且用**解析后**的视频 ID ——
// 路由里的 ID 可能是频道或已过期的视频，直接拿去拼 live_chat 会指向错的东西。
const isYtChat = computed(() => props.platform === "youtube" && !!detail.value?.live);
const ytChatSrc = computed(() =>
  chatSrc(props.platform, detail.value?.room_id || props.id),
);

/** 官方 iframe 播放器不需要我们解析播放地址，缺 plays 不该报错 */
const isEmbed = computed(() => isEmbedPlatform(props.platform));

const detail = ref<RoomDetail | null>(null);
const current = ref<PlayUrl | null>(null);
const loading = ref(true);
const error = ref("");

const msgs = ref<DanmakuMsg[]>([]);

/* ---------------- 直播回放（斗鱼） ----------------
 *
 * 斗鱼的历史场次走的是另一套域名（v.douyu.com），接口要 up_id。
 * 列表用我们自己的 UI，播放**内嵌官方回放页** ——
 * 官方取流接口要签名（不签名返回「权限不足」），内嵌页没有 X-Frame-Options
 * 限制，是唯一不用逆向签名的干净路径。
 */
const replayOpen = ref(false);
const replays = ref<Replay[]>([]);
const replayLoading = ref(false);
const replayErr = ref("");
/** 总场次（接口给的 count，实测这个主播有 2237 场） */
const replayTotal = ref(0);
/** 当前翻到第几页（斗鱼每页锁死 20 条） */
const replayPage = ref(1);
/** 每页条数（斗鱼服务端锁死的，改不了） */
const RP_PER = 20;
const replayPages = computed(() =>
  Math.max(1, Math.ceil(replayTotal.value / RP_PER)),
);
/** 正在跳转（按日期二分查找时要翻好几页） */
const replayJumping = ref(false);
/** 当前页的时间跨度，显示成 "2026-10-08 ~ 2026-09-11" */
const replaySpan = computed(() => {
  const l = replays.value;
  if (!l.length) return "";
  const a = dstr(l[0]?.time || "");
  const b = dstr(l[l.length - 1]?.time || "");
  return a && b ? (a === b ? a : `${a} ~ ${b}`) : "";
});

/** "2026-10-08 13点场" → "2026-10-08" */
function dstr(t: string) {
  return (t || "").slice(0, 10);
}
/** 正在播的回放；非空时舞台切成回放播放器 */
const curReplay = ref<Replay | null>(null);
/** 解析出来的回放播放地址（带签名的 m3u8，已走本地代理） */
const replayPlay = ref<PlayUrl | null>(null);
const canReplay = computed(() => props.platform === "douyu");
/** 整场的分段（一场直播被切成若干 2 小时的段） */
const replayParts = ref<Replay[]>([]);
/** 当前在第几段（0 基） */
const replayPartIdx = ref(0);
/** 历史弹幕（按视频进度飘） */
const replayDms = ref<ReplayDanmaku[]>([]);
/** AI 看点（点了跳到对应位置） */
const replayHls = ref<ReplayHighlight[]>([]);

/** "02:00:05" / "120:05" → 秒 */
function durSecs(s?: string) {
  const p = (s || "").split(":").map((x) => Number(x.trim()));
  if (p.some((x) => !isFinite(x))) return 0;
  if (p.length === 3) return p[0] * 3600 + p[1] * 60 + p[2];
  if (p.length === 2) return p[0] * 60 + p[1];
  return 0;
}

/** 拉某一段的弹幕（`base` = 该段起始 Unix 秒，用来把绝对时间戳换算成段内进度） */
function loadDm(hashId: string, base: number) {
  replayDms.value = [];
  void getReplayDanmaku(props.platform, hashId, base)
    .then((ms) => {
      replayDms.value = ms;
    })
    .catch(() => {});
}

/**
 * 拉某一段的 AI 看点。
 *
 * **必须带这一段的起点和时长**：看点接口给的是整场的（实测跨 11 小时），
 * 而一段只有 2 小时；不带的话超出的那些会被当成段内时间画到进度条外面去。
 */
function loadHl(hashId: string, base: number, dur: number) {
  replayHls.value = [];
  void getReplayHighlights(props.platform, hashId, base, dur)
    .then((hs) => {
      replayHls.value = hs;
    })
    .catch(() => {});
}

/** 飘出一条历史弹幕：同步进侧栏的弹幕列表（视频上的飘幕由播放器自己画） */
function onTimedDm(m: ReplayDanmaku) {
  msgs.value.push({
    platform: props.platform,
    user: m.user,
    text: m.text,
    color: m.color,
    kind: "chat",
    ts: Math.floor(m.time),
  });
}

/** 播完一段自动接下一段 —— 一场直播是切开的，不接的话看完 2 小时就停了 */
function onReplayEnded() {
  const next = replayPartIdx.value + 1;
  if (next < replayParts.value.length) void playPart(next);
}

async function openReplays() {
  // 已经开着就当作「回到列表」（播放器的返回按钮走这里）
  if (replayOpen.value) return;
  replayOpen.value = true;
  // 每次都从第一页重来：不然会拿着上次翻到一半的列表
  replays.value = [];
  replayTotal.value = 0;
  replayPage.value = 1;
  replayErr.value = "";
  await loadReplayPage(1);
}

/**
 * 拉第 `page` 页回放（**一次只显示一页**，不累加）。
 *
 * 为什么不累加：斗鱼 `authorShowVideoList` 每页锁死 20 条
 * （limit 传 100 也只回 20），而这个主播有 2237 场 —— 累加滚到最早
 * 要翻 112 页。改成翻页 + 日期跳转，想去哪直接跳，不用滚。
 */
async function loadReplayPage(page: number) {
  if (replayLoading.value) return;
  replayLoading.value = true;
  replayErr.value = "";
  try {
    const p = Math.max(1, page);
    const r = await getReplays(props.platform, props.id, p);
    replayTotal.value = r.total;
    replayPage.value = p;
    replays.value = r.list;
    if (!r.list.length) replayErr.value = "这个主播还没有开放回放";
  } catch (e) {
    replayErr.value = `${e}`;
  } finally {
    replayLoading.value = false;
  }
}

function goReplayPage(n: number) {
  const t = Math.min(Math.max(1, n), replayPages.value);
  if (t === replayPage.value || replayLoading.value) return;
  void loadReplayPage(t);
}

/**
 * 跳到某个日期所在的页。
 *
 * 列表是按时间倒序的（新 → 旧），所以可以**二分**：拿中间页的首尾日期
 * 跟目标比，落在页内就停，否则往新/旧的方向收窄。
 * 2237 场 = 112 页，二分最多 7 次请求（约几秒），比一页页翻快得多。
 */
async function jumpToDate(dateStr: string) {
  if (!dateStr || replayJumping.value) return;
  const total = replayPages.value;
  if (total <= 1) return;
  replayJumping.value = true;
  replayErr.value = "";
  let found = 1;
  try {
    let lo = 1;
    let hi = total;
    let hit = 1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const r = await getReplays(props.platform, props.id, mid);
      replayTotal.value = r.total;
      const list = r.list;
      if (!list.length) break;
      const first = dstr(list[0]?.time || "");
      const last = dstr(list[list.length - 1]?.time || "");
      if (dateStr >= last && dateStr <= first) {
        // 目标日期就在这一页的跨度里
        hit = mid;
        break;
      }
      if (dateStr > first) {
        // 目标比这一页还新 → 往页码小的方向找
        hi = mid - 1;
      } else {
        lo = mid + 1;
      }
      hit = mid;
    }
    found = hit;
  } catch (e) {
    replayErr.value = `${e}`;
  }
  // 先关掉「定位中」再取数据：那个提示是 v-if，开着的时候整个列表
  // 都不在 DOM 里，下面按 data-d 找节点会一个都找不到。
  replayJumping.value = false;
  await loadReplayPage(found);

  // 定位到的那一页里，把最接近目标日期的一条滚进视野并闪一下 ——
  // 不然用户还得自己在这一页里找
  await nextTick();
  for (const el of document.querySelectorAll<HTMLElement>(".rp-item")) {
    if ((el.dataset.d || "") <= dateStr) {
      el.scrollIntoView({ block: "center" });
      el.classList.add("hit");
      window.setTimeout(() => el.classList.remove("hit"), 1600);
      break;
    }
  }
}

/** 日期选择框变化 */
function onDatePick(e: Event) {
  const v = (e.target as HTMLInputElement).value;
  if (v) void jumpToDate(v);
}

/** 页码输入框回车 */
function onPageInput(e: Event) {
  const el = e.target as HTMLInputElement;
  const v = Number(el.value);
  if (isFinite(v) && v >= 1) goReplayPage(Math.floor(v));
  else el.value = String(replayPage.value);
}

async function playReplay(r: Replay) {
  replayOpen.value = false;
  curReplay.value = r;
  replayPlay.value = null;
  replayErr.value = "";
  replayDms.value = [];
  replayHls.value = [];
  replayParts.value = [];
  replayPartIdx.value = 0;
  try {
    // 先拿分段：看点要按段过滤，得知道第 1 段的起点和时长。
    // 分段很快（一次请求），不拖慢起播。
    const ps = await getReplayParts(
      props.platform,
      props.id,
      r.hash_id,
      r.start_time ?? 0,
    ).catch(() => [] as Replay[]);
    replayParts.value = ps;

    const p0 = ps[0];
    loadDm(p0?.hash_id ?? r.hash_id, p0?.start_time ?? r.start_time ?? 0);
    loadHl(
      p0?.hash_id ?? r.hash_id,
      p0?.start_time ?? r.start_time ?? 0,
      durSecs(p0?.duration),
    );

    // 第一次要开隐藏窗口让官方页面算签名（几秒），之后同一个视频走缓存
    replayPlay.value = await resolveReplay(r.hash_id);
  } catch (e) {
    replayErr.value = `${e}`;
  }
}

/** 切到第 n 段（0 基）：重新解析地址 + 换弹幕 */
async function playPart(n: number) {
  const parts = replayParts.value;
  if (n < 0 || n >= parts.length) return;
  replayPartIdx.value = n;
  const p = parts[n];
  replayPlay.value = null;
  replayErr.value = "";
  loadDm(p.hash_id, p.start_time ?? 0);
  loadHl(p.hash_id, p.start_time ?? 0, durSecs(p.duration));
  try {
    replayPlay.value = await resolveReplay(p.hash_id);
  } catch (e) {
    replayErr.value = `${e}`;
  }
}

/**
 * 左上角返回。
 *
 * 回放是「直播 → 回放列表 → 播放器」三层，返回要一层层退：
 * 在播放器里退回列表，在列表里退回直播。
 */
function onBack() {
  if (replayOpen.value) {
    exitReplay();
    return;
  }
  if (curReplay.value) {
    backToReplayList();
    return;
  }
  router.back();
}

/** 从回放回到回放列表。
 *
 * **不能清 curReplay** —— 清了的话列表里那个「返回直播」按钮会跟着消失
 * （它是 v-if="curReplay"），用户就再也回不到直播了。
 * 播放器留在下面继续放，选别的场次时再换掉。
 */
function backToReplayList() {
  replayOpen.value = true;
}

/**
 * 彻底退出回放、回到直播。
 *
 * 必须把 `curReplay` 一起清掉 —— 它同时是「当前在回放模式」的标记，
 * 留着的话下次点回放按钮会接着上一场播，而不是重新给列表。
 */
function exitReplay() {
  curReplay.value = null;
  replayPlay.value = null;
  replayDms.value = [];
  replayHls.value = [];
  replayParts.value = [];
  replayOpen.value = false;
}



/**
 * 斗鱼给两种时长格式，都转成好读的：
 *   "120:05"    → 分:秒（单段）      → "2 小时 0 分"
 *   "13:13:52"  → 时:分:秒（整场）   → "13 小时 13 分"
 */
function fmtDur(s: string) {
  const v = (s || "").trim();
  const hms = /^(\d+):(\d{2}):(\d{2})$/.exec(v);
  if (hms) {
    const h = Number(hms[1]);
    const m = Number(hms[2]);
    return h ? `${h} 小时 ${m} 分` : `${m} 分`;
  }
  const ms = /^(\d+):(\d{2})$/.exec(v);
  if (ms) {
    const mins = Number(ms[1]);
    const h = Math.floor(mins / 60);
    return h ? `${h} 小时 ${mins % 60} 分` : `${mins} 分`;
  }
  return v;
}
const danmuErr = ref("");
const listRef = ref<HTMLElement | null>(null);
const stageRef = ref<HTMLElement | null>(null);
const stage = ref({ w: 0, h: 0 });
const input = ref("");
/** 头像加载失败才显示首字母占位 */
const avaErr = ref(false);

let unlisten: (() => void) | null = null;
let ro: ResizeObserver | null = null;

const followed = computed(() =>
  detail.value ? store.isFollowed(props.platform, detail.value.room_id) : false,
);

/**
 * 播放器尺寸。
 *
 * 原来是「在舞台里塞一个 16:9 的盒子」，窗口不是 16:9 时四周就会留黑边。
 * 现在直接铺满整个舞台，比例交给播放器的 object-fit: contain 处理
 * （画面完整显示不裁切，比例不符时上下或左右留黑边）。
 */
const playerStyle = computed(() => {
  const { w, h } = stage.value;
  if (!w || !h) return {};
  return { width: `${Math.floor(w)}px`, height: `${Math.floor(h)}px` };
});

function scrollBottom() {
  const el = listRef.value;
  if (el) el.scrollTop = el.scrollHeight;
}

onMounted(async () => {
  ro = new ResizeObserver(() => {
    const el = stageRef.value;
    if (el) stage.value = { w: el.clientWidth, h: el.clientHeight };
  });
  if (stageRef.value) ro.observe(stageRef.value);

  let rid = decodeURIComponent(props.id);
  avaErr.value = false;
  try {
    const d = await getRoom(props.platform, rid);
    detail.value = d;
    rid = d.room_id || rid;
    current.value = d.plays[0] ?? null;
    if (!d.live) error.value = "该主播当前未开播";
    else if (!d.plays.length && !isEmbed.value)
      error.value = "没有解析到可用的播放地址";
    // 登记续流上下文：地址到期后代理自己续，播放器无感（详见 api.ts 的注释）
    syncRenew();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }

  // 弹幕（YouTube 也走自建通道：live_chat 长轮询，能飞屏）
  // SOOP 官方播放器模式下也接 —— 弹幕层是浮在 iframe 外面的独立覆盖层
  // （`pointer-events: none`，不挡官方控件的点击），和「视频由谁渲染」无关，
  // 跟 YouTube 官方 embed + 自建弹幕是同一个做法。
  try {
    unlisten = await listen<DanmakuMsg>("danmaku", (e) => {
      msgs.value.push(e.payload);
      if (msgs.value.length > 400) msgs.value.splice(0, 150);
      nextTick(scrollBottom);
    });
    await startDanmaku(props.platform, rid);
  } catch (e) {
    danmuErr.value = String(e);
  }
});

onBeforeUnmount(() => {
  unlisten?.();
  stopDanmaku().catch(() => {});
  // 离开播放页要清掉续流上下文，否则代理会一直为旧房间续流
  setStreamRenew("", "", "").catch(() => {});
  ro?.disconnect();
});

function pick(p: PlayUrl) {
  current.value = p;
  // 换画质/线路要同步更新续流上下文，否则地址到期后代理会按旧画质续，
  // 把用户的画质选择悄悄改回去。
  if (detail.value) {
    setStreamRenew(props.platform, detail.value.room_id, p.quality).catch(() => {});
  }
}

/**
 * 播放地址续期：代理层需要知道「拿什么参数去重新解析」。
 * 地址到期（斗鱼 300 秒）时代理自己会用这个上下文取新地址，播放器不用重载。
 */
function syncRenew() {
  const d = detail.value;
  const q = current.value?.quality ?? "";
  if (d && !isEmbed.value) {
    setStreamRenew(props.platform, d.room_id, q).catch(() => {});
  } else {
    setStreamRenew("", "", "").catch(() => {});
  }
}

/**
 * 播放器报「地址失效」时进来：重新解析房间，把新地址换上去。
 * 代理层的自动续流能覆盖大部分情况（尤其斗鱼 FLV），
 * 但 HLS（B站等）走的是另一条链路，得靠这里兜底。
 */
async function onRefresh() {
  try {
    const d = await getRoom(props.platform, detail.value?.room_id || decodeURIComponent(props.id));
    detail.value = d;
    if (!d.plays.length) return;
    const q = current.value?.quality;
    const next = d.plays.find((p) => p.quality === q) ?? d.plays[0];
    current.value = { ...next };
    syncRenew();
  } catch {
    /* 解析失败就维持原样，播放器那边会显示错误 */
  }
}

function toggleFollow() {
  if (detail.value) store.toggleFollow(detail.value);
}
</script>

<template>
  <div class="room">
    <!-- 顶部信息栏 -->
    <header class="bar">
      <button class="back" title="返回" @click="onBack">←</button>

      <div class="ava">
        <!-- 只渲染一个：图片加载失败才退回首字母。
             以前两个都渲染，span 有 z-index:0 会永远盖在图片上面。 -->
        <img
          v-if="detail?.avatar && !avaErr"
          :src="detail.avatar"
          referrerpolicy="no-referrer"
          @error="avaErr = true"
        />
        <span v-else>{{ (detail?.streamer || "?").slice(0, 1) }}</span>
      </div>

      <div class="meta">
        <div class="line1">
          <span class="ttl ellipsis" :title="detail?.title">
            {{ detail?.title || (loading ? "正在解析直播间…" : "") }}
          </span>
          <span v-if="detail?.live && detail?.replay" class="live-tag replay">录播中</span>
          <span v-else-if="detail?.live" class="live-tag">直播中</span>
          <span v-else-if="detail" class="off-tag">未开播</span>
        </div>
        <div class="line2">
          <span class="streamer">{{ detail?.streamer || "-" }}</span>
          <span class="id-pill">ID:{{ detail?.room_id || "-" }}</span>
          <span
            class="plat"
            :style="{ background: store.platformColor(props.platform) }"
          >{{ store.platformName(props.platform) }}</span>
          <span v-if="detail?.online" class="hot">👁 {{ detail.online }}</span>
          <button
            v-if="canReplay"
            class="replay-btn"
            :class="{ on: replayOpen || curReplay }"
            title="看这个主播的历史直播回放"
            @click="openReplays"
          >
            📼 回放
          </button>
        </div>
      </div>

      <button
        v-if="detail"
        class="follow round"
        :class="followed ? 'ghost' : 'primary'"
        @click="toggleFollow"
      >
        {{ followed ? "★ 已关注" : "☆ 关注" }}
      </button>
    </header>

    <div class="body">
      <!-- 视频区（不滚动，等比铺满） -->
      <div ref="stageRef" class="stage">
        <div class="stage-inner" :style="playerStyle">
          <!--
            只在**确实在直播**时渲染播放器。
            否则官方 iframe 播放器（YouTube）会拿路由里的视频 ID 去播那条回放 ——
            表现就是「不在直播却播了一段视频」。
          -->
          <Player
            v-if="detail?.live && !curReplay"
            :play="current"
            :plays="detail?.plays ?? []"
            :danmaku="msgs"
            :platform="props.platform"
            :room-id="detail?.room_id || props.id"
            @pick="pick"
            @refresh="onRefresh"
          />

          <!--
            回放播放：走我们自己的播放器。
            地址由后端开一个隐藏窗口、让官方页面自己算出带签名的 m3u8 拿到，
            再经本地代理喂给 hls.js —— 界面完全是我们自己的，不嵌官方页。
          -->
          <ReplayPlayer
            v-if="curReplay && replayPlay"
            :play="replayPlay"
            :danmaku="replayDms"
            :highlights="replayHls"
            :title="curReplay.title"
            :segments="replayParts.map((x) => x.time || x.title)"
            :part-index="replayPartIdx"
            @ended="onReplayEnded"
            @dm="onTimedDm"
            @back="backToReplayList"
            @part="playPart"
          >
            <template #top>
              <span class="rpp-seg">整场 {{ curReplay.duration }}</span>
            </template>
          </ReplayPlayer>
          <div v-else-if="curReplay" class="replay-wait">
            <div v-if="!replayErr" class="spinner"></div>
            <span>{{ replayErr || "正在解析回放地址…" }}</span>
          </div>


          <!-- 回放列表 -->
          <div v-if="replayOpen" class="rp-panel">
            <!-- 回放列表 = 独立一层。返回键一路退：播放器 → 列表 → 直播，
                 所以这里只有一个「返回直播」，不再放 ✕（两个出口容易点错）。 -->
            <div class="rp-head">
              <button class="rp-back" @click="exitReplay">← 返回直播</button>
              <span class="rp-ht">直播回放 · {{ detail?.streamer || "" }}</span>
              <span v-if="replayTotal" class="rp-cnt">共 {{ replayTotal }} 场</span>
            </div>

            <!-- 定位条：一个主播几千场，滚是滚不完的，
                 所以给「跳日期」和「跳页码」两个入口。 -->
            <div v-if="replayTotal" class="rp-bar">
              <label class="rp-date">
                <span>跳到日期</span>
                <input type="date" @change="onDatePick" />
              </label>
              <span v-if="replaySpan" class="rp-span">{{ replaySpan }}</span>
              <span class="rp-sp"></span>
              <button
                class="rp-nav"
                :disabled="replayPage <= 1 || replayLoading || replayJumping"
                @click="goReplayPage(replayPage - 1)"
              >
                上一页
              </button>
              <span class="rp-pg">
                第
                <input
                  type="number"
                  min="1"
                  :max="replayPages"
                  :value="replayPage"
                  @change="onPageInput"
                />
                / {{ replayPages }} 页
              </span>
              <button
                class="rp-nav"
                :disabled="
                  replayPage >= replayPages || replayLoading || replayJumping
                "
                @click="goReplayPage(replayPage + 1)"
              >
                下一页
              </button>
            </div>
            <div v-if="replayJumping" class="rp-tip">正在按日期定位…</div>
            <div v-else-if="replayLoading && !replays.length" class="rp-tip">
              正在拉取回放列表…
            </div>
            <div v-else-if="replayErr && !replays.length" class="rp-tip">
              {{ replayErr }}
            </div>
            <div v-else class="rp-list">
              <div
                v-for="r in replays"
                :key="r.hash_id"
                class="rp-item"
                :data-d="dstr(r.time)"
                @click="playReplay(r)"
              >
                <img
                  v-if="r.cover"
                  :src="r.cover"
                  loading="lazy"
                  referrerpolicy="no-referrer"
                />
                <div class="rp-txt">
                  <div class="rp-t ellipsis" :title="r.title">{{ r.title }}</div>
                  <div class="rp-s">
                    {{ r.time }} · {{ fmtDur(r.duration) }}
                    <template v-if="r.view_num"> · {{ r.view_num }} 次观看</template>
                  </div>
                </div>
              </div>
              <div v-if="replayPage >= replayPages" class="rp-end">
                已到最早一场
              </div>
            </div>
          </div>

          <div v-if="loading" class="veil">
            <div class="spinner"></div>
            <span>正在解析直播间…</span>
          </div>
          <!-- 回放模式下不要盖这行提示：error 是「主播未开播」，但我们在放回放 -->
          <div v-else-if="error && !curReplay" class="warn">{{ error }}</div>
        </div>
      </div>

      <!-- 弹幕 -->
      <aside class="danmu">
        <div class="d-head">
          <span>弹幕</span>
          <span v-if="!isYtChat" class="badge">{{ msgs.length }}</span>
          <span v-else class="badge">官方</span>
        </div>

        <!-- YouTube：官方 live_chat（自带发送框，未登录只能看） -->
        <iframe
          v-if="isYtChat"
          class="d-yt"
          :src="ytChatSrc"
          title="YouTube 聊天"
          frameborder="0"
          sandbox="allow-scripts allow-same-origin allow-popups allow-popups-to-escape-sandbox allow-forms"
        ></iframe>

        <template v-else>
          <div ref="listRef" class="d-list">
            <div v-if="danmuErr" class="d-err">{{ danmuErr }}</div>
            <div v-for="(m, i) in msgs" :key="i" class="dm">
              <span
                class="dm-user"
                :style="m.color && m.color !== '#ffffff' ? { color: m.color } : {}"
              >{{ m.user }}：</span>
              <span class="dm-text">{{ m.text }}</span>
            </div>
            <div v-if="!msgs.length && !danmuErr" class="d-empty">
              {{
                curReplay
                  ? "回放弹幕会跟着进度飘出来"
                  : detail?.live
                    ? "正在连接弹幕…"
                    : "主播未开播，没有弹幕"
              }}
            </div>
          </div>

          <div class="d-foot">
            <input v-model="input" disabled placeholder="发送弹幕需要登录对应平台" />
            <button disabled>发送</button>
          </div>
        </template>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.room {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--bg);
}

/* ---------------- 顶部信息栏 ---------------- */
.bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 20px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  flex: none;
}
.back {
  width: 32px;
  height: 32px;
  padding: 0;
  font-size: 16px;
  line-height: 1;
  border-radius: 50%;
  background: var(--chip);
  flex: none;
}
.ava {
  width: 44px;
  height: 44px;
  flex: none;
  border-radius: 8px;
  overflow: hidden;
  background: var(--chip);
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 600;
  color: var(--fg-mute);
  position: relative;
}
.ava img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
  /* 图片压在最上层，否则 span 的 z-index:0 会盖住它 */
  z-index: 1;
}
.ava span {
  position: relative;
  z-index: 0;
}
.meta {
  flex: 1;
  min-width: 0;
}
.line1 {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.ttl {
  font-size: 17px;
  font-weight: 600;
  color: var(--fg);
  min-width: 0;
}
.live-tag {
  flex: none;
  font-size: 11.5px;
  color: #fff;
  background: var(--green);
  border-radius: 4px;
  padding: 2px 7px;
}
/* 在放录播（斗鱼视频轮播）：房间在推流但内容是录像 */
.live-tag.replay {
  background: var(--warn);
}
.off-tag {
  flex: none;
  font-size: 11.5px;
  color: #fff;
  background: #b9bec7;
  border-radius: 4px;
  padding: 2px 7px;
}
.line2 {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 5px;
  font-size: 12px;
  color: var(--fg-dim);
}
.streamer {
  color: var(--fg-2);
}
.id-pill {
  background: var(--chip);
  color: var(--fg-2);
  border-radius: 999px;
  padding: 1px 10px;
}
.plat {
  color: #fff;
  border-radius: 4px;
  padding: 1px 7px;
}
.follow.round {
  border-radius: 999px;
  height: 34px;
  padding: 0 18px;
  flex: none;
}

/* ---------------- 直播回放（斗鱼） ---------------- */
.replay-btn {
  flex: none;
  height: 24px;
  padding: 0 10px;
  font-size: 12px;
  color: var(--fg-2);
  background: var(--chip);
  border: 1px solid var(--border-2);
  border-radius: 999px;
  cursor: pointer;
}
.replay-btn:hover {
  background: var(--chip-hover);
}
.replay-btn.on {
  color: var(--brand);
  border-color: var(--brand);
  background: var(--brand-soft);
}
/* 回放地址解析中（要开隐藏窗口让官方页算签名，第一次几秒） */
.replay-wait {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  font-size: 13px;
  color: var(--fg-dim);
  background: #000;
}
/* ReplayPlayer 顶栏里的小标签（分段 / 整场时长） */
.rpp-seg {
  flex: none;
  padding: 3px 9px;
  font-size: 11.5px;
  color: #fff;
  background: rgba(0, 0, 0, 0.5);
  border-radius: 999px;
  white-space: nowrap;
}
.rp-panel {
  position: absolute;
  inset: 0;
  z-index: 8;
  display: flex;
  flex-direction: column;
  background: var(--panel);
}
.rp-head {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px;
  font-size: 13.5px;
  font-weight: 600;
  border-bottom: 1px solid var(--border);
}
.rp-back {
  flex: none;
  height: 28px;
  padding: 0 12px;
  font-size: 12.5px;
  font-weight: 400;
  color: var(--fg-2);
  background: var(--chip);
  border: 1px solid var(--border-2);
  border-radius: 999px;
  cursor: pointer;
}
.rp-back:hover {
  background: var(--chip-hover);
}
.rp-ht {
  flex: 1;
  min-width: 0;
  padding: 0 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rp-cnt {
  flex: none;
  font-size: 11.5px;
  font-weight: 400;
  color: var(--fg-dim);
}
/* 定位条 */
.rp-bar {
  flex: none;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-3);
  font-size: 12px;
  color: var(--fg-2);
}
.rp-date {
  display: flex;
  align-items: center;
  gap: 6px;
}
.rp-date input {
  height: 26px;
  padding: 0 6px;
  font-size: 12px;
  color: var(--fg);
  background: var(--panel);
  border: 1px solid var(--border-2);
  border-radius: 6px;
  cursor: pointer;
}
.rp-span {
  color: var(--fg-dim);
  font-variant-numeric: tabular-nums;
}
.rp-sp {
  flex: 1;
  min-width: 0;
}
.rp-nav {
  height: 26px;
  padding: 0 10px;
  font-size: 12px;
  color: var(--fg-2);
  background: var(--panel);
  border: 1px solid var(--border-2);
  border-radius: 6px;
  cursor: pointer;
}
.rp-nav:hover:not(:disabled) {
  background: var(--chip-hover);
}
.rp-nav:disabled {
  opacity: 0.45;
  cursor: default;
}
.rp-pg {
  display: flex;
  align-items: center;
  gap: 4px;
  font-variant-numeric: tabular-nums;
}
.rp-pg input {
  width: 52px;
  height: 26px;
  padding: 0 6px;
  font-size: 12px;
  text-align: center;
  color: var(--fg);
  background: var(--panel);
  border: 1px solid var(--border-2);
  border-radius: 6px;
}
.rp-item.hit {
  background: var(--brand-soft);
  box-shadow: inset 3px 0 0 var(--brand);
}
.rp-end {
  padding: 14px 0 18px;
  text-align: center;
  font-size: 12px;
  color: var(--fg-mute);
}
.rp-tip {
  padding: 24px 14px;
  font-size: 13px;
  color: var(--fg-dim);
}
.rp-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}
.rp-item {
  display: flex;
  gap: 10px;
  padding: 8px;
  border-radius: 8px;
  cursor: pointer;
}
.rp-item:hover {
  background: var(--chip-hover);
}
.rp-item img {
  width: 104px;
  height: 58px;
  flex: none;
  object-fit: cover;
  border-radius: 6px;
  background: var(--chip);
}
.rp-txt {
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 5px;
}
.rp-t {
  font-size: 13px;
  color: var(--fg);
}
.rp-s {
  font-size: 11.5px;
  color: var(--fg-dim);
}

/* ---------------- 主体 ---------------- */
.body {
  flex: 1;
  min-height: 0;
  display: flex;
  overflow: hidden;
}
.stage {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 14px 14px 14px 18px;
  overflow: hidden;
}
.stage-inner {
  position: relative;
  border-radius: 10px;
  overflow: hidden;
  background: #000;
}
.stage-inner :deep(.player) {
  width: 100% !important;
  height: 100% !important;
  aspect-ratio: auto;
  border-radius: 0;
}
.veil {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  background: rgba(0, 0, 0, 0.6);
  color: #fff;
  font-size: 13px;
}
.spinner {
  width: 26px;
  height: 26px;
  border: 2px solid rgba(255, 255, 255, 0.25);
  border-top-color: #fff;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
.warn {
  position: absolute;
  left: 12px;
  right: 12px;
  bottom: 12px;
  padding: 8px 12px;
  background: rgba(0, 0, 0, 0.7);
  color: #ffe1a3;
  border-radius: 8px;
  font-size: 12.5px;
}

/* ---------------- 弹幕 ---------------- */
.danmu {
  width: 288px;
  flex: none;
  background: var(--panel);
  border-left: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.d-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 13px 14px 11px;
  font-size: 13px;
  font-weight: 600;
  color: var(--fg);
  border-bottom: 1px solid var(--border);
  flex: none;
}
.badge {
  font-size: 11px;
  font-weight: 400;
  color: var(--fg-dim);
  background: var(--chip);
  border-radius: 999px;
  padding: 1px 8px;
}
.d-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 10px 12px;
  scrollbar-gutter: stable;
}

/* YouTube 官方 live_chat iframe：铺满弹幕栏剩余高度 */
.d-yt {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: 0;
  background: var(--panel);
}
.dm {
  font-size: 12.5px;
  line-height: 1.65;
  padding: 2px 0;
  word-break: break-word;
  color: var(--fg);
  animation: fadein 0.22s ease;
}
@keyframes fadein {
  from {
    opacity: 0;
    transform: translateY(4px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.dm-user {
  color: var(--fg-dim);
}
.dm-text {
  color: var(--fg);
}
.d-empty,
.d-err {
  padding: 30px 8px;
  text-align: center;
  font-size: 12.5px;
  color: var(--fg-mute);
  line-height: 1.8;
}
.d-err {
  color: #e8a33d;
}
.d-foot {
  flex: none;
  display: flex;
  gap: 8px;
  padding: 10px 12px;
  border-top: 1px solid var(--border);
}
.d-foot input {
  flex: 1;
  min-width: 0;
  height: 32px;
  font-size: 12.5px;
}
.d-foot button {
  flex: none;
  height: 32px;
  padding: 0 14px;
  border-radius: 999px;
}
.d-foot input:disabled,
.d-foot button:disabled {
  opacity: 0.55;
}

/* ==================== 移动端（必须放在样式表最后） ====================
   放在中间会被后面同名规则按「后写的赢」压掉 —— 上一版就栽在这，
   媒体查询里写了 flex:none 却完全没生效。

   PC 版是「播放器 + 右侧 288px 弹幕栏」左右分栏；手机上并排会让两边都窄到
   没法看（实测 374px 窗口时弹幕栏还是死死的 288px、播放器只剩几十像素高），
   所以改成上下分层：上面播放器固定 16:9，下面弹幕列表吃满剩余高度。 */
@media (max-width: 820px) {
  .body {
    flex-direction: column;
  }
  .stage {
    flex: none;
    width: 100%;
    aspect-ratio: 16 / 9;
    padding: 0;
  }
  .danmu {
    width: 100%;
    flex: 1;
    min-height: 0;
    border-left: 0;
    border-top: 1px solid var(--border);
  }
  /* 顶部信息栏窄屏要能换行，否则标题会把按钮挤出屏幕 */
  .bar {
    flex-wrap: wrap;
    height: auto;
    padding: 8px 12px;
    gap: 8px;
  }
  /* 弹幕设置面板在窄屏上占满宽度更好点 */
  .danmu .d-head,
  .danmu .d-foot {
    font-size: 13px;
  }
}
</style>
