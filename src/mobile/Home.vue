<script setup lang="ts">
/**
 * 移动端首页 —— 独立实现，不复用 PC 的 Home.vue。
 *
 * 布局对齐 Simple Live（见调研 B 节）：
 *   顶部   只放「平台」横滑条（B 站 / 斗鱼 / 虎牙 / 抖音…），真分类搬到「分类」tab；
 *           Simple Live 首页 AppBar 就是平台 TabBar，分类不在首页（调研 A2）。
 *   卡片   列数 = floor(宽 / 200) 且**最少 2 列**（调研 B1）；封面**固定高 110、宽撑满、
 *           object-fit:cover**（调研 B3，不是 16:9）；卡片只留 封面 + 在线人数 + 标题
 *           + 主播名（调研 B2），砍掉平台圆点等装饰。
 *   交互   滚到底自动加载、下拉刷新（沿用原实现）。
 *   搜索   监听 route.query.q：有 q 调 searchRooms，清空恢复分类列表 ——
 *           旧版漏了这条分支，导致移动端关键词搜索完全无效（审查报告「致命」项）。
 */
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { getCategories, getRooms, searchRooms, thumbUrl, type Room } from "../api";
import { store } from "../store";

const route = useRoute();
const router = useRouter();

const rooms = ref<Room[]>([]);
const page = ref(1);
const loading = ref(false);
const done = ref(false);
const err = ref("");

/** 当前搜索关键词；来自 route.query.q（与外壳 Shell.doSearch 的约定一致） */
const keyword = ref("");
/** 首页默认分区：后端 get_rooms 必须要一个分类 id，取该平台第一个分类 */
const defaultCat = ref("");

async function loadCats() {
  try {
    const cs = await getCategories(store.current);
    defaultCat.value = cs[0]?.id ?? "";
  } catch (e) {
    err.value = String(e);
  }
}

async function loadRooms(reset = false) {
  if (loading.value) return;
  loading.value = true;
  err.value = "";
  if (reset) {
    page.value = 1;
    rooms.value = [];
    done.value = false;
  }
  try {
    // 有关键词走搜索，否则走默认分类浏览
    const r = keyword.value
      ? await searchRooms(store.current, keyword.value, page.value)
      : await getRooms(store.current, defaultCat.value, page.value);
    const list = r.rooms ?? [];
    rooms.value = reset ? list : [...rooms.value, ...list];
    if (!list.length) done.value = true;
    else page.value += 1;
  } catch (e) {
    err.value = String(e);
    done.value = true;
  } finally {
    loading.value = false;
  }
}

/** 切平台：清掉搜索态（关键词绑平台），重拉分类并回到列表 */
async function onPlatform(id: string) {
  if (id === store.current) return;
  store.setPlatform(id);
  // 先同步 keyword，再清路由，避免 route 监听器重复请求
  keyword.value = "";
  if (route.query.q) router.replace({ path: "/" });
  await loadCats();
  await loadRooms(true);
}

function clearSearch() {
  keyword.value = "";
  if (route.query.q) router.replace({ path: "/" });
  void loadRooms(true);
}

function onScroll(e: Event) {
  const el = e.target as HTMLElement;
  if (el.scrollHeight - el.scrollTop - el.clientHeight < 300) {
    if (!done.value && !loading.value) void loadRooms();
  }
}

/* ---------------- 骨架屏 ----------------
 * 只覆盖「首屏还没拿到任何数据」这一种情况：翻页时列表已在屏上，抽掉内容反而倒退。
 */
const showSkeleton = computed(() => loading.value && rooms.value.length === 0);

/* ---------------- 下拉刷新 ----------------
 * 手机上「到顶再往下拉 = 刷新」是肌肉记忆。关键点：只在 scrollTop<=0 时才接管触摸；
 * 手指一改成上滑立刻放弃接管、交还浏览器滚动 —— 正常滚动完全不受影响。
 */
const PULL_TRIGGER = 60;
const pullY = ref(0);
const pulling = ref(false);
const refreshing = ref(false);
let touchStartY = 0;

function onTouchStart(e: TouchEvent) {
  const el = e.currentTarget as HTMLElement;
  if (el.scrollTop > 0 || refreshing.value) {
    touchStartY = 0;
    return;
  }
  touchStartY = e.touches[0]?.clientY ?? 0;
}

function onTouchMove(e: TouchEvent) {
  if (!touchStartY || refreshing.value) return;
  const dy = (e.touches[0]?.clientY ?? 0) - touchStartY;
  if (dy > 0) {
    pulling.value = true;
    // 阻尼 0.5：拉起来带点阻力，不至于一碰就弹到底
    pullY.value = Math.min(80, dy * 0.5);
  } else {
    pulling.value = false;
    pullY.value = 0;
  }
}

async function onTouchEnd() {
  if (!touchStartY && !pulling.value) return;
  const hit = pulling.value && pullY.value >= PULL_TRIGGER;
  touchStartY = 0;
  pulling.value = false;
  pullY.value = 0;
  if (!hit) return;
  refreshing.value = true;
  try {
    await loadRooms(true);
  } finally {
    refreshing.value = false;
  }
}

