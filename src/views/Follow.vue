<script setup lang="ts">
import { useRouter } from "vue-router";
import { computed, onMounted } from "vue";
import { store, liveRank } from "../store";
import { avatarUrl, getRoom } from "../api";

const router = useRouter();

/**
 * 关注列表状态。
 *
 * 和首页侧栏用同一套：状态存在关注记录里（store.follows[].live，持久化），
 * 所以一进页面顺序就是对的，这里只负责后台刷新。
 */
async function refreshStatus() {
  const list = [...store.follows];
  let idx = 0;
  const workers = Array.from(
    { length: Math.min(2, list.length) },
    async () => {
      while (idx < list.length) {
        const f = list[idx++];
        try {
          const d = await getRoom(f.platform, f.room_id);
          store.setFollowLive(
            f.platform,
            f.room_id,
            d.live ? "LIVE" : "OFFLINE",
          );
          store.setFollowInfo(f.platform, f.room_id, d.streamer, d.avatar);
        } catch {
          store.setFollowLive(f.platform, f.room_id, "UNKNOWN");
        }
      }
    },
  );
  await Promise.all(workers);
}

onMounted(refreshStatus);

/** 展示用头像（关注时可能是空的，store 里会被实时刷新覆盖） */
function showAvatar(f: { platform: string; avatar?: string }) {
  return avatarUrl(f.platform, f.avatar);
}

/** 未开播 */
function isOff(f: { live?: string }) {
  return f.live === "OFFLINE";
}

/** 在播最前、未知中间、未开播最后 */
const sorted = computed(() =>
  [...store.follows].sort((a, b) => liveRank(a.live) - liveRank(b.live)),
);

function open(platform: string, roomId: string) {
  router.push(`/room/${platform}/${encodeURIComponent(roomId)}`);
}

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
  <div class="follow">
    <div class="head">
      <div class="title">我的关注（{{ store.follows.length }}）</div>
      <div class="actions">
        <button @click="exportJson">导出 JSON</button>
        <button @click="importJson">导入 JSON</button>
      </div>
    </div>

    <div v-if="!store.follows.length" class="empty">
      还没有关注任何主播 —— 在播放页点「☆ 关注」即可添加
    </div>

    <div v-else class="grid">
      <div
        v-for="f in sorted"
        :key="f.platform + f.room_id"
        class="item"
        :class="{ off: isOff(f) }"
        @click="open(f.platform, f.room_id)"
      >
        <div class="avatar">
          <img
            v-if="showAvatar(f)"
            :src="showAvatar(f)"
            referrerpolicy="no-referrer"
            @error="($event.target as HTMLImageElement).style.display = 'none'"
          />
          <span v-else>{{ (f.streamer || "?").slice(0, 1) }}</span>
        </div>
        <div class="info">
          <div class="name ellipsis">{{ f.streamer || f.room_id }}</div>
          <div class="sub">
            <span class="dot" :style="{ background: store.platformColor(f.platform) }"></span>
            {{ store.platformName(f.platform) }} · {{ f.room_id }}
          </div>
        </div>
        <span v-if="isOff(f)" class="badge-off">未开播</span>
        <button class="del" @click.stop="store.toggleFollow({ platform: f.platform, room_id: f.room_id } as any)">
          ✕
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.follow {
  height: 100%;
  overflow-y: auto;
  padding: 16px;
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}
.title {
  font-weight: 600;
  font-size: 15px;
}
.actions {
  display: flex;
  gap: 8px;
}
.empty {
  color: var(--fg-dim);
  text-align: center;
  padding: 60px 0;
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 12px;
}
.item {
  display: flex;
  align-items: center;
  gap: 10px;
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 10px;
  cursor: pointer;
}
.item:hover {
  border-color: var(--accent);
}
/* 未开播：整条压暗 */
.item.off {
  opacity: 0.55;
  background: var(--bg-3);
}
.item.off .avatar {
  filter: grayscale(1);
}
.item.off:hover {
  opacity: 0.8;
}
.badge-off {
  flex: none;
  font-size: 11px;
  color: var(--fg-dim);
  background: var(--chip);
  border-radius: 4px;
  padding: 2px 6px;
}
.avatar {
  width: 42px;
  height: 42px;
  flex: none;
  border-radius: 50%;
  background: var(--bg-3);
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  font-weight: 600;
}
.avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.info {
  flex: 1;
  min-width: 0;
}
.name {
  font-size: 13px;
  margin-bottom: 3px;
}
.sub {
  font-size: 11px;
  color: var(--fg-dim);
  display: flex;
  align-items: center;
  gap: 5px;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
}
.del {
  padding: 2px 8px;
  font-size: 12px;
  background: transparent;
  border-color: transparent;
  color: var(--fg-dim);
}
.del:hover {
  color: var(--danger);
  border-color: var(--danger);
}
</style>
