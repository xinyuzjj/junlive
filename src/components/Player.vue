<script setup lang="ts">
import Hls from "hls.js";
import mpegts from "mpegts.js";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { DanmakuMsg, PlayUrl } from "../api";
import {
  dmArea,
  dmOn,
  dmOpacity,
  dmPickColor,
  dmSize,
  dmSpeed,
  isBlocked,
} from "../dmSettings";
import { embedSrc as buildEmbedSrc, isEmbedPlatform } from "../embed";
import DmSet from "./DmSet.vue";

const props = defineProps<{
  play: PlayUrl | null;
  plays?: PlayUrl[];
  danmaku?: DanmakuMsg[];
  /**
   * 历史弹幕（回放用）：每条带 `time`（视频内秒数），按进度飘出来。
   *
   * 跟 `danmaku` 的区别：`danmaku` 是直播的「来一条加一条」，
   * 这个是「一整份带时间轴的」——播放到哪就飘到哪。
   */
  timedDanmaku?: { time: number; text: string; color: string; user: string }[];
  platform?: string;
  roomId?: string;
}>();
const emit = defineEmits<{
  (e: "pick", p: PlayUrl): void;
  /**
   * 播放地址失效，需要上层重新解析。
   *
   * **所有平台的播放地址都是有时效的**（斗鱼 300 秒、B站带 expires 参数、
   * SOOP 的 aid、Twitch 的 usher token……），过期后分片一律 403/断流。
   * 表现就是「看几分钟自己结束」，而原地重试（startLoad）用的是**旧地址**，
   * 怎么重试都没用。所以必须往上报，让上层拿新地址回来。
   */
  (e: "refresh"): void;
  /** 播完了（回放用来续下一段） */
  (e: "ended"): void;
  /** 飘出了一条历史弹幕（上层拿去填弹幕列表） */
  (e: "timed-dm", m: { time: number; text: string; color: string; user: string }): void;
}>();

/**
 * YouTube / Twitch 走「官方 iframe 播放器」而不是自己拉流 —— 原因见 ../embed.ts。
 */
const isEmbed = computed(() => isEmbedPlatform(props.platform));
const embedSrc = computed(() =>
  isEmbed.value ? buildEmbedSrc(props.platform || "", props.roomId || "") : "",
);

const wrapRef = ref<HTMLDivElement | null>(null);
const videoRef = ref<HTMLVideoElement | null>(null);
/** 官方 iframe 播放器（YouTube / Twitch / SOOP）。用来判断「浏览器全屏的元素是不是它」——
 *  SOOP 官方播放器自带全屏按钮，进去以后要同步关掉我们自己的窗口全屏。 */
const embedRef = ref<HTMLIFrameElement | null>(null);

/**
 * SOOP 画质（**结论：官方 embed 调不了，已实测**）
 *
 * embed 页面把画质选择框硬关掉了（HTML 里 `<!-- 화질선택 임베디드는 미노출 -->`），
 * 画质走 auto，在 WebView 里实测落在 640x360 SD 档。
 *
 * 播放器内部确实有 postMessage 通道：
 *   window.addEventListener("message", t => t.data.cmd && this[t.data.cmd](t.data))
 * 但**父页面能发的命令只有** Pload / postData / postMediaEvent / postSetDialog /
 * receivedExtension / isPlayWatching / isLivePageWatching / PisPlayerWatching。
 * 命令表里那些 `changeQuality`、`setQualityList`、`initShowQualityBox` 是
 * **播放器发给父页面的通知**，反着发过去 `typeof this[cmd] !== "function"`，被直接丢弃。
 *
 * 实测记录（直接加载 embed 页，同源可查 DOM）：
 *   初始                    .quality_box → display:none
 *   Pload + showQualityBox  .quality_box → display:none
 *   → 官方在 embed 模式下用 CSS 关死，跨域改不了。
 *
 * 所以 SOOP 保持官方播放器 + auto 画质。想上 1080p 只能走自建流，
 * 但自建流的 aid 时效太短（进播放器后分片被 abort、报「网络连接失败」），
 * 要做得先解决「分片失败时自动重新走四步鉴权换新 aid」。
 */
const status = ref("");
const err = ref("");
const playing = ref(false);
const muted = ref(false);
const volume = ref(Number(localStorage.getItem("junlive.volume") ?? "1"));
const cur = ref(0);
const dur = ref(0);
const bufStart = ref(0);
const bufEnd = ref(0);
const showCtrl = ref(true);
const showLines = ref(false);

/**
 * 自动播放被拒 → 需要用户点一下画面才能起播。
 *
 * 之前 `autoplay()` 里第二次重试的失败是 `.catch(() => {})` —— 静默吞掉。
 * 后果是**起播失败时用户完全看不出发生了什么**：没有报错、没有提示，
 * 画面停在第一帧（有时还播了一小段声音，因为声音在失败前已经起播），
 * 看起来就像「一直卡在缓冲中」。
 *
 * 现在置位 needTapToPlay，UI 会明确提示「点一下播放」，
 * 而不是让人对着一个永远不动的画面干瞪眼。
 */
const needTapToPlay = ref(false);

/** 用户点了画面（或点了播放按钮）：重新起播并清掉提示 */
function resumeFromTap() {
  const v = videoRef.value;
  if (!v) return;
  needTapToPlay.value = false;
  v.play()
    .then(() => {
      playing.value = true;
      poke();
    })
    .catch(() => {});
}

/** 按画质分组，每组是该画质下的若干条 CDN 线路 */
const qualities = computed(() => {
  const m = new Map<string, PlayUrl[]>();
  for (const p of props.plays ?? []) {
    const k = (p.quality || p.format.toUpperCase()).trim();
    if (!m.has(k)) m.set(k, []);
    m.get(k)!.push(p);
  }
  return [...m.entries()].map(([name, list]) => ({ name, list }));
});

