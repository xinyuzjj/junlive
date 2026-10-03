<script setup lang="ts">
/**
 * 移动端播放页 —— 独立实现，不复用 PC 的 Room.vue。
 *
 * 竖屏布局严格对齐 Simple Live 的手机模型（调研结论 simple-live-mobile.md C1）：
 *   1. 播放器   固定 16:9 贴顶、铺满宽度（390 宽 ≈ 219 高），黑底不占满屏
 *   2. 主播条   一行：头像(28) + 主播名(截断) + 在线人数 + 关注按钮，≤48px
 *   3. 弹幕列表 占满剩余高度、可滚动（13px / 行高 22px）
 *   4. 底部操作条 44px：弹幕开关 + 「说点什么…」+ 更多
 *
 * 为什么不再把「画质 / 全屏 / 返回」全塞进主播条：390px 宽下那一行原本挤了
 * 7 个元素（返回/主播名/在线/分区/画质/全屏/收藏），实测全屏按钮只有 38px、
 * 返回 34px，都点不准。Simple Live 的做法是——画质/线路收进底部 sheet，
 * 返回/全屏做成画面上的浮层按钮，主播条只留人。
 *
 * 手势只保留 Simple Live 的那几种（调研 C4）：单击显隐控件、双击全屏、
 * 左侧竖向=亮度、右侧竖向=音量。**不做横向进度手势**——直播没有可拖的时间轴，
 * 横滑只会和系统「侧滑返回」抢事件、误触。
 */
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  avatarUrl,
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
import { getCurrentWindow } from "@tauri-apps/api/window";
import Player from "../components/Player.vue";
import { store } from "../store";
import { soopMode } from "../soopMode";

const props = defineProps<{ platform: string; id: string }>();
const router = useRouter();

const detail = ref<RoomDetail | null>(null);
const current = ref<PlayUrl | null>(null);
const msgs = ref<DanmakuMsg[]>([]);
const loading = ref(true);
const error = ref("");
const showMore = ref(false);
const listRef = ref<HTMLElement | null>(null);
/** 弹幕开关：关掉后列表区只留一行提示（Simple Live 底部操作条上的「弹幕开关」） */
const dmOn = ref(true);
/** 播放器舞台：手势只挂在它上面，下方弹幕列表在另一层，滚动不受影响 */
const stageRef = ref<HTMLElement | null>(null);
/** 解析后的真实房间号（弹幕重试要用） */
const rid = ref("");

/* ---------------- 弹幕连接状态（三态，不再失败静默） ----------------
 * 原来 catch{} 空实现，连不上会永远显示「正在连接弹幕…」，用户分不清
 * 「没弹幕」还是「连不上」。现在分：connecting / ok / error，error 时
 * 显示原因 + 重试按钮（对齐 PC 版 views/Room.vue 的 danmuErr 做法）。 */
const dmState = ref<"connecting" | "ok" | "error">("connecting");
const dmErr = ref("");

/* ---------------- 全屏 ----------------
 * 手机要的是「整个屏幕都是画面」。全屏 = .mr fixed 铺满，舞台铺满，弹幕列表
 * 收起（弹幕由 Player 自带的飞屏层继续飘）。退出按钮必须常驻（手机没有 ESC）。
 *
 * 横屏锁：全屏 = 无条件锁横屏（用户要求：点全屏默认直接横屏，不做「看视频
 * 宽高比再决定」的条件逻辑）。screen.orientation.lock('landscape') 在 WebView
 * 里需要用户手势内调用；失败也不影响 CSS 全屏（失败时给「横过来看」提示兜底）。
 * 例外：若已在浏览器原生全屏里（document.fullscreenElement 存在），不重复锁。
 *
 * 系统栏：光加 CSS 类只覆盖 WebView 内部，改不了安卓的系统状态栏/导航栏；
 * 必须再调 Tauri 的窗口全屏 setFullscreen()，安卓才会进沉浸式隐藏状态栏。 */
