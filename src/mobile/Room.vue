<script setup lang="ts">
/**
 * 移动端播放页 —— 独立实现，不复用 PC 的 Room.vue。
 *
 * 竖屏布局严格对齐 Simple Live 的手机模型（调研结论 simple-live-mobile.md C1）：
 *   1. 播放器   固定 16:9 贴顶、铺满宽度（390 宽 ≈ 219 高），黑底不占满屏
 *   2. 主播条   一行：头像(28) + 主播名(截断) + 在线人数 + 关注按钮，≤48px
 *   3. 弹幕列表 占满剩余高度、可滚动
 *   4. 底部操作条 44px：弹幕开关 + 「说点什么…」+ 更多
 *
 * 本轮在上一版基础上的补齐（逐项差距分析见交付报告）：
 *   - 更多 sheet 里补齐「弹幕设置」（字号/速度/不透明度/显示区域/颜色/屏蔽词），
 *     数据直接复用 src/dmSettings.ts（与 PC 版同一份，不新建状态）；
 *   - 补齐「画面比例：适应/拉伸/铺满」（默认适应），只用 CSS 覆盖 Player 的
 *     object-fit，**不改 Player.vue 的 props/emits/逻辑**；
 *   - 补齐「刷新」入口（走 onRefresh，保留斗鱼续流 setStreamRenew）；
 *   - 画面浮层补齐「暂停/播放」「静音」两个 44px 按钮；
 *   - 弹幕列表补齐「最新」回底按钮（用户上滑翻看时出现，对齐 Simple Live）；
 *   - 补齐横屏左右布局（视频在左、主播条/弹幕/操作条在右栏）；
 *   - 播放失败遮罩补「重试」按钮；单击不再误触发 Player 的暂停。
 *
 * 手势只保留 Simple Live 的那几种（调研 C4）：
 *   单击显隐控件、双击全屏、左半屏竖向=亮度、右半屏竖向=音量（5% 步进）。
 * **不做横向进度手势**——直播没有可拖的时间轴。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import {
  avatarUrl,
  getRoom,
  setStreamRenew,
  androidImmersive,
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
import { twitchMode } from "../twitchMode";
import {
  DM_COLOR_MODES,
  dmArea,
  dmBlock,
  dmColorMode,
  dmCustom,
  dmOn,
  dmOpacity,
  dmSize,
  dmSpeed,
} from "../dmSettings";

const props = defineProps<{ platform: string; id: string }>();
const router = useRouter();

const detail = ref<RoomDetail | null>(null);
const current = ref<PlayUrl | null>(null);
const msgs = ref<DanmakuMsg[]>([]);
const loading = ref(true);
const error = ref("");
const showMore = ref(false);
const listRef = ref<HTMLElement | null>(null);
/** 弹幕开关统一用 dmSettings.dmOn（与飞屏弹幕同源），底部操作条上的「弹」按钮控制它 */
/** 播放器舞台：手势只挂在它上面，下方弹幕列表在另一层，滚动不受影响 */
const stageRef = ref<HTMLElement | null>(null);
/** 解析后的真实房间号（弹幕重试要用） */
const rid = ref("");

/** 弹幕列表字号（对齐 Simple Live 聊天区可调字号；与飞屏字号 dmSize 相互独立） */
const listSize = ref(Number(localStorage.getItem("junlive.dm_list_size") || 13));
watch(listSize, (v) => localStorage.setItem("junlive.dm_list_size", String(v)));

/**
 * 画面比例（对齐 Simple Live「画面尺寸」：0 适应 / 1 拉伸 / 2 铺满）。
 * 默认 contain（适应）——PC 版也明确要求默认适应。
 * 只通过 :deep 覆盖 <video> 的 object-fit，不碰 Player.vue。
 */
const fit = ref(localStorage.getItem("junlive.fit") || "contain");
watch(fit, (v) => localStorage.setItem("junlive.fit", v));
const fitLabel = computed(
  () => ({ contain: "适应", fill: "拉伸", cover: "铺满" })[fit.value] ?? "适应",
);