/** 当前按钮上的文字：画质（+ 线路号） */
const currentLabel = computed(() => {
  const g = qualities.value.find((x) =>
    x.list.some((p) => p.url === props.play?.url),
  );
  if (!g) return props.play?.quality || "画质";
  const i = g.list.findIndex((p) => p.url === props.play?.url);
  return g.list.length > 1 ? `${g.name} · 线路${i + 1}` : g.name;
});
/** 清晰度按钮上的文字：当前实际在播的档位 */
const levelLabel = computed(() => {
  // 手动选了档位就显示那档；自动模式显示 ABR 当前实际在播的档。
  const idx = lockedLevel >= 0 ? lockedLevel : curLevel;
  const l = hlsLevels.value.find((x) => x.index === idx);
  return l ? `${l.height}p` : "清晰度";
});
const res = ref("");
const kbps = ref("");

let hls: Hls | null = null;
let flv: mpegts.Player | null = null;
let hideTimer: number | undefined;
let tick: number | undefined;
let retried = 0;
/** 锁定的档位下标，-1 表示不锁（交给 ABR） */
let lockedLevel = -1;
/** 用户锁定的高度（如 720），比下标可靠 —— levels 顺序会变，高度不会。
 *  -1 表示没锁。 */
let lockedHeight = -1;

/**
 * 清晰度档位处理：
 *   - 用户选了一档 → 把该档当作**上限**（autoLevelCapping），
 *     ABR 在上限以下自由选，带宽不足会自己降档，不会卡住。
 *   - 没选 → 完全交给 ABR。
 *   - 刻意**不锁最高档**：锁最高会强行拉 1080p，
 *     Twitch 走代理抖动大（实测 84ms~1891ms），大分片更容易抽干缓冲。
 *
 * Twitch / YouTube 给的是「主播放列表」，内部含多档码率，hls.js 默认随带宽
 * 自动切换（ABR），表现就是清晰度一直变。给 currentLevel 赋 >=0 的值会关掉自动切换。
 *
 * 注意别直接取最后一档 —— YouTube 的列表能到 4K/8K（maxh/4320），锁到那种档位
 * 会去拉超大分片，反而直接报网络错误。所以取「不超过 1080p 的最高档」。
 *
 * 只设 currentLevel 不够：直播的主列表会周期性刷新，hls.js 重新解析后 ABR 会「复活」。
 * 所以同时设 autoLevelCapping 兜住上限，并在 LEVEL_SWITCHED 里发现漂了就拉回来。
 */
/** hls.js 里的可用档位，供 UI 展示（{index, height, bitrate}） */
const hlsLevels = ref<{ index: number; height: number; bitrate: number }[]>([]);
/** 用户选的档位下标；-1 = 自动（交给 ABR） */
const wantLevel = ref(-1);
/** 自动模式下 hls.js 实际选中的档位，UI 用来显示当前清晰度 */
let curLevel = -1;
const showLevels = ref(false);

/** 把清晰度锁到一档固定值，别再自动变 */
function lockLevel(h: Hls) {
  // 先把可用档位暴露给 UI（每次主列表刷新都会变）
  hlsLevels.value = (h.levels ?? [])
    .map((lv, i) => ({ index: i, height: lv.height || 0, bitrate: lv.bitrate || 0 }))
    .filter((x) => x.height > 0)
    .sort((a, b) => b.height - a.height);

  if (!h.levels || h.levels.length <= 1) {
    lockedLevel = -1;
    return;
  }

  // 用户明确选过档位 → 钉死那一档，不让 ABR 乱跳。
  // 直播主列表会周期刷新，levels 的顺序/下标都可能变，
  // 所以优先按**高度**找回用户选的那一档，找不到才退回下标。
  if (lockedHeight > 0) {
    const byHeight = h.levels.findIndex((lv) => (lv.height || 0) === lockedHeight);
    if (byHeight >= 0) {
      pinLevel(h, byHeight);
      wantLevel.value = byHeight;
      return;
    }
  }
  if (wantLevel.value >= 0 && h.levels[wantLevel.value]) {
    pinLevel(h, wantLevel.value);
    return;
  }

  // 没选过就交给 hls.js 的 ABR 自己判断带宽选档。
  // 刻意**不锁最高档**：之前锁 ≤1080p 会强行拉 1080p，
  // 而 Twitch 走代理时抖动大（实测 84ms~1891ms），大码率分片更容易
  // 把缓冲抽干 → 一卡一卡。交给 ABR 降档反而更流畅。
  lockedLevel = -1;
  h.autoLevelCapping = -1;
}

/**
 * 把某一档「钉死」：关掉自动切档，并反复把 currentLevel 拉回来。
 *
 * 为什么必须反复拉：直播的主播放列表会周期性刷新，hls.js 每次重新解析
 * 都会让 ABR「复活」——表现就是用户明明选了 720p，过一会儿又自己跳到 1080p，
 * 或者带宽波动时被降到 480p。只在 pickLevel 里设一次 currentLevel 是不够的。
 */
function pinLevel(h: Hls, i: number) {
  if (!h.levels?.[i]) return;
  lockedLevel = i;
  lockedHeight = h.levels[i].height || -1;
  // autoLevelCapping = i 的语义是「不许超过第 i 档」，
  // 也就是把 i 当**上限**用：ABR 仍可在这个上限以下自由选，
  // 带宽不足时会自动降到 480p/360p，而不是卡住不动。
  //
  // 之前设 h.currentLevel = i 是「锁死那一档」—— 带宽不够时它只会
  // 反复重试同一个大分片，表现就是一直「缓冲中」。上限模式没有这个问题。
  h.autoLevelCapping = i;
  // 关键：不要设 currentLevel，设了就等于锁死，ABR 不会动了。
}

/** 用户点了某一档清晰度（i = -1 表示「自动」，交给 ABR） */
function pickLevel(i: number) {
  wantLevel.value = i;
  showLevels.value = false;
  if (!hls) return;
  if (i < 0) {
    // 「自动」：解除上限，交回给 hls.js 的 ABR
    lockedLevel = -1;
    lockedHeight = -1;
    hls.autoLevelCapping = -1;
    return;
  }
  pinLevel(hls, i);
}

/** 直播流没有有限时长 */
const isLive = computed(() => !isFinite(dur.value) || dur.value <= 0);

