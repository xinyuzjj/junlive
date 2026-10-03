<script setup lang="ts">
/**
 * 移动端关注页 —— 不复用 PC 版 src/views/Follow.vue。
 *
 * PC 版是多列网格 + 小按钮，靠鼠标操作；手机上那种密度点不准、也扫不动。
 * 移动版重做成「单列大行」：
 *   - 每行至少 56px 高（≥44px 触控热区），整行可点直接进直播间
 *   - 只保留一眼要看的信息：头像 / 主播名 / 平台 / 在线状态点 / 进入箭头
 *   - 删除走左滑手势（手机上比点小 ✕ 更顺手），滑够 60px 再弹一次确认
 * 数据层完全复用 store：关注记录、在线状态、导出/导入逻辑都和 PC 版同源，
 * 所以两端数据天然一致。
 */
import { computed, onMounted, ref } from "vue";
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

/** 在播最前、未知中间、未开播最后（与 PC 版一致） */
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
 * 在线状态点的颜色（需求：绿=直播中 / 灰=未开播 / 黄=未知）。
 * 未知包含 UNKNOWN 和还没刷到的 undefined —— 不能误当成「在播」。
 */
function liveColor(f: FollowItem): string {
  if (f.live === "LIVE") return "var(--ok, #00c853)";
  if (f.live === "OFFLINE") return "var(--fg-mute, #c0c4cc)";
  return "#f5a623";
}

function open(platform: string, roomId: string) {
  router.push(`/room/${platform}/${encodeURIComponent(roomId)}`);
}

/* ---------------- 左滑删除 ---------------- */
/**
 * 用 touchstart/touchend 记起止点，算水平位移。
 * 加两道闸：左滑 > 60px，且水平位移明显大于垂直（>1.5 倍）——
 * 后者是为了不跟列表上下滚动抢手势，否则用户一滚就容易误删。
 * 删前 window.confirm 再确认一次，防手滑。
 */
const touchStart = ref<{ x: number; y: number } | null>(null);

function onTouchStart(e: TouchEvent) {
  const t = e.touches[0];
  if (!t) return;
  touchStart.value = { x: t.clientX, y: t.clientY };
}

function onTouchEnd(e: TouchEvent, f: FollowItem) {
  const s = touchStart.value;
  touchStart.value = null;
  if (!s) return;
  const t = e.changedTouches[0];
  if (!t) return;
  const dx = t.clientX - s.x;
  const dy = t.clientY - s.y;
  if (dx < -60 && Math.abs(dx) > Math.abs(dy) * 1.5) {
    removeOne(f);
  }
}

function removeOne(f: FollowItem) {
  if (!window.confirm(`取消关注「${f.streamer || f.room_id}」？`)) return;
  // toggleFollow 对已关注项就是删除；这里只需要 platform / room_id，
  // 其余字段它不看，所以用类型断言最小化构造（与 PC 版调用方式一致）。
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

    <!-- 单列列表 -->
    <div v-else class="mf-list">
      <div
        v-for="f in sorted"
        :key="f.platform + '/' + f.room_id"
        class="mf-row"
        :class="{ off: f.live === 'OFFLINE' }"
        @click="open(f.platform, f.room_id)"
        @touchstart.passive="onTouchStart"
        @touchend.passive="onTouchEnd($event, f)"
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
            {{ store.platformName(f.platform) }}
          </div>
        </div>

        <span class="mf-arrow">›</span>
      </div>
    </div>

    <div v-if="store.follows.length" class="mf-hint">左滑一行可取消关注</div>
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
  padding: 0 16px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
  transition: background 0.12s;
  /* 禁掉横向文字选择，避免左滑时选中文字 */
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

.mf-arrow {
  flex: none;
  font-size: 20px;
  color: var(--fg-mute, #c0c4cc);
  padding-right: 2px;
}

.mf-hint {
  text-align: center;
  font-size: 12px;
  color: var(--fg-dim);
  padding: 16px 0 4px;
}
</style>
