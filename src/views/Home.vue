<script setup lang="ts">
import { avatarUrl } from "../api";
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  getCategories,
  getRoom,
  getRooms,
  searchRooms,
  type Category,
  type Room,
} from "../api";
import { store, liveRank, type FollowItem } from "../store";

const route = useRoute();
const router = useRouter();

/**
 * 关注列表的在线状态。
 *
 * 参考 DTV：状态**存在关注记录里**（`store.follows[].live`），一进页面顺序就是对的，
 * 后台再并发刷新覆盖。如果只在进页面时临时拉一次、拉完就丢，
 * 结果回来之前顺序是乱的（这就是之前「未开播的还在上面」的原因）。
 */
const avatarErr = ref<Record<string, boolean>>({});
const favKey = (f: { platform: string; room_id: string }) =>
  `${f.platform}:${f.room_id}`;

/** 在播最前、未知中间、未开播最后。sort 稳定，同档内保持原顺序 */
const sortedFollows = computed(() =>
  [...store.follows].sort((a, b) => liveRank(a.live) - liveRank(b.live)),
);

/** 和 DTV 一样用并发 2，别把平台接口打爆 */
const REFRESH_CONCURRENCY = 2;

async function loadFavStatus() {
  const list = [...store.follows];
  let idx = 0;
  const workers = Array.from(
    { length: Math.min(REFRESH_CONCURRENCY, list.length) },
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
          // 拉不到时不能谎报「在播」，标成未知（DTV 的做法）
          store.setFollowLive(f.platform, f.room_id, "UNKNOWN");
        }
      }
    },
  );
  await Promise.all(workers);
}

const categories = ref<Category[]>([]);
const activeParent = ref("");
const activeSub = ref("");
const rooms = ref<Room[]>([]);
const page = ref(1);
const loading = ref(false);
const loadingMore = ref(false);
const error = ref("");
const keyword = ref("");
const isSearch = ref(false);
const expandCats = ref(false);

const parents = computed(() => categories.value);

const shownParents = computed(() =>
  expandCats.value ? parents.value : parents.value.slice(0, 18),
);

const subs = computed(() => {
  const p = parents.value.find((c) => c.id === activeParent.value);
  if (!p || !p.children.length) return [];
  return [{ id: p.id, name: "全部" }, ...p.children];
});

const currentCatId = computed(() => activeSub.value || activeParent.value);

const title = computed(() =>
  isSearch.value ? `「${keyword.value}」` : store.platformName(store.current),
);

function firstCat(cats: Category[]): string {
  return cats.length ? cats[0].id : "";
}

async function loadCategories() {
  categories.value = [];
  activeParent.value = "";
  activeSub.value = "";
  error.value = "";
  try {
    const cats = await getCategories(store.current);
    categories.value = cats;
    activeParent.value = firstCat(cats);
    const p = cats.find((c) => c.id === activeParent.value);
    activeSub.value = p && p.children.length ? p.children[0].id : "";
  } catch (e) {
    error.value = `获取分类失败：${e}`;
  }
}

async function loadRooms(reset = true) {
  if (reset) {
    page.value = 1;
    rooms.value = [];
  }
  if (isSearch.value) {
    if (!keyword.value) return;
    loading.value = reset;
    try {
      const r = await searchRooms(store.current, keyword.value, page.value);
      rooms.value = reset ? r.rooms : [...rooms.value, ...r.rooms];
      error.value = rooms.value.length ? "" : `没有搜到「${keyword.value}」相关的直播间`;
    } catch (e) {
      error.value = `${e}`;
    } finally {
      loading.value = false;
    }
    return;
  }

  if (!currentCatId.value) {
    rooms.value = [];
    return;
  }
  loading.value = reset;
  try {
    const r = await getRooms(store.current, currentCatId.value, page.value);
    rooms.value = reset ? r.rooms : [...rooms.value, ...r.rooms];
    error.value = rooms.value.length ? "" : `${store.platformName(store.current)} 暂时没有返回数据`;
  } catch (e) {
    error.value = `${e}`;
  } finally {
    loading.value = false;
  }
}