/** 进度条时间窗口：直播时取最近 60 秒滑动窗口 */
const winStart = computed(() => {
  if (!isLive.value) return 0;
  const end = bufEnd.value || cur.value;
  return Math.max(0, end - Math.max(60, end - (bufStart.value || 0)));
});
const winLen = computed(() => {
  if (!isLive.value) return dur.value || 1;
  const end = bufEnd.value || cur.value + 1;
  return Math.max(1, end - winStart.value);
});
const playedPct = computed(() =>
  Math.min(100, Math.max(0, ((cur.value - winStart.value) / winLen.value) * 100)),
);
const bufPct = computed(() =>
  Math.min(100, Math.max(0, ((bufEnd.value - winStart.value) / winLen.value) * 100)),
);
/** 直播延迟（秒） */
const latency = computed(() => {
  if (!isLive.value) return 0;
  const end = bufEnd.value || cur.value;
  return Math.max(0, end - cur.value);
});

function fmt(t: number): string {
  if (!isFinite(t) || t < 0) t = 0;
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = Math.floor(t % 60);
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

function onProgress() {
  const v = videoRef.value;
  if (!v) return;
  cur.value = v.currentTime || 0;
  dur.value = v.duration || 0;
  if (v.buffered.length) {
    bufStart.value = v.buffered.start(0);
    bufEnd.value = v.buffered.end(v.buffered.length - 1);
  }
}

function onSeek(e: MouseEvent) {
  const v = videoRef.value;
  const bar = e.currentTarget as HTMLElement;
  if (!v || !bar) return;
  const r = bar.getBoundingClientRect();
  const p = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  const target = winStart.value + p * winLen.value;
  if (isFinite(target)) {
    v.currentTime = Math.max(0, target);
    onProgress();
  }
}

/** 追到直播最新画面 */
function jumpLive() {
  const v = videoRef.value;
  if (!v) return;
  if (hls && typeof hls.latency === "number" && hls.latency > 0) {
    v.currentTime = v.currentTime + hls.latency;
  } else {
    const end = v.buffered.length ? v.buffered.end(v.buffered.length - 1) : 0;
    if (end > 0) v.currentTime = Math.max(0, end - 1);
  }
  v.play().catch(() => {});
}

function toggle() {
  const v = videoRef.value;
  if (!v) return;
  if (v.paused) {
    needTapToPlay.value = false;
    v.play().catch(() => {});
  } else v.pause();
}

function toggleMute() {
  const v = videoRef.value;
  if (!v) return;
  v.muted = !v.muted;
  muted.value = v.muted;
  if (!v.muted && v.volume === 0) {
    v.volume = 0.6;
    volume.value = 0.6;
  }
}

function onVolume() {
  const v = videoRef.value;
  if (!v) return;
  v.volume = volume.value;
  v.muted = volume.value === 0;
  muted.value = v.muted;
  localStorage.setItem("junlive.volume", String(volume.value));
}

/**
 * 窗口全屏（CSS 全屏）。
 *
 * 和浏览器 Fullscreen API 的区别：那个会把窗口切到系统全屏（还会挡住弹幕层、
 * 有些平台 iframe 还会抢），而这个是**纯 CSS** 把播放器铺满整个应用窗口
 * （position: fixed; inset: 0），顶栏和弹幕栏都被盖住 —— DTV 用的就是这个
 * （xgplayer 的 cssFullscreen）。
 */
const cssFull = ref(false);
function toggleCssFull() {
  cssFull.value = !cssFull.value;
}

/**
 * 浏览器全屏（系统级，走 Fullscreen API）。
 *
 * 注意：iframe 那边**故意没加 allowfullscreen** ——
 * YouTube / Twitch 官方播放器自带的全屏是 iframe 内部全屏，我们的弹幕层在 iframe
 * 外面会被整个盖住；想用 fullscreenchange 拦截换成自己的全屏也不可行
 * （requestFullscreen() 需要用户手势，事件回调里手势已过期，只会缩回来）。
 * 所以去掉 allowfullscreen，全屏统一走我们自己的按钮。
 */
function fullscreen() {
  const el = wrapRef.value;
  if (!el) return;
  if (document.fullscreenElement) document.exitFullscreen();
  else el.requestFullscreen().catch(() => {});
}

function poke() {
  showCtrl.value = true;
  if (hideTimer) window.clearTimeout(hideTimer);
  hideTimer = window.setTimeout(() => {
    if (playing.value && !videoRef.value?.paused) showCtrl.value = false;
  }, 2600);
}

function onKey(e: KeyboardEvent) {
  const tag = (e.target as HTMLElement)?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return;
  // Esc 退出窗口全屏。这条必须放在 video 检查之前 ——
  // embed 平台（YouTube/Twitch/SOOP）没有 <video>，否则按 Esc 没反应。
  if (e.key === "Escape" && cssFull.value) {
    cssFull.value = false;
    return;
  }
  const v = videoRef.value;
  if (!v) return;
  switch (e.key) {
    case " ":
    case "k":
      e.preventDefault();
      toggle();
      break;
    case "ArrowLeft":
      v.currentTime = Math.max(0, v.currentTime - 5);
      break;
    case "ArrowRight":
      v.currentTime = v.currentTime + 5;
      break;
    case "ArrowUp":
      volume.value = Math.min(1, volume.value + 0.05);
      onVolume();
      break;
    case "ArrowDown":
      volume.value = Math.max(0, volume.value - 0.05);
      onVolume();
      break;
    case "m":
      toggleMute();
      break;
    case "f":
      fullscreen();
      break;
    case "l":
      jumpLive();
      break;
  }
  poke();
}

function destroy() {
  lockedLevel = -1;
  if (hls) {
    try {
      hls.destroy();
    } catch {}
    hls = null;
  }
  if (flv) {
    try {
      flv.destroy();
    } catch {}
    flv = null;
  }
  const v = videoRef.value;
  if (v) {
    v.removeAttribute("src");
    try {
      v.load();
    } catch {}
  }
  res.value = "";
  kbps.value = "";
  cur.value = 0;
  bufStart.value = 0;
  bufEnd.value = 0;
  playing.value = false;
}

function attach(p: PlayUrl | null) {
  destroy();
  err.value = "";
  status.value = "";
  // 官方 iframe 播放器自带播放逻辑，不接 hls.js / mpegts
  if (isEmbed.value) return;
  const v = videoRef.value;
  if (!p || !v) return;
  status.value = "正在连接…";
  v.volume = volume.value;
  v.muted = volume.value === 0;
  muted.value = v.muted;

  const autoplay = () => {
    v.play()
      .then(() => {
        playing.value = true;
        poke();
      })
      .catch(() => {
        // 第一次失败（多半是带声音的自动播放被拒）→ 降级静音重试。
        v.muted = true;
        muted.value = true;
        v.play()
          .then(() => (playing.value = true))
          .catch(() => {
            // 第二次也失败：**不要静默吞掉**（详见 needTapToPlay 的注释）。
            // 置位后 UI 会提示「点一下播放」，用户点一下就恢复。
            needTapToPlay.value = true;
          });
      });
  };

  if (p.format === "flv") {
    if (!mpegts.isSupported()) {
      err.value = "当前环境不支持 FLV 播放";
      return;
    }
    // 缓冲**按平台区分**，别一刀切 —— 缓冲大 = 延迟大，不该给不需要的平台。
    //
    // - 斗鱼：FLV 直链每 300 秒被 CDN 主动断开，后端要续流，中间必然有一小段
    //   没有数据。缓冲给到 6~12 秒才扛得住，代价就是延迟多几秒。
    // - 虎牙：同样是 FLV，但直链不会被周期性切断，给大缓冲只是白白多延迟，
    //   只留 1~3 秒抗抖动余量。
    const flvLatency =
      props.platform === "douyu"
        ? { liveBufferLatencyMaxLatency: 12, liveBufferLatencyMinRemain: 6 }
        : { liveBufferLatencyMaxLatency: 3, liveBufferLatencyMinRemain: 1 };

    const player = mpegts.createPlayer(
      { type: "flv", isLive: true, url: p.proxy },
      {
        // 注意别加 autoCleanupSourceBuffer —— 它会把还没播到的关键帧清掉，
        // 表现是「播一会儿卡在缓存中」，上一版就是栽在这。
        enableStashBuffer: true,
        // stashInitialSize 单位是**字节**（默认 384*1024）。之前写 128
        // 等于 128 字节、形同没有，顺手改回默认值。
        stashInitialSize: 384 * 1024,
        // 有界追帧：缓冲超过 maxLatency 才追，追到剩 minRemain 停。
        // 完全关掉的话延迟会无限累积（实测每次续流涨约 3 秒）。
        liveBufferLatencyChasing: true,
        ...flvLatency,
        lazyLoad: false,
      },
    );
    player.attachMediaElement(v);
    player.on(mpegts.Events.ERROR, (_t: unknown, d: unknown) => {
      // 同样是地址过期占多数（斗鱼的 FLV 地址 300 秒就失效），
      // 原地重连没用，得重新解析。
      askRefresh("FLV 断流");
      err.value = `FLV 播放出错：${String(d)}`;
    });
    player.load();
    const pr = player.play() as unknown;
    if (pr instanceof Promise) pr.catch(() => {});
    status.value = "";
    flv = player;
    autoplay();
    return;
  }

  if (Hls.isSupported()) {
    // 缓冲**按平台区分**，别一刀切 —— 缓冲大 = 延迟大。
    //
    // - 海外平台（Twitch / SOOP）必须走代理，实测代理节点延迟抖动可达
    //   84ms → 1891ms，缓冲给小了会被瞬间抽干 → 反复「缓冲中」。
    //   代价是延迟多十几秒、起播慢，换来不卡。
    // - 国内平台（B站 / 抖音）直连，延迟本来就稳，给大缓冲只是白白多十几秒延迟，
    //   完全没必要。
    //
    // 另外：都不要开 lowLatencyMode —— 它靠「减小缓冲 + 快速追帧」换延迟，
    // 网络一抖就露馅（实测 Twitch 自建流会一卡一卡）。
    const overseas = props.platform === "twitch" || props.platform === "soop";
    const hlsBuf = overseas
      ? {
          liveSyncDurationCount: 8,
          maxBufferLength: 60,
          maxMaxBufferLength: 120,
          fragLoadingMaxRetry: 10,
        }
      : {
          liveSyncDurationCount: 3,
          maxBufferLength: 15,
          maxMaxBufferLength: 30,
          fragLoadingMaxRetry: 4,
        };
    const h = new Hls({
      ...hlsBuf,
      backBufferLength: 30,
      manifestLoadingMaxRetry: 6,
      manifestLoadingTimeOut: 30000,
      fragLoadingRetryDelay: 800,
      // 缓冲快空时不要立刻 panic，而是先尝试补一段再决定。
      startFragPrefetch: true,
      enableWorker: true,
    });
    h.loadSource(p.proxy);
    h.attachMedia(v);
    h.on(Hls.Events.MANIFEST_PARSED, () => {
      lockLevel(h);
      status.value = "";
      autoplay();
    });
    h.on(Hls.Events.LEVEL_SWITCHED, (_e, data) => {
      const lv = h.levels?.[data.level];
      if (lv) {
        res.value = lv.width && lv.height ? `${lv.width}×${lv.height}` : "";
        kbps.value = lv.bitrate ? `${Math.round(lv.bitrate / 1000)} kbps` : "";
      }
      // 直播的主列表会周期性刷新，hls.js 重新解析后 ABR 会「复活」，
      // 表现就是清晰度又开始自己变。这里发现漂了就拉回来。
      // 只有「用户手动选过档位」才纠正漂移。
      // lockedLevel === -1 表示交给 ABR，此时 data.level 变化是正常的
      // （带宽不足自动降档反而更流畅），不能拉回来。
      // 上限模式（autoLevelCapping = lockedLevel）下不干预，
      // 只记录当前实际档位供 UI 显示 —— 让 ABR 在上限内自由升降，
      // 这样带宽不足时它会降档而不是卡住。
      if (lockedLevel < 0) {
        // 自动模式下记录当前实际档位，UI 上要显示 1080p/720p 这些。
        curLevel = data.level;
      }
    });
    h.on(
      Hls.Events.ERROR,
      (_e: unknown, data: { fatal: boolean; type: string; details: string }) => {
        if (!data.fatal) return;
        if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
          // 先原地重试几次（偶发丢包有用）
          if (retried++ < 3) {
            status.value = `网络错误，第 ${retried} 次重试…`;
            h.startLoad();
            return;
          }
          // 重试还不行 —— 大概率是播放地址过期了（时效就是几分钟），
          // 原地重试永远用的是旧地址，必须向上层要一份新的。
          askRefresh("网络中断");
        } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
          status.value = "解码错误，恢复中…";
          h.recoverMediaError();
          return;
        } else {
          err.value = `播放失败：${data.details}`;
        }
        status.value = "";
      },
    );
    hls = h;
  } else if (v.canPlayType("application/vnd.apple.mpegurl")) {
    v.src = p.proxy;
    status.value = "";
    autoplay();
  } else {
    err.value = "当前环境不支持 HLS 播放";
  }
}

