<script setup lang="ts">
/**
 * 移动端关注页 —— 不复用 PC 版 src/views/Follow.vue。
 *
 * PC 版是多列网格 + 小按钮，靠鼠标操作；手机上那种密度点不准、也扫不动。
 * 移动版照 Simple Live 重做成「单列大行」：
 *   - 每行至少 56px 高（≥44px 触控热区），整行可点直接进直播间
 *   - 在线（品牌色点）在前、离线（灰点）在后 —— 排序在数据层做（见下方 sorted）
 *   - 取消关注走行末图标按钮 + 确认弹窗；**不做左滑删除**
 * 数据层完全复用 store：关注记录、在线状态、导出/导入逻辑都和 PC 版同源，
 * 所以两端数据天然一致。
 */
import { computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import { avatarUrl, getRoom, type Room } from "../api";
import { store, liveRank, type FollowItem } from "../store";

const router = useRouter();

/**
 * 后台刷在线状态：和 PC 版同一套逻辑。
 * 状态存在关注记录里（store.follows[].live 持久化），一进页面顺序就是对的，
 * 这里只负责异步刷新覆盖。并发限 2，避免一次打太多请求被平台限流。
 */
async function refreshStatus() {
  const list = [...store.follows];
  let idx = 0;
  const workers = Array.from({ length: Math.min(2, list.length) }, async () => {
    while (idx < list.length) {
      const f = list[idx++];
      try {
        const d = await getRoom(f.platform, f.room_id);
        store.setFollowLive(f.platform, f.room_id, d.live ? "LIVE" : "OFFLINE");
        store.setFollowInfo(f.platform, f.room_id, d.streamer, d.avatar);
      } catch {
        store.setFollowLive(f.platform, f.room_id, "UNKNOWN");
      }
    }
  });
  await Promise.all(workers);
}

onMounted(refreshStatus);

/**
 * 在线置顶：在播最前、未知中间、未开播最后。
 * 排序照搬 store 里现成的 liveRank（与 PC 版 src/views/Follow.vue 调用方式一致），
 * 不自己另写一套权重 —— 这是 Simple Live 在数据层 sort liveStatus 的同款做法。
 */
const sorted = computed(() =>
  [...store.follows].sort((a, b) => liveRank(a.live) - liveRank(b.live)),
);

/** 展示用头像地址（斗鱼相对路径的兜底在 avatarUrl 里） */
function showAvatar(f: FollowItem) {
  return avatarUrl(f.platform, f.avatar);
}

/** 加载失败时隐藏 <img>，露出底下的首字母占位 */
function hideImg(e: Event) {
  (e.target as HTMLImageElement).style.display = "none";
}

/** 首字母占位：没有头像或加载失败时显示主播名首字 */
function initial(f: FollowItem) {
  return (f.streamer || f.room_id || "?").slice(0, 1);
}

/**
 * 在线状态点的颜色。
 * 只用设计变量（不硬编码色值，深色模式才能跟着主题一起变）：
 *   在播 → --brand（品牌色，最醒目）
 *   未开播 → --fg-dim（灰）
 *   未知 → --border-2（更浅的中性色，避免被误当成在播）
 */
function liveColor(f: FollowItem): string {
  if (f.live === "LIVE") return "var(--brand)";
  if (f.live === "OFFLINE") return "var(--fg-dim)";
  return "var(--border-2)";
}

/** 状态文案：Simple Live 明确写出「直播中 / 未开播」，比只有色点更清楚 */
function liveText(f: FollowItem): string {
  if (f.live === "LIVE") return "直播中";
  if (f.live === "OFFLINE") return "未开播";
  return "未知";
}

function open(platform: string, roomId: string) {
  router.push(`/room/${platform}/${encodeURIComponent(roomId)}`);
}

/* ---------------- 取消关注 ---------------- */
/**
 * 取消关注：行末图标按钮 + 二次确认。
 *
 * 为什么不用左滑（Simple Live 也没有左滑）：左滑手势会和列表纵向滚动抢事件，
 * 手机上很容易误删；采用成熟做法，删除只走显式按钮。
 * toggleFollow 对已关注项就是删除；这里只用 platform / room_id，
 * 其余字段它不看，所以用类型断言最小化构造（与 PC 版调用方式一致）。
 */
function confirmRemove(f: FollowItem) {
  if (!window.confirm(`取消关注「${f.streamer || f.room_id}」？`)) return;
  store.toggleFollow({ platform: f.platform, room_id: f.room_id } as unknown as Room);
}

/* ---------------- 导出 / 导入 ---------------- */
/**
 * 导出/导入直接照抄 PC 版 src/views/Follow.vue 的调用方式 ——
 * 这两段逻辑没有抽成 store 方法，所以两端各自实现一份，
 * 但用同一个 localStorage 键（junlive.follows），格式天然互通。
 */
function exportJson() {
  const blob = new Blob([JSON.stringify(store.follows, null, 2)], {
    type: "application/json",
  });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "junlive-follows.json";
  a.click();
  URL.revokeObjectURL(a.href);
}

function importJson() {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".json";
  input.onchange = async () => {
    const f = input.files?.[0];
    if (!f) return;
    try {
      const arr = JSON.parse(await f.text());
      if (Array.isArray(arr)) {
        for (const item of arr) {
          if (!item?.platform || !item?.room_id) continue;
          if (!store.isFollowed(item.platform, item.room_id)) {
            store.follows.push({
              platform: item.platform,
              room_id: String(item.room_id),
              streamer: item.streamer || "",
              avatar: item.avatar || "",
              added: item.added || Date.now(),
            });
          }
        }
        localStorage.setItem("junlive.follows", JSON.stringify(store.follows));
      }
    } catch (e) {
      alert("导入失败：" + e);
    }
  };
  input.click();
}
</script>

<template>
  <div class="mf">
    <!-- 顶部：标题 + 数量，右侧导出/导入两个小按钮 -->
    <div class="mf-head">
      <div class="mf-title">
        我的关注<span v-if="store.follows.length" class="mf-count">{{ store.follows.length }}</span>
      </div>
      <div class="mf-actions">
        <button @click="exportJson">导出</button>
        <button @click="importJson">导入</button>
      </div>
    </div>

    <!-- 空状态：居中提示，引导去播放页点 ☆ -->
    <div v-if="!store.follows.length" class="mf-empty">
      <div class="mf-empty-ico">⭐</div>
      <div>还没有关注任何主播 —— 在播放页点 ☆ 即可添加</div>
    </div>

    <!-- 单列列表（在线在前、离线在后，顺序来自 sorted） -->
    <div v-else class="mf-list">
      <div
        v-for="f in sorted"
        :key="f.platform + '/' + f.room_id"
        class="mf-row"
        :class="{ off: f.live === 'OFFLINE' }"
        @click="open(f.platform, f.room_id)"
      >
        <div class="mf-ava">
          <span class="mf-ini">{{ initial(f) }}</span>
          <img
            v-if="showAvatar(f)"
            :src="showAvatar(f)"
            referrerpolicy="no-referrer"
            @error="hideImg"
          />
        </div>

        <div class="mf-info">
          <div class="mf-name ellipsis">{{ f.streamer || f.room_id }}</div>
          <div class="mf-sub">
            <span class="mf-live" :style="{ background: liveColor(f) }"></span>
            <span class="mf-status">{{ liveText(f) }}</span>
            <span class="mf-sep">·</span>
            {{ store.platformName(f.platform) }}
          </div>
        </div>

        <!-- 行末取消关注按钮：热区 44×44，点击即弹确认（取消冒泡，不触发进房） -->
        <button
          class="mf-unfollow"
          type="button"
          title="取消关注"
          aria-label="取消关注"
          @click.stop="confirmRemove(f)"
        >
          <svg
            viewBox="0 0 24 24"
            width="20"
            height="20"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path d="M4 7h16" />
            <path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
            <path d="M6 7l1 12a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-12" />
            <path d="M10 11v6M14 11v6" />
          </svg>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.mf {
  height: 100%;
  overflow-y: auto;
  background: var(--bg);
  /* 底部留白，避开底部标签栏 */
  padding-bottom: 12px;
}

.mf-head {
  position: sticky;
  top: 0;
  z-index: 2;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 12px 16px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
}
.mf-title {
  font-size: 16px;
  font-weight: 700;
  color: var(--fg);
  display: flex;
  align-items: center;
  gap: 6px;
}
.mf-count {
  font-size: 12px;
  font-weight: 600;
  color: var(--brand);
  background: var(--brand-soft);
  border-radius: 999px;
  padding: 1px 8px;
}
.mf-actions {
  display: flex;
  gap: 8px;
}
.mf-actions button {
  height: 32px;
  padding: 0 14px;
  font-size: 13px;
  border-radius: 999px;
}

.mf-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 100px 32px;
  text-align: center;
  color: var(--fg-dim);
  font-size: 14px;
  line-height: 1.6;
}
.mf-empty-ico {
  font-size: 34px;
  opacity: 0.7;
}