async function more() {
  loadingMore.value = true;
  page.value += 1;
  await loadRooms(false);
  loadingMore.value = false;
}

function refresh() {
  loadRooms(true);
}

async function init() {
  const kw = (route.query.kw as string) || "";
  const p = (route.query.p as string) || "";
  if (p && p !== store.current) store.setPlatform(p);
  isSearch.value = !!kw;
  keyword.value = kw;
  expandCats.value = false;
  await loadCategories();
  await loadRooms(true);
}

function pickParent(id: string) {
  activeParent.value = id;
  const p = categories.value.find((c) => c.id === id);
  activeSub.value = p && p.children.length ? p.children[0].id : "";
  isSearch.value = false;
  keyword.value = "";
  loadRooms(true);
}

function pickSub(id: string) {
  activeSub.value = id;
  isSearch.value = false;
  keyword.value = "";
  loadRooms(true);
}

function openRoom(r: Room) {
  router.push(`/room/${r.platform}/${encodeURIComponent(r.room_id)}`);
}

function openFav(f: FollowItem) {
  router.push(`/room/${f.platform}/${encodeURIComponent(f.room_id)}`);
}

onMounted(() => {
  init();
  loadFavStatus();
});
watch(() => [route.query.t, store.current], init);
// 关注列表变了（新增/删除）就重新查一次在线状态
watch(() => store.follows.length, loadFavStatus);
</script>