/* ---------------- 弹幕连接状态（三态，不再失败静默） ---------------- */
const dmState = ref<"connecting" | "ok" | "error">("connecting");
const dmErr = ref("");

/* ---------------- 全屏 ----------------
 * 手机要的是「整个屏幕都是画面」。全屏 = .mr fixed 铺满，舞台铺满，弹幕列表
 * 收起（弹幕由 Player 自带的飞屏层继续飘）。退出按钮必须常驻（手机没有 ESC）。
 *
 * 横屏锁：全屏 = 无条件锁横屏。screen.orientation.lock('landscape') 在 WebView
 * 里需要用户手势内调用；失败也不影响 CSS 全屏（失败时给「横过来看」提示兜底）。
 * 例外：若已在浏览器原生全屏里（document.fullscreenElement 存在），不重复锁。
 *
 * 系统栏：光加 CSS 类只覆盖 WebView 内部，改不了安卓的系统状态栏/导航栏；
 * 必须再调 Tauri 的窗口全屏 setFullscreen()，安卓才会进沉浸式隐藏状态栏。
 */
const full = ref(false);
/** 是否横屏。全屏锁横屏失败时（iOS / 系统锁旋转）给用户一个「转过来」的提示 */
const landscape = ref(false);
/** 控件是否可见（沉浸式：3.5 秒后自动隐藏，点一下再出来）——对齐 Simple Live 的自动隐藏 */
const ctrlOn = ref(true);
let ctrlTimer: number | null = null;

/** 播放 / 静音状态（用于浮层按钮图标）。Player 内部 video 的 play/pause 事件不往外抛，
 *  这里用一个轻量轮询读取真实状态，避免去改 Player.vue 的接口。 */
const playingNow = ref(false);
const mutedNow = ref(false);
let stateTimer: number | null = null;

function syncOrient() {
  landscape.value = window.innerWidth > window.innerHeight;
}

/** 重置「控件自动隐藏」倒计时（竖屏/全屏都适用，对齐 Simple Live 的自动隐藏） */
function pokeCtrl() {
  ctrlOn.value = true;
  if (ctrlTimer) window.clearTimeout(ctrlTimer);
  ctrlTimer = window.setTimeout(() => {
    ctrlOn.value = false;
  }, 3500);
}

/** 单击：显隐浮层控件（返回/全屏/播放/静音） */
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
  // Tauri 窗口全屏：让窗口铺满整个屏幕，去掉白边。
  //
  // ⚠️ 但这**不够** —— tao 在安卓上把 set_fullscreen 实现成了空函数
  // （tao-0.37.1/src/platform_impl/android/mod.rs:823 只有一句
  // "Cannot set fullscreen on Android"），Tauri 2 也没有隐藏系统栏的 API。
  // 所以要另外经 JNI 调 Android 的 WindowInsetsController（见 immersive.rs）。
  try {
    await getCurrentWindow().setFullscreen(full.value);
  } catch {
    /* 浏览器预览 / 无窗口 API 时忽略，CSS 全屏仍然生效 */
  }
  // 隐藏系统状态栏与导航栏。applied=false（如老系统、非安卓）时也不影响，
  // 上面已经改成 window 全屏，CSS 全屏同样生效。
  try {
    await androidImmersive(full.value);
  } catch {
    /* 命令不可用时忽略 */
  }
  syncOrient();
}

/* ------------------------------------------------------------------ 手势控制
 * 只做 Simple Live 的那几种：
 *   单击         → 显隐浮层控件
 *   双击         → 全屏
 *   左半屏上下滑 → 亮度（仅移动端，原理见 applyBright）
 *   右半屏上下滑 → 音量（5% 步进，对齐 Simple Live）
 * 竖向手势的起手点限制在屏高 25%~75%（Simple Live 做法，调研 C4）。
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
/** 本轮触摸是否落在某个浮层按钮上（落上则整轮交给按钮的 click，不走手势/点按逻辑） */
let gOnButton = false;
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
  return window.innerWidth <= 820 || /android|iphone|ipad|ipod/i.test(navigator.userAgent);
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
 */