/**
 * 向上层要一份新的播放地址。
 *
 * 所有平台的地址都带时效（斗鱼 300s、B站 expires、SOOP aid、Twitch token），
 * 过期后原地重试（hls.startLoad()）用的还是旧地址，怎么试都没用 ——
 * 必须让上层重新解析一次房间。
 *
 * 限流很重要：一次断流会连着抛好几个致命错误，不拦的话会把上层刷爆，
 * 而重新解析是要发网络请求的，反复打断反而更慢。
 */
let askedAt = 0;
function askRefresh(why: string) {
  const now = Date.now();
  if (now - askedAt < 8000) return;
  askedAt = now;
  retried = 0;
  err.value = "";
  status.value = `${why}，正在重新解析地址…`;
  emit("refresh");
}

function reload() {
  retried = 0;
  attach(props.play);
}

function onPlay() {
  playing.value = true;
  poke();
}
function onPause() {
  playing.value = false;
  showCtrl.value = true;
}

/**
 * SOOP 官方播放器自带全屏按钮（我们在它的 iframe 上开了 allowfullscreen）。
 * 用户在 iframe 里按全屏时，父页的 `document.fullscreenElement` 会变成那个 iframe，
 * 而它内部的按键（含 ESC）由**浏览器**处理、不会冒泡到我们这边 ——
 * 所以这条同步很关键：一旦官方全屏生效就把我们自己的窗口全屏关掉，
 * 免得两层全屏叠着打架，退出时也退不干净。
 */