<template>
  <div class="home">
    <!-- 左侧：我的关注 -->
    <aside class="side">
      <div class="side-head">
        <span>我的关注</span>
        <div class="side-actions">
          <span class="side-count">{{ store.follows.length }}</span>
          <button
            class="side-follow"
            title="管理关注（删除 / 导入导出）"
            @click="router.push('/follow')"
          >
            关注
          </button>
        </div>
      </div>
      <div class="side-list">
        <div
          v-for="f in sortedFollows"
          :key="f.platform + f.room_id"
          class="fav"
          :class="{ 'fav-off': f.live === 'OFFLINE' }"
          @click="openFav(f)"
        >
          <div class="fav-avatar">
            <!-- 只渲染一个：图片加载失败才退回首字母。
                 以前两个都渲染，span 有 z-index 会永远盖在图片上面。 -->
            <img
              v-if="avatarUrl(f.platform, f.avatar) && !avatarErr[favKey(f)]"
              :src="avatarUrl(f.platform, f.avatar)"
              referrerpolicy="no-referrer"
              @error="avatarErr[favKey(f)] = true"
            />
            <span v-else>{{ (f.streamer || "?").slice(0, 1) }}</span>
          </div>
          <div class="fav-txt">
            <div class="fav-name ellipsis">{{ f.streamer || f.room_id }}</div>
            <div class="fav-sub ellipsis">{{ store.platformName(f.platform) }}</div>
          </div>
          <!-- 绿=直播中，灰=未开播，暗黄=状态未知 -->
          <span
            class="live-dot"
            :class="{
              on: f.live === 'LIVE',
              unknown: f.live === 'UNKNOWN' || !f.live,
            }"
            :title="
              f.live === 'LIVE'
                ? '直播中'
                : f.live === 'OFFLINE'
                  ? '未开播'
                  : '状态未知'
            "
          ></span>
        </div>
        <div v-if="!store.follows.length" class="side-empty">
          还没有关注的主播<br />
          <span class="tiny">在播放页点「关注」就会出现在这里</span>
        </div>
      </div>
    </aside>

    <!-- 右侧 -->
    <section class="main">
      <div class="cats">
        <div class="cat-row">
          <button
            v-for="c in shownParents"
            :key="c.id"
            class="pill"
            :class="{ active: activeParent === c.id && !isSearch }"
            @click="pickParent(c.id)"
          >
            {{ c.name }}
          </button>
        </div>
        <button
          v-if="parents.length > 18"
          class="expand"
          @click="expandCats = !expandCats"
        >
          {{ expandCats ? "收起 ⌃" : "展开 ⌄" }}
        </button>

        <div v-if="subs.length" class="sub-row">
          <button
            v-for="s in subs"
            :key="s.id"
            class="pill small"
            :class="{ active: activeSub === s.id && !isSearch }"
            @click="pickSub(s.id)"
          >
            {{ s.name }}
          </button>
        </div>
      </div>

      <div class="head">
        <div class="head-left">
          <span class="head-title">{{ title }}</span>
          <span v-if="rooms.length" class="count">{{ rooms.length }} 个直播间</span>
        </div>
        <button class="ghost round" :disabled="loading" @click="refresh">
          <span :class="{ spin: loading }">↻</span> 刷新
        </button>
      </div>

      <div class="grid-wrap">
        <div v-if="loading" class="grid">
          <div v-for="i in 12" :key="i" class="card sk">
            <div class="thumb sk-block"></div>
            <div class="row">
              <div class="avatar sk-block"></div>
              <div class="txt">
                <div class="sk-line" style="width: 90%"></div>
                <div class="sk-line" style="width: 55%"></div>
              </div>
            </div>
          </div>
        </div>

        <div v-else-if="!rooms.length" class="empty">
          <div class="empty-icon">📺</div>
          <div class="empty-title">{{ error || "这里还没有直播" }}</div>
          <div class="empty-sub">
            {{ isSearch ? "换个关键词，或直接粘贴直播间链接" : "换个分类试试，或点右上角刷新" }}
          </div>
          <button class="primary" @click="refresh">重新加载</button>
        </div>

        <div v-else class="grid">
          <div
            v-for="r in rooms"
            :key="r.platform + r.room_id"
            class="card"
            @click="openRoom(r)"
          >
            <div class="thumb">
              <img
                v-if="r.cover"
                :src="r.cover"
                loading="lazy"
                referrerpolicy="no-referrer"
                @error="($event.target as HTMLImageElement).style.display = 'none'"
              />
              <div class="ph">{{ (r.streamer || "?").slice(0, 1) }}</div>
              <span v-if="r.online" class="hot">👁 {{ r.online }}</span>
              <span v-if="!r.live" class="off">未开播</span>
              <span class="play-hint">▶</span>
            </div>
            <div class="row">
              <div class="avatar">
                <img
                  v-if="avatarUrl(r.platform, r.avatar)"
                  :src="avatarUrl(r.platform, r.avatar)"
                  referrerpolicy="no-referrer"
                  @error="($event.target as HTMLImageElement).style.display = 'none'"
                />
                <span>{{ (r.streamer || "?").slice(0, 1) }}</span>
              </div>
              <div class="txt">
                <div class="title ellipsis" :title="r.title">
                  {{ r.title || "（无标题）" }}
                </div>
                <div class="sub ellipsis">{{ r.streamer || "-" }}</div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div v-if="rooms.length && !isSearch" class="foot">
        <button class="ghost round" :disabled="loadingMore" @click="more">
          {{ loadingMore ? "加载中…" : "加载更多" }}
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.home {
  display: flex;
  height: 100%;
  overflow: hidden;
  background: var(--bg);
}

/* ==================== 移动端 ====================
   手机上隐藏左侧关注栏 —— 关注已经是底部标签栏里独立的一页，
   再挤在首页左侧会把卡片区压到只剩一半宽（实测 390px 时卡片只剩一列还溢出）。
   卡片改成两列、间距放大，触控更舒服。 */
@media (max-width: 820px) {
  .home .side {
    display: none;
  }
  .home .grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    padding: 10px;
  }
}

