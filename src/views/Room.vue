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
import Player from "../components/Player.vue";

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
      <button class="back" title="返回" @click="router.back()">←</button>

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
          <span v-if="detail?.live" class="live-tag">直播中</span>
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
            v-if="detail?.live"
            :play="current"
            :plays="detail?.plays ?? []"
            :danmaku="msgs"
            :platform="props.platform"
            :room-id="detail?.room_id || props.id"
            @pick="pick"
            @refresh="onRefresh"
          />

          <div v-if="loading" class="veil">
            <div class="spinner"></div>
            <span>正在解析直播间…</span>
          </div>
          <div v-else-if="error" class="warn">{{ error }}</div>
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
              {{ detail?.live ? "正在连接弹幕…" : "主播未开播，没有弹幕" }}
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