function open(r: Room) {
  router.push(`/room/${r.platform || store.current}/${encodeURIComponent(r.room_id)}`);
}

/**
 * 封面加载失败兜底：把破图藏起来，露出卡片底色，避免手机上出现一排破图图标。
 * 不做回退原图（原图更费流量，且多半一样挂）。
 */
function coverErr(e: Event) {
  (e.target as HTMLImageElement).style.visibility = "hidden";
}

onMounted(async () => {
  keyword.value = (route.query.q as string) || "";
  await loadCats();
  await loadRooms(true);
});

// 外壳搜索框把关键词塞进 ?q=：q 变了就重搜；清空（q 为空）则恢复分类列表
watch(
  () => route.query.q,
  (q) => {
    const nq = (q as string) || "";
    if (nq === keyword.value) return; // 切平台时已同步，避免重复请求
    keyword.value = nq;
    void loadRooms(true);
  },
);
</script>

<template>
  <div class="mh">
    <!-- 顶部：只放平台（对齐 Simple Live 首页 AppBar 的平台 TabBar） -->
    <div class="mh-plats">
      <button
        v-for="p in store.platforms"
        :key="p.id"
        class="mh-plat"
        :class="{ on: p.id === store.current }"
        :style="p.id === store.current ? { color: p.color, borderColor: p.color } : {}"
        @click="onPlatform(p.id)"
      >
        <span class="mh-plat-dot" :style="{ background: p.color }"></span>
        {{ p.name }}
      </button>
      <span v-if="!store.platforms.length" class="mh-plats-empty">正在加载平台…</span>
    </div>

    <!-- 搜索态提示条：让用户知道当前在看搜索结果，并可一键清除 -->
    <div v-if="keyword" class="mh-sbar">
      <span class="mh-sbar-t">搜索「{{ keyword }}」</span>
      <button class="mh-sbar-x" @click="clearSearch">清除</button>
    </div>

    <div
      class="mh-scroll"
      @scroll="onScroll"
      @touchstart="onTouchStart"
      @touchmove="onTouchMove"
      @touchend="onTouchEnd"
      @touchcancel="onTouchEnd"
    >
      <!-- 下拉刷新提示条：高度跟着手指走，正在刷新时顶到固定高度 -->
      <div
        class="mh-pull"
        :class="{ on: pulling || refreshing }"
        :style="{ height: (refreshing ? 44 : pullY) + 'px' }"
      >
        <span class="mh-pull-i" :class="{ spin: refreshing }">↻</span>
        <span>{{ refreshing ? "刷新中…" : pullY >= PULL_TRIGGER ? "松手刷新" : "下拉刷新" }}</span>
      </div>

      <div v-if="err && rooms.length" class="mh-warn">{{ err }}</div>

      <!-- 骨架屏：首屏没数据时用灰块占位 -->
      <div v-if="showSkeleton" class="mh-grid">
        <div v-for="i in 6" :key="i" class="mh-card">
          <div class="mh-thumb sk"></div>
          <div class="sk-line"></div>
          <div class="sk-line short"></div>
        </div>
      </div>

      <div v-else-if="rooms.length" class="mh-grid">
        <button
          v-for="r in rooms"
          :key="r.platform + r.room_id"
          class="mh-card"
          @click="open(r)"
        >
          <div class="mh-thumb">
            <!-- 封面走 thumbUrl：只拉 400px 缩略图，省流量省内存 -->
            <img
              :src="thumbUrl(store.current, r.cover, 400)"
              referrerpolicy="no-referrer"
              loading="lazy"
              alt=""
              @error="coverErr"
            />
            <!--
              在线人数直接显示后端返回的字符串（后端 fmt_num 已格式化成「1.2万」）。
              旧版又 Number("1.2万") 一次 → NaN → 空串，留下一个无字黑胶囊（审查「致命」项）。
              后端没给就整个胶囊不渲染。
            -->
            <span v-if="r.online" class="mh-hot">{{ r.online }}</span>
          </div>
          <!-- 标题只留一行（不是两行） -->
          <div class="mh-title">{{ r.title || "（无标题）" }}</div>
          <div class="mh-streamer">{{ r.streamer || "-" }}</div>
        </button>
      </div>

      <!-- 空态：区分「加载失败」「搜索无结果」「分区空」 -->
      <div v-else-if="!loading" class="mh-empty">
        <template v-if="err">
          <div class="mh-empty-t">{{ err }}</div>
          <button class="mh-clear" @click="loadRooms(true)">重试</button>
        </template>
        <template v-else-if="keyword">
          <div class="mh-empty-t">没有搜到「{{ keyword }}」相关的直播间</div>
          <button class="mh-clear" @click="clearSearch">清除搜索</button>
        </template>
        <template v-else>
          <div class="mh-empty-t">这里暂时没有在播的直播间</div>
        </template>
      </div>

      <div v-if="loading && rooms.length" class="mh-more">加载中…</div>
      <div v-else-if="done && rooms.length" class="mh-more">没有更多了</div>
    </div>
  </div>