const full = ref(false);
/** 是否横屏。全屏锁横屏失败时（iOS / 系统锁旋转）给用户一个「转过来」的提示 */
const landscape = ref(false);
/** 全屏时控件是否可见（沉浸式：3 秒后自动隐藏，点一下再出来） */
const ctrlOn = ref(true);
let ctrlTimer: number | null = null;

function syncOrient() {
  landscape.value = window.innerWidth > window.innerHeight;
}

/** 全屏时重置「控件自动隐藏」倒计时；竖屏下不自动隐藏（没有常显的播放器控制栏可藏） */
function pokeCtrl() {
  ctrlOn.value = true;
  if (ctrlTimer) window.clearTimeout(ctrlTimer);
  ctrlTimer = window.setTimeout(() => {
    if (full.value) ctrlOn.value = false;
  }, 3000);
}

/** 单击：显隐我们自己的浮层控件（返回/全屏 或 退出） */
function toggleCtrl() {
  ctrlOn.value = !ctrlOn.value;
  if (ctrlOn.value) pokeCtrl();
  else if (ctrlTimer) {
    window.clearTimeout(ctrlTimer);
    ctrlTimer = null;
  }
}

async function toggleFull() {
  full.value = !full.value;
  if (full.value) {
    ctrlOn.value = true;
    pokeCtrl();
    // 点全屏默认直接横屏：无条件锁横屏（除非已在浏览器原生全屏里，避免重复锁）
    if (!document.fullscreenElement) {
      try {
        const so = screen.orientation as unknown as { lock?: (o: string) => Promise<void> };
        await so?.lock?.("landscape");
      } catch {
        /* 锁不住就靠下面那行提示让用户自己转 */
      }
    }
  } else {
    if (ctrlTimer) {
      window.clearTimeout(ctrlTimer);
      ctrlTimer = null;
    }
    ctrlOn.value = true;
    try {
      (screen.orientation as unknown as { unlock?: () => void }).unlock?.();
    } catch {
      /* 忽略 */
    }
  }
  // Tauri 窗口全屏：安卓上只有窗口真全屏才会隐藏系统状态栏/导航栏并去掉白边。
  // 光加 CSS 类改不了系统栏。浏览器预览时 getCurrentWindow() 会抛错 —— 包 try/catch。
  try {
    await getCurrentWindow().setFullscreen(full.value);
  } catch {
    /* 浏览器预览 / 无窗口 API 时忽略，CSS 全屏仍然生效 */
  }
  syncOrient();
}

/* ------------------------------------------------------------------ 手势控制
 * 只做 Simple Live 的那几种：
 *   单击         → 显隐浮层控件
 *   双击         → 全屏
 *   左半屏上下滑 → 亮度（仅移动端，原理见 applyBright）
 *   右半屏上下滑 → 音量
 * 竖向手势的起手点限制在屏高 25%~75%（Simple Live 做法，调研 C4）——避开顶部
 * 状态栏/通知下拉和底部导航/返回手势区，防止和系统手势打架。
 * 监听只挂在 .mr-stage（画面区域）上，下面弹幕列表是另一层，滚动不受影响。
 */
const gTip = ref<{ icon: string; text: string } | null>(null);
/** 模拟亮度值（不是系统亮度，见 applyBright） */
const bright = ref(1);
const BRIGHT_MIN = 0.4;
const BRIGHT_MAX = 1.3;
/** 小于这个总位移当点按处理 —— 手指轻微抖动不该触发手势 */
const GESTURE_MIN = 12;
/** 竖向手势起手点的屏高比例区间（避开系统手势区） */
const V_START_MIN = 0.25;
const V_START_MAX = 0.75;

let gStartX = 0;
let gStartY = 0;
/** 手势开始时的基准值：音量 / 亮度 */
let gStartVal = 0;
/** 手势过程中位移是否已超过阈值（区分点按与拖动） */
let gMoved = false;
/** 起手点是否落在允许的竖向区间内 */
let gInBand = false;
let gTipTimer: number | null = null;
type GKind = "none" | "volume" | "bright" | "ignore";
let gKind: GKind = "none";