/* ---------------- 左侧关注 ---------------- */
.side {
  width: 232px;
  flex: none;
  background: var(--panel);
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border);
}
.side-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 16px 10px;
  font-size: 13px;
  font-weight: 600;
  color: var(--fg);
  flex: none;
}
.side-count {
  font-size: 12px;
  font-weight: 400;
  color: var(--fg-dim);
  background: var(--chip);
  border-radius: 999px;
  padding: 1px 8px;
}
.side-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
/* 关注管理入口（原来在顶栏，挪到侧栏） */
.side-follow {
  font-size: 11px;
  font-weight: 400;
  height: 22px;
  padding: 0 9px;
  border-radius: 999px;
  background: var(--chip);
  border: 1px solid var(--border-2);
  color: var(--fg-2);
  cursor: pointer;
}
.side-follow:hover {
  background: var(--brand-soft);
  color: var(--brand);
  border-color: var(--brand);
}
.side-list {
  flex: 1;
  overflow-y: auto;
  padding: 0 8px 12px;
}
.fav {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 8px;
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.12s;
}
.fav:hover {
  background: var(--chip);
}
/* 未开播：整条压暗 + 头像灰度。
   注意类名不能叫 `.off` —— Home.vue 里 `.off` 是房间卡片上的「未开播」角标
   （绝对定位），撞名会让侧栏条目全部叠在同一个坐标上。 */
.fav-off {
  opacity: 0.5;
}
.fav-off .fav-avatar {
  filter: grayscale(1);
}
.fav-off:hover {
  opacity: 0.75;
}
/* 在线状态点：绿=直播中，灰=未开播 */
.live-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex: none;
  background: var(--fg-mute);
}
.live-dot.on {
  background: var(--green);
  box-shadow: 0 0 0 3px rgba(0, 200, 83, 0.18);
}
/* 状态未知（还没查到 / 拉失败） */
.live-dot.unknown {
  background: var(--fg-mute);
  opacity: 0.4;
}
.fav-avatar {
  width: 40px;
  height: 40px;
  flex: none;
  border-radius: 50%;
  overflow: hidden;
  background: var(--chip);
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 600;
  color: var(--fg-mute);
}
.fav-avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  position: absolute;
  /* 图片压在最上层：以前 span 有 z-index:0、img 是 auto，
     结果首字母永远盖在头像上面（即使图片已经加载出来了）。 */
  z-index: 1;
}
.fav-avatar {
  position: relative;
}
.fav-avatar span {
  position: relative;
  z-index: 0;
}
.fav-txt {
  flex: 1;
  min-width: 0;
}
.fav-name {
  font-size: 14px;
  color: var(--fg);
  line-height: 1.4;
}
.fav-sub {
  font-size: 12px;
  color: var(--fg-dim);
  line-height: 1.5;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex: none;
}
.side-empty {
  padding: 40px 12px;
  text-align: center;
  color: var(--fg-mute);
  font-size: 12.5px;
  line-height: 2;
}
.tiny {
  font-size: 11.5px;
  color: #d5d8de;
}

/* ---------------- 右侧 ---------------- */
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.cats {
  flex: none;
  padding: 16px 20px 12px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
}
.cat-row,
.sub-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.sub-row {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px dashed var(--border-2);
  flex-wrap: nowrap;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: thin;
  padding-bottom: 2px;
}
.sub-row .pill {
  flex: none;
}
.sub-row::-webkit-scrollbar {
  height: 6px;
}
.pill {
  height: 32px;
  padding: 0 16px;
  border-radius: 999px;
  background: var(--chip);
  border: 1px solid transparent;
  color: var(--fg-2);
  font-size: 13px;
  line-height: 1;
}
.pill:hover {
  background: var(--chip-hover);
  color: var(--fg);
}
.pill.small {
  height: 28px;
  padding: 0 14px;
  font-size: 12.5px;
}
.pill.active {
  background: var(--brand-soft);
  border-color: var(--brand);
  color: var(--brand);
  font-weight: 600;
}
.expand {
  display: block;
  margin: 8px auto 0;
  background: transparent;
  color: var(--fg-dim);
  font-size: 12.5px;
  padding: 2px 10px;
}
.expand:hover {
  color: var(--brand);
  background: transparent;
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 20px 6px;
  flex: none;
}
.head-left {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.plat-mini {
  font-size: 11px;
  color: #fff;
  padding: 2px 8px;
  border-radius: 4px;
  flex: none;
}
.head-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--fg);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.count {
  font-size: 12px;
  color: var(--fg-dim);
  flex: none;
}
.ghost.round {
  border-radius: 999px;
  height: 30px;
  flex: none;
}
.spin {
  display: inline-block;
  animation: spin 0.9s linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.grid-wrap {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 12px 20px 20px;
  scrollbar-gutter: stable;
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(196px, 1fr));
  gap: 16px 16px;
}