function applyBright(v: number) {
  bright.value = Math.min(BRIGHT_MAX, Math.max(BRIGHT_MIN, v));
  const el = stageVideo();
  if (el) el.style.filter = `brightness(${bright.value})`;
}

/** 音量按 5% 步进取整（对齐 Simple Live 的 _convertVolume） */
function roundVolume(v: number): number {
  return Math.min(1, Math.max(0, Math.round(v / 0.05) * 0.05));
}

function onStageTouchStart(e: TouchEvent) {
  const t = e.touches[0];
  if (!t) return;
  // 落在浮层按钮上的触摸：整轮交给按钮自己的 click，不做手势、不做点按判定。
  gOnButton = !!(e.target as HTMLElement)?.closest?.("button");
  if (gOnButton) {
    gKind = "ignore";
    gMoved = false;
    return;
  }
  // 抑制浏览器在 touchend 后合成的 click —— 否则单击会顺带触发 Player 里
  // <video> 的 @click="toggle"，表现就是「点一下想显隐控件，结果画面暂停了」。
  if (e.cancelable) e.preventDefault();
  gStartX = t.clientX;
  gStartY = t.clientY;
  gKind = "none";
  gMoved = false;
  // 起手点必须落在屏幕竖向 25%~75% 内，否则这轮竖向手势忽略
  const r = gStartY / window.innerHeight;
  gInBand = r >= V_START_MIN && r <= V_START_MAX;
}

function onStageTouchMove(e: TouchEvent) {
  if (gOnButton) return;
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
    // 灵敏度基准用整屏高；往上滑是增大，所以取 -dy；半个屏高对应调满/调到底
    const h = window.innerHeight;
    const ratio = -dy / Math.max(1, h * 0.5);
    if (gKind === "volume") {
      const nv = roundVolume(gStartVal + ratio);
      const vv = stageVideo();
      if (vv) {
        vv.volume = nv;
        vv.muted = nv === 0;
      }
      mutedNow.value = vv?.muted ?? false;
      showTip(nv === 0 ? "🔇" : "🔊", `音量 ${Math.round(nv * 100)}%`);
    } else {
      applyBright(gStartVal + ratio * (BRIGHT_MAX - BRIGHT_MIN));
      const pct = Math.round(((bright.value - BRIGHT_MIN) / (BRIGHT_MAX - BRIGHT_MIN)) * 100);
      showTip("☀", `亮度 ${pct}%`);
    }
  }
}