.mf-list {
  display: flex;
  flex-direction: column;
}
/* 行高 60px：满足 ≥56px 要求，也远超 44px 触控热区 */
.mf-row {
  display: flex;
  align-items: center;
  gap: 12px;
  height: 60px;
  /* 右侧留窄一点，把空间让给 44px 的取消按钮 */
  padding: 0 8px 0 16px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
  transition: background 0.12s;
  /* 禁掉文字选中，避免点击/长按选中文字 */
  user-select: none;
}
.mf-row:active {
  background: var(--chip);
}
/* 未开播：整行压暗，头像去色 */
.mf-row.off {
  opacity: 0.6;
}
.mf-row.off .mf-ava img {
  filter: grayscale(1);
}

.mf-ava {
  position: relative;
  width: 40px;
  height: 40px;
  flex: none;
  border-radius: 50%;
  overflow: hidden;
  background: var(--chip);
  display: flex;
  align-items: center;
  justify-content: center;
}
.mf-ava img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.mf-ini {
  font-size: 16px;
  font-weight: 700;
  color: var(--fg-2);
}

.mf-info {
  flex: 1;
  min-width: 0;
}
.mf-name {
  font-size: 15px;
  color: var(--fg);
  margin-bottom: 4px;
}
.mf-sub {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--fg-dim);
}
.mf-live {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex: none;
}
/* 状态文字比平台名更亮一点，突出「直播中 / 未开播」 */
.mf-status {
  color: var(--fg-2);
}
.mf-sep {
  opacity: 0.5;
}

/* 行末取消关注：44×44 热区，图标按钮，默认低调、按下变强调色 */
.mf-unfollow {
  flex: none;
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 50%;
  color: var(--fg-dim);
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}
.mf-unfollow:hover {
  color: var(--accent);
  background: var(--chip);
}
.mf-unfollow:active {
  color: var(--accent);
  background: var(--chip-hover);
}
</style>