</template>

<style scoped>
.mh {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

/* ---------- 平台横滑条 ----------
   对齐 Simple Live：首页顶部只有平台，没有二级分区（分区在「分类」tab）。
   每个 chip 高 44，满足最小触控面积。 */
.mh-plats {
  display: flex;
  align-items: center;
  gap: 8px;
  overflow-x: auto;
  flex-shrink: 0;
  scrollbar-width: none;
  padding: 8px 12px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
}
.mh-plats::-webkit-scrollbar {
  display: none;
}
.mh-plat {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 44px;
  padding: 0 16px;
  border-radius: 999px;
  border: 1px solid transparent;
  background: var(--chip);
  color: var(--fg-2);
  font-size: 14px;
  white-space: nowrap;
}
.mh-plat.on {
  background: var(--brand-soft);
  font-weight: 600;
}
.mh-plat-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.mh-plats-empty {
  font-size: 13px;
  color: var(--fg-dim);
  padding: 0 4px;
}

/* ---------- 搜索态提示条 ---------- */
.mh-sbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
  height: 56px;
  padding: 0 12px 0 14px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
}
.mh-sbar-t {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  color: var(--fg-2);
}
.mh-sbar-x {
  flex: none;
  height: 44px;
  padding: 0 16px;
  border-radius: 999px;
  border: 1px solid var(--border-2);
  background: var(--chip);
  color: var(--fg);
  font-size: 13px;
}

.mh-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  /* 关掉浏览器/WebView 自带的整页下拉刷新，否则会和我们的手势打架 */
  overscroll-behavior-y: contain;
  padding: 8px 8px 16px;
}

/* 下拉刷新提示条：默认被 height:0 压扁，靠内联 height 撑开 */
.mh-pull {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 0;
  overflow: hidden;
  color: var(--fg-dim);
  font-size: 12px;
  transition: height 0.15s;
}
.mh-pull.on {
  transition: none;
}
.mh-pull-i {
  display: inline-block;
  font-size: 14px;
}
.mh-pull-i.spin {
  animation: mh-spin 0.8s linear infinite;
}
@keyframes mh-spin {
  to {
    transform: rotate(360deg);
  }
}

/* ---------- 卡片网格 ----------
   列数 = floor(宽/200) 最少 2 列：用 auto-fill + minmax(170px, 1fr) 近似，
   170 保证 390px 宽下仍是 2 列、不会塌成 1 列。间距 8px（调研 B1）。 */
.mh-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
  gap: 8px;
}
.mh-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  border: 0; /* 无边框无阴影：Simple Live 的卡片风格 */
  background: none;
  padding: 0;
  text-align: left;
  color: var(--fg);
}
/* 封面：固定高 110、宽 100%、object-fit:cover（不是 16:9）—— 调研 B3 */
.mh-thumb {
  position: relative;
  width: 100%;
  height: 110px;
  border-radius: 6px;
  overflow: hidden;
  background: var(--chip);
}
.mh-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
/* 在线人数角标：用主题变量而非写死的黑色，深浅色下都读得清 */
.mh-hot {
  position: absolute;
  right: 4px;
  top: 4px;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--fg);
  color: var(--bg);
  font-size: 11px;
  line-height: 1.5;
}
/* 标题一行截断 */
.mh-title {
  font-size: 13px;
  line-height: 1.4;
  color: var(--fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mh-streamer {
  font-size: 12px;
  color: var(--fg-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.mh-warn {
  padding: 10px 12px;
  color: var(--accent);
  font-size: 13px;
}
.mh-more {
  padding: 18px;
  text-align: center;
  color: var(--fg-dim);
  font-size: 13px;
}

/* ---------- 空态 ---------- */
.mh-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 60px 20px;
  text-align: center;
}
.mh-empty-t {
  font-size: 14px;
  color: var(--fg-2);
  line-height: 1.6;
}
.mh-clear {
  height: 44px;
  padding: 0 20px;
  border-radius: 999px;
  border: 1px solid var(--border-2);
  background: var(--chip);
  color: var(--fg);
  font-size: 14px;
}

/* ---------- 骨架屏 ----------
   缩略图占位同样固定高 110，与真实卡片一一对齐，数据到位时不跳版。 */
.mh-thumb.sk,
.sk-line {
  background: linear-gradient(
    100deg,
    var(--chip) 30%,
    var(--chip-hover) 50%,
    var(--chip) 70%
  );
  background-size: 200% 100%;
  animation: mh-shimmer 1.3s ease-in-out infinite;
}
@keyframes mh-shimmer {
  to {
    background-position: -200% 0;
  }
}
.sk-line {
  height: 12px;
  border-radius: 4px;
}
.sk-line.short {
  width: 62%;
}
</style>