function onFsChange() {
  if (document.fullscreenElement && document.fullscreenElement === embedRef.value) {
    cssFull.value = false;
  }
}

onMounted(() => {
  attach(props.play);
  tick = window.setInterval(onProgress, 500);
  window.addEventListener("keydown", onKey);
  document.addEventListener("fullscreenchange", onFsChange);
  poke();
});
watch(
  () => props.play,
  (p) => {
    retried = 0;
    attach(p);
  },
);
onBeforeUnmount(() => {
  destroy();
  if (dmRaf) cancelAnimationFrame(dmRaf);
  if (tick) window.clearInterval(tick);
  if (hideTimer) window.clearTimeout(hideTimer);
  window.removeEventListener("keydown", onKey);
  document.removeEventListener("fullscreenchange", onFsChange);
});


// ---------------------------------------------------------------- 弹幕公屏

// 弹幕设置（开关/透明度/字号/速度/区域/屏蔽词）统一放在 ../dmSettings，
// 这里和 DmSet.vue 共用同一份，存 localStorage。

interface FlyingDm {
  id: number;
  text: string;
  color: string;
  top: number;
  dur: number;
}

const flying = ref<FlyingDm[]>([]);
let dmSeq = 0;
let dmTrack = 0;

/**
 * 弹幕是高频的（热门房间每秒几十条）。
 * 每来一条就直接 push 进响应式数组，Vue 就会重渲一次列表 ——
 * 表现就是画面发涩、切页卡顿。
 *
 * 所以先塞进待处理队列，用 requestAnimationFrame **每帧批量合并一次**。
 */
let dmPending: FlyingDm[] = [];
let dmRaf = 0;
/** 同屏最多保留多少条，超出丢最老的（防止 DOM 越堆越多拖慢合成） */
const DM_MAX = 160;

function flushDm() {
  dmRaf = 0;
  if (!dmPending.length) return;
  const next = flying.value.concat(dmPending);
  dmPending = [];
  flying.value = next.length > DM_MAX ? next.slice(next.length - DM_MAX) : next;
}

/** 把一条弹幕丢到公屏上飘 */
function flyDanmaku(text: string, color = "") {
  if (!dmOn.value || !text.trim()) return;
  if (isBlocked(text)) return;
  const h = wrapRef.value?.clientHeight || 400;
  const lineH = dmSize.value + 8;
  // 只占用画面上方的 dmArea 比例
  const tracks = Math.max(1, Math.floor((h * dmArea.value) / lineH));
  dmTrack = (dmTrack + 1) % tracks;
  const id = ++dmSeq;
  dmPending.push({
    id,
    text: text.trim(),
    color: dmPickColor(color),
    top: dmTrack * lineH + 6,
    dur: dmSpeed.value,
  });
  if (!dmRaf) dmRaf = requestAnimationFrame(flushDm);
  // 飘完就回收，避免 DOM 越堆越多
  window.setTimeout(() => {
    flying.value = flying.value.filter((f) => f.id !== id);
  }, dmSpeed.value * 1000 + 300);
}

// 监听弹幕数组的新增项（只处理新增，避免重复飘）
watch(
  () => props.danmaku?.length ?? 0,
  (n, old) => {
    if (!props.danmaku || !dmOn.value) return;
    const from = old ?? 0;
    if (n <= from) return;
    // 一次最多补 6 条，防止切换房间时刷屏
    const start = Math.max(from, n - 6);
    for (let i = start; i < n; i++) {
      const m = props.danmaku[i];
      if (m) flyDanmaku(m.text, m.color);
    }
  },
);

/* ---------------- 历史弹幕（回放） ----------------
 *
 * 整份弹幕是按时间轴来的，播放到哪就飘到哪。
 * 用 <video> 的 timeupdate（约 4 次/秒）驱动：够及时，又不用 rAF 空转。
 */