function onStageTouchEnd() {
  if (gOnButton) {
    gOnButton = false;
    gKind = "none";
    return;
  }
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

/* ---------------- 播放控制（浮层按钮用，操作的是 Player 内部的 <video>） ---- */
function togglePlay() {
  const v = stageVideo();
  if (!v) return;
  if (v.paused) v.play?.().catch(() => {});
  else v.pause?.();
  playingNow.value = !v.paused;
  pokeCtrl();
}
function toggleMute() {
  const v = stageVideo();
  if (!v) return;
  v.muted = !v.muted;
  if (!v.muted && v.volume === 0) v.volume = 0.6;
  mutedNow.value = v.muted;
  pokeCtrl();
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

/** 用户是否手动上滑翻看（此时右下角出现「最新」回底按钮，对齐 Simple Live） */
const dmManual = ref(false);
function onDmScroll() {
  const el = listRef.value;
  if (!el) return;
  dmManual.value = !nearBottom(el);
}
function toBottom() {
  dmManual.value = false;
  scrollBottom();
}

/**
 * 收到一条弹幕。
 *
 * 关键修复（对齐 PC 版 views/Room.vue:103-107）：
 *  - **先 nextTick 再滚**：push 之后 DOM 还没渲染出新行，立刻读 scrollHeight
 *    拿到的是旧值，列表会永远停在倒数第二条。
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

/**
 * 缓冲状态。
 *
 * 直播里最让人以为「卡死」的就是这段：封面已经垫在下面了，但播放器
 * 还在转、画面没出来。没有明确反馈时用户只会盯着一张不动的图。
 * 靠 video 的 waiting/stalled 进、playing/canplay/seeking 出。
 */
const buffering = ref(false);
let bufTimer: number | null = null;

function onBufStart() {
  buffering.value = true;
}
function onBufEnd() {
  // 延后一点再关：seeking 与 playing 常常连着来，立刻关会闪一下。
  if (bufTimer) window.clearTimeout(bufTimer);
  bufTimer = window.setTimeout(() => (buffering.value = false), 350);
}

/** 房间解析失败时的「重试」：重新拉一次 getRoom（初始 error 态没有重试入口是缺口） */
async function retryRoom() {
  loading.value = true;
  error.value = "";
  try {
    const d = await getRoom(props.platform, rid.value || decodeURIComponent(props.id));
    detail.value = d;
    rid.value = d.room_id || rid.value;
    current.value = d.plays[0] ?? null;
    if (!d.live) error.value = "该主播当前未开播";
    else if (!d.plays.length) error.value = "没有解析到可用的播放地址";
    if (d.room_id) setStreamRenew(props.platform, d.room_id, current.value?.quality ?? "").catch(() => {});
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
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

/* ---------------- Twitch 播放方式（官方播放器 / 自建流，用户自选） ---------------- */
const isTwitch = () => props.platform === "twitch";
const twitchOptions = [
  { v: "official", label: "官方播放器（更流畅，画质锁 640×360）" },
  { v: "native", label: "自建流（画质 1080p，token 约 1 小时）" },
] as const;

/** 画面比例可选项（对齐 Simple Live「画面尺寸」的前三项） */
const fitOptions = [
  { v: "contain", label: "适应" },
  { v: "fill", label: "拉伸" },
  { v: "cover", label: "铺满" },
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

  // 缓冲事件：靠 DOM 冒泡绑在舞台上，不改 Player.vue 的 props/emits
  // （那是全平台共用的接口，动它会牵连 PC 端）。
  const el = stageRef.value;
  if (el) {
    for (const ev of ["waiting", "stalled"]) el.addEventListener(ev, onBufStart);
    for (const ev of ["playing", "canplay", "seeking"]) el.addEventListener(ev, onBufEnd);
  }

  // 弹幕：监听只注册一次，连接失败可由「重试」按钮重新触发
  try {
    unlisten = await listen<DanmakuMsg>("danmaku", (e) => onDanmaku(e.payload));
  } catch {
    /* 事件通道注册失败不影响播放 */
  }
  await connectDanmaku();

  // 控件自动隐藏（对齐 Simple Live）
  pokeCtrl();
  // 轻量轮询 Player 内 <video> 的播放/静音状态，供浮层按钮显示图标
  stateTimer = window.setInterval(() => {
    const v = stageVideo();
    if (!v) return;
    playingNow.value = !v.paused;
    mutedNow.value = v.muted || v.volume === 0;
  }, 700);
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", syncOrient);
  window.removeEventListener("orientationchange", syncOrient);
  if (ctrlTimer) window.clearTimeout(ctrlTimer);
  if (stateTimer) window.clearInterval(stateTimer);
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
  // 系统栏同理：退出房间必须把状态栏/导航栏还回来，
  // 否则退回首页后整个界面少一条系统栏，看着像没铺满。
  androidImmersive(false).catch(() => {});
});
</script>

<template>
  <!--
    类名必须写成对象形式 `{ full: full }`。
    ⚠️ 之前写的是数组 `:class="[full, landscape, \`fit-${fit}\`]"` ——
    Vue 2/3 的**数组 class 里的 `true` 会被直接忽略**（不是渲染成 "true"，
    也不是渲染成 "full"），所以全屏时类名只有 `mr fit-contain`，
    CSS 里的 `.mr.full` 永远匹配不上，全屏样式（fixed 铺满、隐藏侧栏）全不生效。
    -->
  <div class="mr" :class="{ full: full, landscape: landscape, [`fit-${fit}`]: true }">
    <!-- 播放器：竖屏固定 16:9 贴顶；横屏时在左栏铺满高度。
         手势（单击/双击/亮度/音量）挂在这一层，只作用于画面区域。 -->
    <div
      ref="stageRef"
      class="mr-stage"
      @touchstart="onStageTouchStart"
      @touchmove="onStageTouchMove"
      @touchend="onStageTouchEnd"
      @touchcancel="onStageTouchEnd"
    >
      <!-- 封面海报：拉流还没出画面时（加载中 / 未开播 / 出错）先用缩略图垫底 -->
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
      <!--
        缓冲指示：直播最容易让人以为卡死的就是这一段 —— 封面已经垫在下面了，
        但播放器还在转、没出画面。用户看到的是「一张图卡着不动」，
        分不清是在加载还是死了。这里给一个明确的「缓冲中」+ 转圈。
        判定靠 video 元素的事件：waiting/stalled 进去，playing/seeking 出来。
      -->
      <div v-else-if="buffering" class="mr-veil buffering">
        <span class="mr-spin"></span>
        <span>缓冲中…</span>
      </div>
      <!-- 房间解析失败 / 未开播：给可点的「重试」，不再只有一行红字 -->
      <div v-else-if="error" class="mr-veil err">
        <div class="mr-veil-t">{{ error }}</div>
        <button class="mr-retry" @click.stop="retryRoom">重试</button>
      </div>

      <!-- 画面浮层控件，热区一律 44×44。单击画面可显隐、3.5 秒后自动隐藏。
           竖屏：返回(左) + 全屏(右)；底部再给 播放/暂停(左) + 静音(右)。
           全屏：退出按钮（手机没有 ESC 键）。 -->
      <template v-if="!full">
        <button
          v-show="ctrlOn"
          class="mr-ov mr-ov-l"
          aria-label="返回"
          @click.stop="router.back()"
        >‹</button>
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
      <button
        v-else
        class="mr-exit"
        :class="{ gone: !ctrlOn }"
        @click.stop="toggleFull"
      >✕ 退出全屏</button>

      <!-- 播放/暂停：隐藏了 Player 那套 18~31px 的桌面控制栏，播放控制由这两个 44px 浮层按钮接管 -->
      <button
        v-show="ctrlOn"
        class="mr-ov mr-ov-bl"
        :aria-label="playingNow ? '暂停' : '播放'"
        @click.stop="togglePlay"
      >{{ playingNow ? "❚❚" : "▶" }}</button>
      <button
        v-show="ctrlOn"
        class="mr-ov mr-ov-br"
        :aria-label="mutedNow ? '取消静音' : '静音'"
        @click.stop="toggleMute"
      >{{ mutedNow ? "🔇" : "🔊" }}</button>
    </div>

    <!-- 侧栏：竖屏在画面下方（上下排列）；横屏在右侧一栏（左右排列）。
         内容 = 主播条 + 弹幕列表 + 底部操作条。 -->
    <div v-if="!full" class="mr-side">
      <!-- 主播条：一行，≤48px。头像 + 主播名 + 在线人数 + 关注。 -->
      <div class="mr-bar">
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

      <!-- 弹幕列表：占满剩余高度，可滚动 -->
      <div ref="listRef" class="mr-dm" @scroll.passive="onDmScroll">
        <template v-if="!dmOn">
          <div class="mr-dm-empty">弹幕已关闭</div>
        </template>
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
          <div
            v-for="(m, i) in msgs"
            :key="`${m.ts}-${i}`"
            class="mr-dm-row"
            :style="{ fontSize: listSize + 'px', lineHeight: listSize + 9 + 'px' }"
          >
            <span class="mr-dm-u" :style="m.color && m.color !== '#ffffff' ? { color: m.color } : {}">{{ m.user }}</span>
            <span class="mr-dm-t">{{ m.text }}</span>
          </div>
        </template>
        <!-- 「最新」回底按钮：用户上滑翻看时出现（对齐 Simple Live） -->
        <button v-if="dmManual && msgs.length" class="mr-latest" @click="toBottom">最新 ↓</button>
      </div>

      <!-- 底部操作条：44px。左「弹幕开关」+ 中「说点什么…」+ 右「更多」 -->
      <div class="mr-acts">
        <button class="mr-act mr-act-dm" :class="{ off: !dmOn }" aria-label="弹幕开关" @click="dmOn = !dmOn">弹</button>
        <button class="mr-act-input" @click="promptDm">说点什么…</button>
        <button class="mr-act mr-act-more" aria-label="更多" @click="showMore = true">更多</button>
      </div>
    </div>

    <!-- 手势浮层：居中，fixed 定位不参与布局、pointer-events:none 不拦触摸 -->
    <div v-if="gTip" class="mr-gesture">
      <span class="mr-gesture-i">{{ gTip.icon }}</span>
      <span class="mr-gesture-t">{{ gTip.text }}</span>
    </div>

    <!-- 竖屏提示：全屏锁横屏失败时（iOS / 系统锁了旋转）提示用户手动转过来 -->
    <div v-if="full && !landscape" class="mr-rotate">
      <span class="mr-rotate-i">📱↻</span>
      <span>横过来看，画面更足</span>
    </div>

    <!-- 更多 sheet：画质/线路 + 画面比例 + 弹幕设置 + 刷新 +（SOOP）播放方式 -->
    <div v-if="showMore" class="mr-sheet" @click.self="showMore = false">
      <div class="mr-sheet-in">
        <div class="mr-sheet-head">画质 / 线路</div>
        <button
          v-for="p in detail?.plays ?? []"
          :key="p.url"
          class="mr-sheet-item"
          :class="{ on: p.url === current?.url }"
          @click="pick(p)"
        >
          {{ p.quality }}<span class="fmt">{{ (p.format || "").toUpperCase() }}</span
          ><span v-if="p.url === current?.url" class="tick">✓</span>
        </button>
        <div v-if="!detail?.plays?.length" class="mr-sheet-empty">没有可选的画质</div>

        <div class="mr-sheet-head">画面比例（当前：{{ fitLabel }}）</div>
        <div class="mr-sheet-seg">
          <button
            v-for="o in fitOptions"
            :key="o.v"
            class="mr-seg-item"
            :class="{ on: fit === o.v }"
            @click="fit = o.v"
          >{{ o.label }}</button>
        </div>

        <div class="mr-sheet-head">弹幕设置</div>
        <div class="mr-set-row">
          <span class="mr-set-l">显示弹幕</span>
          <input v-model="dmOn" type="checkbox" class="mr-switch" />
        </div>
        <label class="mr-set-row">
          <span class="mr-set-l">字号 {{ dmSize }}</span>
          <input v-model.number="dmSize" type="range" min="12" max="34" step="1" class="mr-range" />
        </label>
        <label class="mr-set-row">
          <span class="mr-set-l">速度 {{ dmSpeed }}s</span>
          <input v-model.number="dmSpeed" type="range" min="4" max="18" step="1" class="mr-range" />
        </label>
        <label class="mr-set-row">
          <span class="mr-set-l">不透明度 {{ Math.round(dmOpacity * 100) }}%</span>
          <input v-model.number="dmOpacity" type="range" min="0.2" max="1" step="0.05" class="mr-range" />
        </label>
        <div class="mr-set-row">
          <span class="mr-set-l">显示区域</span>
          <select v-model.number="dmArea" class="mr-sel">
            <option :value="0.25">1/4</option>
            <option :value="0.5">1/2</option>
            <option :value="0.75">3/4</option>
            <option :value="1">全屏</option>
          </select>
        </div>
        <div class="mr-set-row">
          <span class="mr-set-l">弹幕颜色</span>
          <span class="mr-colwrap">
            <select v-model="dmColorMode" class="mr-sel">
              <option v-for="m in DM_COLOR_MODES" :key="m.id" :value="m.id">{{ m.name }}</option>
            </select>
            <input v-if="dmColorMode === 'custom'" v-model="dmCustom" type="color" class="mr-color" />
          </span>
        </div>
        <label class="mr-set-row">
          <span class="mr-set-l">列表字号 {{ listSize }}</span>
          <input v-model.number="listSize" type="range" min="12" max="20" step="1" class="mr-range" />
        </label>
        <label class="mr-set-row">
          <span class="mr-set-l">屏蔽词</span>
          <input v-model="dmBlock" class="mr-text" placeholder="逗号分隔，命中不显示" />
        </label>

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

        <!--
          Twitch 播放方式：官方播放器更流畅但画质锁 640x360，
          自建流能到 1080p 但 token 有时效。放在播放页的「更多」里，
          不用跑到设置页 —— 换台看 Twitch 时最容易想的就是「怎么这么糊」。
        -->
        <template v-if="isTwitch()">
          <div class="mr-sheet-head">Twitch 播放方式</div>
          <button
            v-for="o in twitchOptions"
            :key="o.v"
            class="mr-sheet-item"
            :class="{ on: twitchMode === o.v }"
            @click="twitchMode = o.v"
          >
            {{ o.label }}<span v-if="twitchMode === o.v" class="tick">✓</span>
          </button>
        </template>

        <div class="mr-sheet-head">其他</div>
        <button class="mr-sheet-item" @click="onRefresh(); showMore = false">
          刷新直播<span class="tick">↻</span>
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

/* 16:9 贴顶、铺满宽度。max-height 兜底：极扁视口下不让舞台按宽度撑破屏幕。 */
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

/* ---------- 画面比例（对齐 Simple Live「画面尺寸」0/1/2）----------
   只覆盖 Player 内 <video> 的 object-fit，不动 Player.vue 的代码。
   .vid 默认 contain（适应）就是 Player.vue 里的样式，另两种在这里加。 */
.mr.fit-fill :deep(.vid) {
  object-fit: fill;
}
.mr.fit-cover :deep(.vid) {
  object-fit: cover;
}

/* ---------- 隐藏 Player 自带的桌面控制栏 ----------
   Player.vue 的 .ctrl 是为鼠标设计的（暂停/音量/线路/画质，实测按钮仅 18~31px），
   219px 高的手机舞台放不下也点不准；播放控制改由 Room 自己的 44px 浮层按钮接管。
   注意只隐藏 .ctrl；embed 平台的 .embed-tools 保留（官方播放器自有控件）。 */
.mr :deep(.ctrl) {
  display: none !important;
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
  flex-direction: column;
  gap: 12px;
  align-items: center;
  justify-content: center;
  color: #fff;
  font-size: 13px;
  background: rgba(0, 0, 0, 0.5);
  z-index: 15;
}
.mr-veil-t {
  padding: 0 20px;
  text-align: center;
}
/* 缓冲：转圈 + 文案，让「还在加载」和「卡死了」在视觉上分得开 */
.mr-spin {
  width: 22px;
  height: 22px;
  border: 2px solid rgba(255, 255, 255, 0.25);
  border-top-color: #fff;
  border-radius: 50%;
  animation: mr-spin 0.8s linear infinite;
}
@keyframes mr-spin {
  to { transform: rotate(360deg); }
}
.mr-veil.buffering {
  flex-direction: column;
  gap: 8px;
}
.mr-veil.err {
  color: #ffb4b4;
}

/* 画面上的浮层按钮（返回/全屏/播放/静音）：热区 44×44 */
.mr-ov {
  position: absolute;
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 22px;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 22px;
  line-height: 1;
  z-index: 20;
}
.mr-ov:active {
  background: rgba(0, 0, 0, 0.7);
}
.mr-ov-l {
  top: calc(10px + env(safe-area-inset-top, 0));
  left: 10px;
  font-size: 26px;
}
.mr-ov-r {
  top: calc(10px + env(safe-area-inset-top, 0));
  right: 10px;
}
.mr-ov-bl {
  left: 10px;
  bottom: 10px;
}
.mr-ov-br {
  right: 10px;
  bottom: 10px;
}

/* 侧栏：竖屏时在画面下方上下排列 */
.mr-side {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
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

/* ---------- 全屏（铺满） ---------- */
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
/* 横屏全屏：把播放器容器强制铺满 viewport（100vw × 100vh）。 */
.mr.full.landscape .mr-stage :deep(.player) {
  width: 100vw;
  height: 100vh;
  border-radius: 0;
}

/* ---------- 横屏（非全屏）：视频在左、侧栏在右（对齐 Simple Live 平板/横屏布局）----
   横屏时把 .mr 从「上下」改成「左右」：舞台铺满左栏高度，右侧固定 300px 放
   主播条 + 弹幕 + 操作条。竖屏时这套规则不生效。 */
@media (orientation: landscape) {
  .mr:not(.full) {
    flex-direction: row;
  }
  .mr:not(.full) .mr-stage {
    flex: 1;
    width: auto;
    height: 100%;
    max-height: none;
    aspect-ratio: auto;
  }
  .mr:not(.full) .mr-side {
    width: 300px;
    flex: none;
    height: 100%;
    border-left: 1px solid var(--border);
  }
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

/* 弹幕列表：占满剩余高度（字号/行高由内联 style 按设置决定） */
.mr-dm {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  padding: 6px 12px 12px;
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
/* 「最新」回底按钮：贴在弹幕区右下角，热区 ≥44 */
.mr-latest {
  position: sticky;
  bottom: 8px;
  left: 100%;
  transform: translateX(-8px);
  min-width: 44px;
  height: 44px;
  padding: 0 14px;
  border: 1px solid var(--border-2);
  border-radius: 22px;
  background: var(--panel);
  color: var(--brand);
  font-size: 13px;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.25);
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
  max-height: 80vh;
  overflow-y: auto;
}
.mr-sheet-head {
  padding: 12px 18px 6px;
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
  gap: 8px;
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
.mr-sheet-item .fmt {
  font-size: 11px;
  color: var(--fg-dim);
}
.mr-sheet-item .tick {
  margin-left: auto;
}

/* 画面比例分段控件 */
.mr-sheet-seg {
  display: flex;
  gap: 8px;
  padding: 0 18px 6px;
}
.mr-seg-item {
  flex: 1;
  height: 44px;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: var(--chip);
  color: var(--fg-2);
  font-size: 14px;
}
.mr-seg-item.on {
  border-color: var(--brand);
  color: var(--brand);
  font-weight: 600;
}

/* 弹幕设置行：每行 ≥44 高，保证滑块/开关也有足够触控高度 */
.mr-set-row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 48px;
  padding: 0 18px;
  font-size: 14px;
  color: var(--fg-2);
}
.mr-set-l {
  flex-shrink: 0;
  min-width: 96px;
}
.mr-range {
  flex: 1;
  height: 44px; /* 让 range 的命中区达到 44 高 */
  accent-color: var(--brand);
}
.mr-switch {
  width: 44px;
  height: 28px;
  accent-color: var(--brand);
}
.mr-sel,
.mr-text {
  flex: 1;
  min-width: 0;
  height: 44px;
  padding: 0 10px;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: var(--bg);
  color: var(--fg);
  font-size: 15px;
}
.mr-text {
  font-size: 16px; /* ≥16 防止 iOS 聚焦缩放 */
}
.mr-colwrap {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
}
.mr-color {
  width: 44px;
  height: 44px;
  padding: 0;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: transparent;
}
</style>