/* ---------------- 卡片 ---------------- */
.card {
  cursor: pointer;
  transition: transform 0.16s ease;
}
.card:hover {
  transform: translateY(-2px);
}
.thumb {
  position: relative;
  aspect-ratio: 16 / 9;
  border-radius: 12px;
  overflow: hidden;
  background: #eef0f3;
  transition: box-shadow 0.16s;
}
.card:hover .thumb {
  box-shadow: var(--shadow);
}
.thumb img {
  position: relative;
  z-index: 1;
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  transition: transform 0.25s ease;
}
.card:hover .thumb img {
  transform: scale(1.04);
}
.ph {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 26px;
  font-weight: 700;
  color: #d5d8de;
}
.hot {
  position: absolute;
  z-index: 2;
  right: 8px;
  top: 8px;
  font-size: 11.5px;
  color: #fff;
  background: rgba(0, 0, 0, 0.55);
  border-radius: 10px;
  padding: 2px 9px;
  line-height: 1.5;
}
.off {
  position: absolute;
  z-index: 2;
  left: 8px;
  top: 8px;
  font-size: 11.5px;
  color: #fff;
  background: rgba(0, 0, 0, 0.55);
  border-radius: 10px;
  padding: 2px 9px;
}
.play-hint {
  position: absolute;
  z-index: 3;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 30px;
  color: #fff;
  background: rgba(0, 0, 0, 0.28);
  opacity: 0;
  transition: opacity 0.18s;
}
.card:hover .play-hint {
  opacity: 1;
}

.row {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 10px 2px 0;
}
.avatar {
  width: 36px;
  height: 36px;
  flex: none;
  border-radius: 50%;
  overflow: hidden;
  background: var(--chip);
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 600;
  color: var(--fg-mute);
  font-size: 14px;
  position: relative;
}
.avatar img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.txt {
  flex: 1;
  min-width: 0;
}
.title {
  font-size: 14px;
  font-weight: 500;
  color: var(--fg);
  line-height: 1.4;
}
.sub {
  font-size: 12px;
  color: var(--fg-dim);
  margin-top: 2px;
}
.card:hover .title {
  color: var(--brand);
}

/* ---------------- 骨架屏 / 空状态 ---------------- */
.sk {
  pointer-events: none;
}
.sk-block,
.sk-line {
  background: linear-gradient(90deg, #eef0f3 25%, #f6f7f9 37%, #eef0f3 63%);
  background-size: 400% 100%;
  animation: shimmer 1.3s ease infinite;
}
.sk-line {
  height: 11px;
  border-radius: 4px;
  margin-bottom: 7px;
}
.sk .row {
  padding-top: 10px;
}
@keyframes shimmer {
  0% {
    background-position: 100% 50%;
  }
  100% {
    background-position: 0 50%;
  }
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 70px 20px;
  text-align: center;
}
.empty-icon {
  font-size: 40px;
  opacity: 0.45;
}
.empty-title {
  font-size: 14px;
  color: var(--fg);
}
.empty-sub {
  font-size: 12.5px;
  color: var(--fg-dim);
  max-width: 420px;
  line-height: 1.7;
}
.empty .primary {
  margin-top: 8px;
}

.foot {
  padding: 14px;
  text-align: center;
  flex: none;
}
</style>