const timedList = computed(() =>
  [...(props.timedDanmaku ?? [])].sort((a, b) => a.time - b.time),
);
let timedIdx = 0;

// 换了一份弹幕（换段/换场次）就重头来
watch(
  () => props.timedDanmaku,
  () => {
    timedIdx = 0;
  },
);

function pumpTimedDm() {
  const v = videoRef.value;
  const list = timedList.value;
  if (!v || !list.length) return;
  const t = v.currentTime;
  // 拖动进度条之后游标要重新定位，否则要么补飘一大堆、要么再也不飘
  if (timedIdx > 0 && list[timedIdx - 1] && list[timedIdx - 1].time > t + 1) {
    let i = 0;
    while (i < list.length && list[i].time < t) i++;
    timedIdx = i;
    return;
  }
  // 一次最多补 6 条，避免刚起播时刷屏
  let fired = 0;
  while (timedIdx < list.length && list[timedIdx].time <= t && fired < 6) {
    const m = list[timedIdx++];
    fired++;
    if (dmOn.value) flyDanmaku(m.text, m.color);
    emit("timed-dm", m);
  }
}

/** 跳到指定秒（回放看点用） */
function seek(t: number) {
  const v = videoRef.value;
  if (v) v.currentTime = Math.max(0, t);
}

defineExpose({ reload, seek });
</script>

<template>
  <div
    ref="wrapRef"
    class="player"
    :class="{
      hide: !showCtrl,
      'css-full': cssFull,
      'embed-mode': isEmbed,
      'soop-embed': isEmbed && props.platform === 'soop',
    }"
    @mousemove="poke"
    @click="poke"
  >
    <!-- 走官方 iframe 的平台（YouTube / Twitch，以及切成「官方播放页」的 SOOP） -->
    <iframe
      v-if="isEmbed"
      ref="embedRef"
      class="embed"
      :src="embedSrc"
      :title="props.platform === 'soop' ? 'SOOP 播放器' : 'YouTube 播放器'"
      frameborder="0"
      allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
      :allowfullscreen="props.platform === 'soop'"
    ></iframe>

    <!-- YouTube 走官方 iframe、没有自定义控制栏，把弹幕设置和全屏放在画面右下角。
         全屏要用我们自己的（对 .player 全屏），这样弹幕层才还在。 -->
    <div v-if="isEmbed" class="embed-tools">
      <DmSet />
      <button
        class="ico"
        :class="{ on: cssFull }"
        :title="cssFull ? '退出窗口全屏' : '窗口全屏（铺满应用窗口）'"
        @click.stop="toggleCssFull"
      >
        ⛶
      </button>
      <button class="ico" title="系统全屏" @click.stop="fullscreen">⤢</button>
    </div>

    <video
      v-if="!isEmbed"
      ref="videoRef"
      class="vid"
      playsinline
      @dblclick="fullscreen"
      @click.stop="toggle"
      @play="onPlay"
      @pause="onPause"
      @timeupdate="pumpTimedDm"
      @ended="emit('ended')"
      @waiting="status = status || '缓冲中…'"
      @playing="status = ''"
    ></video>

    <div v-if="status && !isEmbed" class="veil">
      <div class="spinner"></div>
      <div class="veil-text">{{ status }}</div>
    </div>

    <!--
      自动播放被拒：明确告诉用户点一下，而不是让画面静止着、
      也不给任何提示（之前是 .catch(() => {}) 静默吞掉，看起来像「卡在缓冲」）。
    -->
    <div v-if="needTapToPlay && !isEmbed" class="veil">
      <div class="veil-text">点击画面开始播放</div>
      <button class="primary" @click.stop="resumeFromTap">播放</button>
    </div>

    <div v-if="err && !isEmbed" class="veil error">
      <div class="veil-text">{{ err }}</div>
      <button class="primary" @click.stop="reload">重试</button>
    </div>

    <!-- 弹幕公屏：YouTube 也能飞屏（弹幕走自建的 live_chat 长轮询） -->
    <div
      v-if="dmOn"
      class="dm-layer"
      :style="{ opacity: dmOpacity }"
    >
      <div
        v-for="f in flying"
        :key="f.id"
        class="dm-fly"
        :style="{
          top: f.top + 'px',
          color: f.color,
          fontSize: dmSize + 'px',
          animationDuration: f.dur + 's',
        }"
      >
        {{ f.text }}
      </div>
    </div>

    <div
      v-if="!playing && !status && !err && !isEmbed"
      class="bigplay"
      @click.stop="toggle"
    >
      ▶
    </div>

    <div v-if="!isEmbed" class="ctrl" :class="{ show: showCtrl }" @click.stop>
      <div class="bar" @click="onSeek">
        <div class="track">
          <div class="buffered" :style="{ width: bufPct + '%' }"></div>
          <div class="played" :style="{ width: playedPct + '%' }"></div>
        </div>
        <div class="knob" :style="{ left: playedPct + '%' }"></div>
      </div>

      <div class="row">
        <button class="ico" :title="playing ? '暂停 (空格)' : '播放 (空格)'" @click="toggle">
          {{ playing ? "❚❚" : "▶" }}
        </button>

        <button class="ico" :title="muted ? '取消静音 (M)' : '静音 (M)'" @click="toggleMute">
          {{ muted || volume === 0 ? "🔇" : "🔊" }}
        </button>
        <input
          class="vol"
          v-model.number="volume"
          type="range"
          min="0"
          max="1"
          step="0.02"
          @input="onVolume"
        />

        <span class="time">
          {{ fmt(cur) }}<template v-if="!isLive"> / {{ fmt(dur) }}</template>
        </span>

        <span class="spacer"></span>

        <span v-if="res" class="tag">{{ res }}</span>
        <span v-if="kbps" class="tag">{{ kbps }}</span>

        <button
          v-if="isLive"
          class="live"
          :class="{ warn: latency > 8 }"
          title="回到直播最新画面 (L)"
          @click="jumpLive"
        >
          <i class="dot"></i>
          <template v-if="latency > 1">延迟 {{ latency.toFixed(1) }}s · 追帧</template>
          <template v-else>实时</template>
        </button>

        <DmSet />

        <!--
          清晰度选择（HLS 档位）。
          Twitch 这类平台只有一条线路、但流内部含多档码率，
          所以上面的「画质 / 线路」按钮不会显示（qualities.length === 1），
          清晰度必须在这里选。Twitch 走代理时抖动大，
          1080p 容易卡，用户可以手动降到 720p 换取流畅。
        -->
        <div v-if="hlsLevels.length > 1" class="lines">
          <button class="line-btn" title="切换清晰度上限（带宽不足会自动降档）"
              @click="showLevels = !showLevels">
            <span class="lb-ico">⇅</span>
            <span class="lb-txt ellipsis">{{ levelLabel }}</span>
          </button>
          <div v-if="showLevels" class="line-menu">
            <button :class="{ on: wantLevel === -1 }" @click="pickLevel(-1)">
              自动（不设上限）
              <span v-if="wantLevel === -1" class="ck">✓</span>
            </button>
            <button
              v-for="l in hlsLevels"
              :key="l.index"
              :class="{ on: (lockedLevel >= 0 ? lockedLevel : curLevel) === l.index }"
              @click="pickLevel(l.index)"
            >
              {{ l.height }}p
              <span v-if="l.bitrate" class="lm-kbps">{{ Math.round(l.bitrate / 1000) }}k</span>
              <span v-if="l.index === wantLevel" class="lm-hint">上限</span>
              <span v-if="(lockedLevel >= 0 ? lockedLevel : curLevel) === l.index" class="ck">✓</span>
            </button>
          </div>
        </div>

        <div v-if="qualities.length" class="lines">
          <button class="line-btn" title="切换画质 / 线路" @click="showLines = !showLines">
            <span class="lb-ico">⇅</span>
            <span class="lb-txt ellipsis">{{ currentLabel }}</span>
          </button>
          <div v-if="showLines" class="line-menu">
            <div v-for="g in qualities" :key="g.name" class="lm-group">
              <div class="lm-title">{{ g.name }}</div>
              <button
                v-for="(p, i) in g.list"
                :key="i"
                :class="{ on: props.play?.url === p.url }"
                @click="
                  emit('pick', p);
                  showLines = false;
                "
              >
                <span class="ln ellipsis">
                  {{ g.list.length > 1 ? `线路 ${i + 1}` : "播放" }}
                </span>
                <span class="lf">{{ p.format.toUpperCase() }}</span>
              </button>
            </div>
          </div>
        </div>

        <button
          class="ico"
          :class="{ on: cssFull }"
          :title="cssFull ? '退出窗口全屏' : '窗口全屏（铺满应用窗口）'"
          @click="toggleCssFull"
        >
          ⛶
        </button>
        <button class="ico" title="系统全屏 (F / 双击)" @click="fullscreen">⤢</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.player {
  position: relative;
  width: 100%;
  height: 100%;
  background: #000;
  border-radius: 10px;
  overflow: hidden;
  user-select: none;
}
/* 窗口全屏（CSS 全屏）：铺满整个应用窗口，顶栏和弹幕栏都被盖住。
   纯 CSS 实现，不动系统窗口 —— 参考 DTV 用的 xgplayer cssFullscreen。 */