/** 双击/单击判定用的计时器 */
let tapTimer: number | null = null;
let lastTapAt = 0;

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
  gMoved = false;
  // 起手点必须落在屏幕竖向 25%~75% 内，否则这轮竖向手势忽略
  const r = gStartY / window.innerHeight;
  gInBand = r >= V_START_MIN && r <= V_START_MAX;
}

function onStageTouchMove(e: TouchEvent) {
  const t = e.touches[0];
  if (!t) return;
  const dx = t.clientX - gStartX;
  const dy = t.clientY - gStartY;

  // 首次超过阈值时才判定手势类型，此后这一轮触摸锁定该类型
  if (gKind === "none") {
    if (Math.abs(dx) < GESTURE_MIN && Math.abs(dy) < GESTURE_MIN) return;
    gMoved = true;
    // 横向滑动已被删除（直播没有进度可拖）；这里直接忽略，不做任何提示
    if (Math.abs(dx) > Math.abs(dy)) {
      gKind = "ignore";
      return;
    }
    // 竖向：起手点不在允许区间 → 忽略
    if (!gInBand) {
      gKind = "ignore";
      return;
    }
    const v = stageVideo();
    if (!v) {
      gKind = "ignore";
      return;
    }
    if (gStartX < window.innerWidth / 2) {
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
    // 灵敏度基准用整屏高（原来用约 202px 的舞台高，拖 100px 就从 0 到满，极易误触）
    const h = window.innerHeight;
    // 往上滑是增大，所以取 -dy；滑半个屏高对应调满/调到底
    const ratio = -dy / Math.max(1, h * 0.5);
    if (gKind === "volume") {
      const nv = Math.min(1, Math.max(0, gStartVal + ratio));
      const vv = stageVideo();
      if (vv) {
        vv.volume = nv; // 音量写 video.volume
        vv.muted = nv === 0;
      }
      showTip(nv === 0 ? "🔇" : "🔊", `音量 ${Math.round(nv * 100)}%`);
    } else {
      applyBright(gStartVal + ratio * (BRIGHT_MAX - BRIGHT_MIN));
      const pct = Math.round(((bright.value - BRIGHT_MIN) / (BRIGHT_MAX - BRIGHT_MIN)) * 100);
      showTip("☀", `亮度 ${pct}%`);
    }
  }
}

function onStageTouchEnd() {
  gKind = "none";
  hideTipSoon();

  // 没有明显位移 → 当点按处理。单击显隐控件、双击全屏。
  // 单击要等 260ms 再执行，给双击留判定窗口。
  if (!gMoved) {
    const now = Date.now();
    if (now - lastTapAt < 260) {
      if (tapTimer) {
        window.clearTimeout(tapTimer);
        tapTimer = null;
      }
      lastTapAt = 0;
      toggleFull();
    } else {
      lastTapAt = now;
      if (tapTimer) window.clearTimeout(tapTimer);
      tapTimer = window.setTimeout(() => {
        tapTimer = null;
        lastTapAt = 0;
        toggleCtrl();
      }, 260);
    }
  }
  gMoved = false;
}

/* ------------------------------------------------------------------ 数据 */
let unlisten: (() => void) | null = null;

function scrollBottom() {
  const el = listRef.value;
  if (el) el.scrollTop = el.scrollHeight;
}

/** 用户是否停在底部附近（判断要不要自动跟随） */
function nearBottom(el: HTMLElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 40;
}

/**
 * 收到一条弹幕。
 *
 * 关键修复（对齐 PC 版 views/Room.vue:103-107）：
 *  - **先 nextTick 再滚**：push 之后 DOM 还没渲染出新行，立刻读 scrollHeight
 *    拿到的是旧值，列表会永远停在倒数第二条（原移动端的 bug）。
 *  - 裁剪（400 → 砍 150）时如果用户正在上滑翻看，就按「被砍掉的高度」回补
 *    scrollTop，保持视觉位置不跳；只有本来就在底部才继续跟随。
 */
function onDanmaku(m: DanmakuMsg) {
  const el = listRef.value;
  const following = el ? nearBottom(el) : true;
  const prevScrollH = el?.scrollHeight ?? 0;
  msgs.value.push(m);
  if (msgs.value.length > 400) {
    msgs.value.splice(0, 150);
    nextTick(() => {
      const el2 = listRef.value;
      if (!el2) return;
      if (following) el2.scrollTop = el2.scrollHeight;
      // 内容变矮了 (prevScrollH - 新 scrollHeight) 那么多，把 scrollTop 往回补，
      // 视觉上停在同一批弹幕上，不会突然上跳。
      else el2.scrollTop = Math.max(0, el2.scrollTop - (prevScrollH - el2.scrollHeight));
    });
    return;
  }
  nextTick(scrollBottom);
}

/** 连接弹幕。失败不再静默：置 error 态 + 原因 + 重试按钮 */
async function connectDanmaku() {
  dmState.value = "connecting";
  dmErr.value = "";
  try {
    await startDanmaku(props.platform, rid.value);
    dmState.value = "ok";
  } catch (e) {
    dmState.value = "error";
    dmErr.value = String(e);
  }
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
  // 换画质/线路必须同步更新续流上下文 —— 斗鱼地址 300 秒到期，代理靠这个
  // 上下文自动续流（实测 8 分钟不断）。丢了这行斗鱼 5 分钟必断。
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

/** 「说点什么…」：本轮不做真实发送，只提示 */
function promptDm() {
  gTip.value = { icon: "💬", text: "发弹幕功能开发中" };
  hideTipSoon();
}

/* ---------------- SOOP 播放方式（只 SOOP 需要，读 src/soopMode.ts） ---------------- */
const isSoop = () => props.platform === "soop";
const soopOptions = [
  { v: "native", label: "自建流（画质可选）" },
  { v: "embed", label: "官方播放器控件（画质不可调）" },
  { v: "official", label: "官方完整页（带官网界面）" },
] as const;

onMounted(async () => {
  syncOrient();
  window.addEventListener("resize", syncOrient);
  window.addEventListener("orientationchange", syncOrient);
  rid.value = decodeURIComponent(props.id);
  try {
    const d = await getRoom(props.platform, rid.value);
    detail.value = d;
    rid.value = d.room_id || rid.value;
    current.value = d.plays[0] ?? null;
    if (!d.live) error.value = "该主播当前未开播";
    else if (!d.plays.length) error.value = "没有解析到可用的播放地址";
    // 登记续流上下文：地址到期后代理自己续，播放器无感（详见 api.ts 注释）
    if (d.room_id) setStreamRenew(props.platform, d.room_id, current.value?.quality ?? "").catch(() => {});
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }

  // 弹幕：监听只注册一次，连接失败可由「重试」按钮重新触发
  try {
    unlisten = await listen<DanmakuMsg>("danmaku", (e) => onDanmaku(e.payload));
  } catch {
    /* 事件通道注册失败不影响播放 */
  }
  await connectDanmaku();
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", syncOrient);
  window.removeEventListener("orientationchange", syncOrient);
  if (ctrlTimer) window.clearTimeout(ctrlTimer);
  if (gTipTimer) window.clearTimeout(gTipTimer);
  if (tapTimer) window.clearTimeout(tapTimer);
  unlisten?.();
  stopDanmaku().catch(() => {});
  // 离开播放页要清掉续流上下文，否则代理会一直为旧房间续流
  setStreamRenew("", "", "").catch(() => {});
  // 离开房间必须恢复窗口非全屏，否则退回首页还卡在沉浸全屏（安卓系统栏也不回来）
  try {
    getCurrentWindow().setFullscreen(false).catch(() => {});
  } catch {
    /* 浏览器预览忽略 */
  }
});
</script>

<template>
  <div class="mr" :class="{ full, landscape }">
    <!-- 播放器：固定 16:9 贴顶、铺满宽度。
         手势（单击/双击/亮度/音量）挂在这一层，只作用于画面区域。 -->
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

      <!-- 竖屏浮层控件：返回（左）+ 全屏（右），热区 44×44。
           单击画面可显隐。Simple Live 把返回/全屏放在画面上，而不是塞进主播条。 -->
      <template v-if="!full">
        <button v-show="ctrlOn" class="mr-ov mr-ov-l" aria-label="返回" @click.stop="router.back()">‹</button>
        <button
          v-show="ctrlOn"
          class="mr-ov mr-ov-r"
          aria-label="全屏"
          title="全屏（横屏铺满）"
          @click.stop="toggleFull"
        >
          <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
            <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />
          </svg>
        </button>
      </template>

      <!-- 全屏退出入口：手机没有 ESC 键，必须有可见、点得到的按钮（热区 44）。
           沉浸式：3 秒后自动淡出，点屏幕任意位置再出来。 -->
      <button v-else class="mr-exit" :class="{ gone: !ctrlOn }" @click.stop="toggleFull">✕ 退出全屏</button>
    </div>

    <!-- 主播条：一行，≤48px。只放 头像 + 主播名 + 在线人数 + 关注。
         画质/全屏/返回都已经挪走（见上），390px 下不再挤爆。 -->
    <div v-if="!full" class="mr-bar">
      <img
        v-if="detail?.avatar"
        class="mr-ava"
        :src="avatarUrl(props.platform, detail.avatar)"
        referrerpolicy="no-referrer"
        alt=""
        @error="coverErr"
      />
      <div v-else class="mr-ava mr-ava-ph">{{ (detail?.streamer || "?").slice(0, 1) }}</div>
      <div class="mr-meta">
        <div class="mr-name">{{ detail?.streamer || "-" }}</div>
      </div>
      <span v-if="detail?.online" class="mr-online">👁 {{ detail.online }}</span>
      <button
        class="mr-fav"
        :class="{ on: detail && store.isFollowed(props.platform, detail.room_id) }"
        @click="toggleFollow"
      >
        <span class="mr-fav-i">{{ detail && store.isFollowed(props.platform, detail.room_id) ? "★" : "☆" }}</span>
        <span class="mr-fav-t">{{ detail && store.isFollowed(props.platform, detail.room_id) ? "已关注" : "关注" }}</span>
      </button>
    </div>

    <!-- 弹幕列表：占满剩余高度，可滚动（13px / 行高 22px） -->
    <div v-if="!full" ref="listRef" class="mr-dm">
      <div v-if="!dmOn" class="mr-dm-empty">弹幕已关闭</div>
      <template v-else>
        <div v-if="detail && !detail.live" class="mr-dm-empty">主播未开播，没有弹幕</div>
        <div v-else-if="dmState === 'error'" class="mr-dm-empty err">
          <div class="mr-dm-err-t">弹幕连接失败</div>
          <div class="mr-dm-reason">{{ dmErr }}</div>
          <button class="mr-retry" @click="connectDanmaku">重试</button>
        </div>
        <div v-else-if="!msgs.length" class="mr-dm-empty">
          {{ dmState === "connecting" ? "正在连接弹幕…" : "已连接，等待弹幕…" }}
        </div>
        <div v-for="(m, i) in msgs" :key="`${m.ts}-${i}`" class="mr-dm-row">
          <span class="mr-dm-u" :style="m.color && m.color !== '#ffffff' ? { color: m.color } : {}">{{ m.user }}</span>
          <span class="mr-dm-t">{{ m.text }}</span>
        </div>
      </template>
    </div>

    <!-- 底部操作条：44px。左「弹幕开关」+ 中「说点什么…」+ 右「更多」 -->
    <div v-if="!full" class="mr-acts">
      <button class="mr-act mr-act-dm" :class="{ off: !dmOn }" aria-label="弹幕开关" @click="dmOn = !dmOn">弹</button>
      <button class="mr-act-input" @click="promptDm">说点什么…</button>
      <button class="mr-act mr-act-more" aria-label="更多" @click="showMore = true">更多</button>
    </div>

    <!-- 手势浮层：居中，fixed 定位不参与布局、pointer-events:none 不拦触摸。
         z-index 8050 —— 高于全屏舞台(8000)，低于退出按钮(8100)，退出键始终可点 -->
    <div v-if="gTip" class="mr-gesture">
      <span class="mr-gesture-i">{{ gTip.icon }}</span>
      <span class="mr-gesture-t">{{ gTip.text }}</span>
    </div>

    <!-- 竖屏提示：全屏锁横屏失败时（iOS / 系统锁了旋转）提示用户手动转过来 -->
    <div v-if="full && !landscape" class="mr-rotate">
      <span class="mr-rotate-i">📱↻</span>
      <span>横过来看，画面更足</span>
    </div>

    <!-- 更多：底部 sheet。分组列出 画质 + 播放方式（仅 SOOP） -->
    <div v-if="showMore" class="mr-sheet" @click.self="showMore = false">
      <div class="mr-sheet-in">
        <div class="mr-sheet-head">画质</div>
        <button
          v-for="p in detail?.plays ?? []"
          :key="p.url"
          class="mr-sheet-item"
          :class="{ on: p.quality === current?.quality }"
          @click="pick(p)"
        >
          {{ p.quality }}<span v-if="p.quality === current?.quality" class="tick">✓</span>
        </button>
        <div v-if="!detail?.plays?.length" class="mr-sheet-empty">没有可选的画质</div>

        <template v-if="isSoop()">
          <div class="mr-sheet-head">播放方式</div>
          <button
            v-for="o in soopOptions"
            :key="o.v"
            class="mr-sheet-item"
            :class="{ on: soopMode === o.v }"
            @click="soopMode = o.v"
          >
            {{ o.label }}<span v-if="soopMode === o.v" class="tick">✓</span>
          </button>
        </template>
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

/* 16:9 贴顶、铺满宽度。max-height 兜底：极扁视口（横屏手机/窄窗口）下不让舞台
   按宽度撑破屏幕、把下面的弹幕挤成 0。 */
.mr-stage {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  max-height: 60vh;
  flex: none;
  background: #000;
  touch-action: none;
  user-select: none;
  -webkit-touch-callout: none;
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

/* 画面上的浮层按钮（返回 / 全屏）：热区 44×44，半透明黑底保证在任何画面上可读 */
.mr-ov {
  position: absolute;
  top: calc(6px + env(safe-area-inset-top, 0));
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 22px;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 26px;
  line-height: 1;
  z-index: 20;
}
.mr-ov:active {
  background: rgba(0, 0, 0, 0.7);
}
.mr-ov-l {
  left: 6px;
}
.mr-ov-r {
  right: 6px;
}

/* 主播条：一行 ≤48px */
.mr-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 48px;
  padding: 0 10px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.mr-ava {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  object-fit: cover;
  flex-shrink: 0;
  background: var(--chip);
}
.mr-ava-ph {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  color: var(--fg-dim);
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
.mr-online {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--fg-dim);
}
/* 关注按钮：热区至少 44 高 */
.mr-fav {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 3px;
  min-width: 44px;
  height: 44px;
  padding: 0 10px;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: var(--chip);
  color: var(--fg-2);
  font-size: 13px;
}
.mr-fav.on {
  color: var(--brand);
  border-color: var(--brand);
}
.mr-fav-i {
  font-size: 15px;
}
.mr-fav-t {
  font-size: 12px;
  white-space: nowrap;
}

/* ---------- 全屏（铺满） ----------
   整个 .mr 变成全屏定位，舞台铺满，常规的信息条/弹幕列表/操作条隐藏，
   弹幕交给 Player 组件自带的飞屏层（它本来就浮在视频上面）。
   横屏锁由 toggleFull 按视频流宽高比决定。 */
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
  max-height: none;
  aspect-ratio: auto;
}

/* 横屏全屏：把播放器容器强制铺满 viewport（100vw × 100vh）。
   Player 自带的飞屏弹幕层是相对播放器 inset:0 的，只有容器铺满它才跟着铺满整屏。
   为什么只在横屏铺满：手机横屏时 16:9 画面刚好吃掉整屏；纵屏若也铺满，16:9 会
   被拉扁、或因 object-fit: contain 留出很宽黑边，弹幕飘在黑边上很突兀。 */
.mr.full.landscape .mr-stage :deep(.player) {
  width: 100vw;
  height: 100vh;
  border-radius: 0;
}

/* 全屏退出按钮：热区 ≥44 */
.mr-exit {
  position: fixed;
  left: 12px;
  top: calc(12px + env(safe-area-inset-top, 0));
  z-index: 8100;
  min-width: 44px;
  height: 44px;
  padding: 0 16px;
  border: 0;
  border-radius: 22px;
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

/* 手势浮层：画面中央的小提示 */
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
  color: #fff;
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

/* 弹幕列表：占满剩余高度，13px / 行高 22px */
.mr-dm {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  padding: 6px 12px 12px;
  font-size: 13px;
  line-height: 22px;
}
.mr-dm-empty {
  padding: 20px;
  text-align: center;
  color: var(--fg-dim);
  font-size: 13px;
}
.mr-dm-err-t {
  color: #e06a6a;
  font-weight: 600;
  margin-bottom: 6px;
}
.mr-dm-reason {
  font-size: 12px;
  color: var(--fg-dim);
  word-break: break-all;
  margin-bottom: 12px;
}
/* 重试按钮：热区 ≥44 */
.mr-retry {
  min-width: 44px;
  height: 44px;
  padding: 0 20px;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: var(--chip);
  color: var(--fg);
  font-size: 14px;
}
.mr-dm-row {
  display: flex;
  gap: 6px;
  word-break: break-word;
}
.mr-dm-u {
  flex-shrink: 0;
  opacity: 0.9;
}
.mr-dm-t {
  color: var(--fg-2);
}

/* 底部操作条：44px（+ 底部安全区） */
.mr-acts {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 44px;
  box-sizing: content-box;
  padding: 0 8px env(safe-area-inset-bottom, 0);
  background: var(--panel);
  border-top: 1px solid var(--border);
  flex-shrink: 0;
}
.mr-act {
  flex-shrink: 0;
  min-width: 44px;
  height: 44px;
  padding: 0 10px;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: var(--chip);
  color: var(--fg-2);
  font-size: 13px;
}
.mr-act.off {
  color: var(--fg-dim);
  opacity: 0.6;
}
.mr-act-input {
  flex: 1;
  min-width: 0;
  height: 44px;
  padding: 0 14px;
  border: 1px solid var(--border-2);
  border-radius: 22px;
  background: var(--bg);
  color: var(--fg-dim);
  font-size: 13px;
  text-align: left;
}

/* 更多 sheet */
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
  max-height: 70vh;
  overflow-y: auto;
}
.mr-sheet-head {
  padding: 10px 18px;
  font-size: 13px;
  color: var(--fg-dim);
}
.mr-sheet-empty {
  padding: 4px 18px 12px;
  font-size: 13px;
  color: var(--fg-dim);
}
.mr-sheet-item {
  display: flex;
  align-items: center;
  width: 100%;
  min-height: 44px;
  height: 48px;
  padding: 0 18px;
  border: 0;
  background: none;
  font-size: 15px;
  color: var(--fg);
  text-align: left;
}
.mr-sheet-item.on {
  color: var(--brand);
  font-weight: 600;
}
.mr-sheet-item .tick {
  margin-left: auto;
}
</style>