.player.css-full {
  position: fixed;
  inset: 0;
  width: 100vw;
  height: 100vh;
  border-radius: 0;
  z-index: 9999;
}
.player.hide {
  cursor: none;
}
video {
  width: 100%;
  height: 100%;
  display: block;
  background: #000;
}

/* 官方 iframe 播放器（YouTube 等） */
.embed {
  width: 100%;
  height: 100%;
  display: block;
  border: 0;
  background: #000;
}

/* 官方播放器模式：iframe 让出底部 34px 给工具条，弹幕层也收窄到画面区域，
   这样弹幕不会飘到工具条上，工具条也不会压住官方的控件栏。 */
.embed-mode .embed {
  height: calc(100% - 34px);
}
.embed-mode .dm-layer {
  bottom: 34px;
}

/* 全屏时的处理：不再预留底部那一条（会把画面压扁、还空出一条很难看的横条），
   让 iframe 铺满；工具条改成浮在画面右下角、比官方控件栏再高一点（bottom:52px）
   以免遮挡官方的画质/音量按钮，并跟随控制栏一起自动隐藏（.hide）。 */
.player.css-full.embed-mode .embed {
  height: 100%;
}
.player.css-full.embed-mode .dm-layer {
  bottom: 0;
}
.player.css-full .embed-tools {
  position: absolute;
  bottom: 52px;
  right: 10px;
  height: auto;
  border: 0;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.42);
  padding: 2px 4px;
  transition: opacity 0.2s;
}
.player.css-full.hide .embed-tools {
  opacity: 0;
  pointer-events: none;
}

/* ===== SOOP 官方播放器（只影响 soop，不动 YouTube / Twitch）=====
   它的控件栏常驻底部，顶部还有一条频道信息栏，只有右上角那块是空的。
   所以按钮放右上角，且**不占位**（iframe 铺满 100%），
   鼠标不动时跟着控制栏一起淡出，免得一直压在频道信息上。
   放在 .css-full 规则之后，保证全屏时位置不变。 */
.player.soop-embed .embed {
  height: 100%;
}
.player.soop-embed .dm-layer {
  bottom: 0;
}
.player.soop-embed .embed-tools {
  position: absolute;
  top: 8px;
  right: 44px;
  height: auto;
  border: 0;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.35);
  padding: 2px 4px;
  transition: opacity 0.2s;
}
.player.soop-embed.hide .embed-tools {
  opacity: 0;
  pointer-events: none;
}

.veil {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 13px;
  pointer-events: none;
}
.veil.error {
  color: #ffb3b3;
  pointer-events: auto;
}
.veil-text {
  max-width: 70%;
  text-align: center;
  line-height: 1.6;
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

.bigplay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 42px;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(0, 0, 0, 0.25);
  cursor: pointer;
}

.ctrl {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  padding: 22px 12px 8px;
  background: linear-gradient(to top, rgba(0, 0, 0, 0.82), rgba(0, 0, 0, 0));
  opacity: 0;
  transform: translateY(6px);
  transition: opacity 0.18s, transform 0.18s;
  pointer-events: none;
}
.ctrl.show {
  opacity: 1;
  transform: none;
  pointer-events: auto;
}

.bar {
  position: relative;
  height: 14px;
  cursor: pointer;
  display: flex;
  align-items: center;
}
.track {
  position: relative;
  width: 100%;
  height: 4px;
  background: rgba(255, 255, 255, 0.22);
  border-radius: 3px;
  overflow: hidden;
  transition: height 0.12s;
}
.bar:hover .track {
  height: 6px;
}
.buffered {
  position: absolute;
  inset: 0 auto 0 0;
  background: rgba(255, 255, 255, 0.35);
}
.played {
  position: absolute;
  inset: 0 auto 0 0;
  background: var(--accent);
}
.knob {
  position: absolute;
  top: 50%;
  width: 12px;
  height: 12px;
  margin-left: -6px;
  border-radius: 50%;
  background: #fff;
  transform: translateY(-50%) scale(0);
  transition: transform 0.12s;
  pointer-events: none;
}
.bar:hover .knob {
  transform: translateY(-50%) scale(1);
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
  color: #fff;
  font-size: 12px;
}
.spacer {
  flex: 1;
}
.ico {
  background: transparent;
  border: none;
  color: #fff;
  font-size: 14px;
  padding: 4px 6px;
  line-height: 1;
  opacity: 0.9;
}
.ico:hover {
  opacity: 1;
  background: rgba(255, 255, 255, 0.12);
  border-radius: 5px;
}
/* 选中态（窗口全屏开着时高亮） */
.ico.on {
  color: var(--accent);
  opacity: 1;
}
/* 画面固定「适应」：完整显示不裁切，比例不符时留黑边 */
.vid {
  object-fit: contain;
  background: #000;
}
.vol {
  width: 72px;
  padding: 0;
  accent-color: var(--accent);
}
.time {
  font-variant-numeric: tabular-nums;
  opacity: 0.85;
  margin-left: 4px;
}
.tag {
  background: rgba(255, 255, 255, 0.14);
  border-radius: 4px;
  padding: 1px 6px;
  font-size: 11px;
  opacity: 0.9;
}
.live {
  display: flex;
  align-items: center;
  gap: 5px;
  background: rgba(255, 255, 255, 0.14);
  border: none;
  color: #fff;
  border-radius: 5px;
  padding: 3px 8px;
  font-size: 11px;
}
.live.warn {
  background: rgba(255, 92, 92, 0.28);
}
.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #ff4d4d;
  box-shadow: 0 0 6px #ff4d4d;
}

/* 弹幕公屏 */
.dm-layer {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
  z-index: 6;
  /* 独立合成层 + 限定重绘范围：弹幕再密也不会拖累视频解码 */
  contain: layout style paint;
  transform: translateZ(0);
}
.dm-fly {
  position: absolute;
  left: 100%;
  white-space: nowrap;
  font-weight: 600;
  line-height: 1.25;
  text-shadow:
    0 1px 2px rgba(0, 0, 0, 0.9),
    0 0 2px rgba(0, 0, 0, 0.75);
  animation-name: dmfly;
  animation-timing-function: linear;
  animation-fill-mode: forwards;
  will-change: transform;
  backface-visibility: hidden;
}
@keyframes dmfly {
  from {
    transform: translate3d(0, 0, 0);
  }
  to {
    transform: translate3d(calc(-100% - 100vw), 0, 0);
  }
}

/* 弹幕设置 */
/* YouTube 画面右下角的小工具条（弹幕设置 + 全屏） */
/* 官方 iframe 播放器的工具栏。
   **不能做成浮动层**：官方播放器（尤其 SOOP）底部那条控件栏是常驻的，
   浮动工具条压在它上面会挡住官方的画质/音量/全屏按钮。
   做法：让 iframe 让出底部一条，工具条做成静态的一行，
   视觉上像是播放器自带的下沿，永远不会和官方按钮抢位置。 */
.embed-tools {
  position: relative;
  height: 34px;
  z-index: 8;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 2px;
  padding: 0 6px;
  background: #14151a;
  border-top: 1px solid rgba(255, 255, 255, 0.08);
}

/* 线路下拉 */
.lines {
  position: relative;
}
.line-btn {
  display: flex;
  align-items: center;
  gap: 5px;
  max-width: 190px;
  background: rgba(255, 255, 255, 0.14);
  border: none;
  color: #fff;
  border-radius: 5px;
  padding: 3px 9px;
  font-size: 11px;
}
.line-btn:hover {
  background: rgba(255, 255, 255, 0.24);
}
.lb-ico {
  opacity: 0.85;
}
.lb-txt {
  max-width: 150px;
}
/* 清晰度菜单里的 kbps 标注与对勾 */
.lm-hint {
  margin-left: 6px;
  font-size: 10px;
  color: var(--brand);
  opacity: 0.75;
}
.lm-kbps {
  margin-left: 6px;
  font-size: 11px;
  color: var(--fg-dim);
}
.ck {
  margin-left: auto;
  color: var(--brand);
}
.line-menu {
  position: absolute;
  right: 0;
  bottom: 32px;
  min-width: 210px;
  max-height: 260px;
  overflow-y: auto;
  background: rgba(22, 24, 28, 0.96);
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 8px;
  padding: 5px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
  z-index: 10;
}
.line-menu button {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
  background: transparent;
  border: none;
  color: rgba(255, 255, 255, 0.85);
  border-radius: 6px;
  padding: 7px 9px;
  font-size: 12px;
  text-align: left;
}
.line-menu button:hover {
  background: rgba(255, 255, 255, 0.12);
}
.line-menu button.on {
  background: rgba(79, 140, 255, 0.3);
  color: #fff;
}
.lm-group + .lm-group {
  margin-top: 4px;
  padding-top: 4px;
  border-top: 1px solid rgba(255, 255, 255, 0.08);
}
.lm-title {
  font-size: 10.5px;
  color: rgba(255, 255, 255, 0.45);
  padding: 4px 9px 2px;
  letter-spacing: 0.5px;
}
.ln {
  flex: 1;
  min-width: 0;
}
.lf {
  font-size: 10px;
  opacity: 0.6;
  flex: none;
}
</style>
