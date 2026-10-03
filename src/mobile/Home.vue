<script setup lang="ts">
/**
 * 移动端首页 —— 独立实现，不复用 PC 的 Home.vue。
 *
 * 与 PC 版的差别（不是缩放，是两种设计）：
 *   分类   一级/二级分区都改成横向滑动条，不用 PC 那种换行的标签墙
 *   卡片   固定两列；PC 是 auto-fill 五列
 *   关注   不在这里显示（关注是底部标签栏里独立的一页）
 *   分页   滚到底自动加载，不用 PC 的「加载更多」按钮
 */
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { getCategories, getRooms, thumbUrl, type Category, type Room } from "../api";
import { store } from "../store";

const route = useRoute();
const router = useRouter();

const cats = ref<Category[]>([]);
const sub = ref("");
const rooms = ref<Room[]>([]);
const page = ref(1);
const loading = ref(false);
const done = ref(false);
const err = ref("");

const curCat = ref("");

const subs = computed(() => cats.value.find((c) => c.id === curCat.value)?.children ?? []);

/** 数字格式化：手机屏窄，十万和千万要区别开 */
/** 观看人数是字符串（各平台单位不一），统一转成「万」显示 */
function fmt(raw: string | undefined): string {
  const n = Number(raw || 0);
  if (!n) return "";
  if (n >= 10000) return (n / 10000).toFixed(1).replace(/\.0$/, "") + "万";
  return String(n);
}

async function loadCats() {
  try {
    cats.value = await getCategories(store.current);
    curCat.value = cats.value[0]?.id ?? "";
    sub.value = "";
    await loadRooms(true);
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
    const cat = sub.value || curCat.value;
    const r = await getRooms(store.current, cat, page.value);
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

function onScroll(e: Event) {
  const el = e.target as HTMLElement;
  if (el.scrollHeight - el.scrollTop - el.clientHeight < 300) {
    if (!done.value && !loading.value) void loadRooms();
  }
}

/* ---------------- 骨架屏 ----------------
 * 只覆盖「首屏还没拿到任何数据」这一种情况：
 *   继续翻页时列表已经在屏上了，这时候再把内容抽掉换成灰块反而是倒退。
 */
const showSkeleton = computed(() => loading.value && rooms.value.length === 0);

/* ---------------- 下拉刷新 ----------------
 * 手机上「到顶再往下拉 = 刷新」是肌肉记忆，不做的话用户会觉得界面卡死了。
 * 关键点：只在 scrollTop<=0（已经在顶部）时才接管触摸；一旦手指改成上滑，
 * 立刻放弃接管、交还给浏览器滚动 —— 这样正常滚动完全不受影响。
 */
const PULL_TRIGGER = 60; // 触发刷新所需的下拉距离（px）
const pullY = ref(0); // 当前下拉位移，用来把提示条「撑」出来
const pulling = ref(false); // 是否处于下拉手势中
const refreshing = ref(false); // 是否正在刷新
let touchStartY = 0;

function onTouchStart(e: TouchEvent) {
  const el = e.currentTarget as HTMLElement;
  // 不在顶部、或正在刷新 → 不接管，原生滚动照常
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
    // 手指上滑 = 用户想看下面的内容，马上放弃下拉
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
  router.push(`/room/${store.current}/${encodeURIComponent(r.room_id)}`);
}

/**
 * 封面加载失败兜底：把破图藏起来，露出卡片的底色（--bg-3），
 * 避免手机上出现一排破图图标。失败多半是缩略图规则变了或图挂了，
 * 这里不做回退原图（原图更费流量，且多半一样挂）。
 */
function coverErr(e: Event) {
  const el = e.target as HTMLImageElement;
  el.style.visibility = "hidden";
}

onMounted(loadCats);
watch(() => store.current, loadCats);
watch(() => route.query.q, () => loadRooms(true));
</script>

<template>
  <div class="mh">
    <!-- 一级分类：横向滑动 -->
    <div class="mh-cats">
      <button
        v-for="c in cats"
        :key="c.id"
        class="mh-chip"
        :class="{ on: c.id === curCat }"
        @click="curCat = c.id; sub = ''; loadRooms(true)"
      >
        {{ c.name }}
      </button>
    </div>

    <!-- 二级分区：横向滑动 -->
    <div v-if="subs.length" class="mh-subs">
      <button class="mh-sub" :class="{ on: !sub }" @click="sub = ''; loadRooms(true)">全部</button>
      <button
        v-for="s in subs"
        :key="s.id"
        class="mh-sub"
        :class="{ on: s.id === sub }"
        @click="sub = s.id; loadRooms(true)"
      >
        {{ s.name }}
      </button>
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

      <div v-if="err" class="mh-warn">{{ err }}</div>

      <!-- 骨架屏：首屏没数据时用灰块占位，避免「白屏 + 一行文字」的坠落感 -->
      <div v-if="showSkeleton" class="mh-grid">
        <div v-for="i in 6" :key="i" class="mh-card">
          <div class="mh-thumb sk"></div>
          <div class="sk-line"></div>
          <div class="sk-line short"></div>
        </div>
      </div>

      <div v-else class="mh-grid">
        <button v-for="r in rooms" :key="r.room_id" class="mh-card" @click="open(r)">
          <div class="mh-thumb">
            <!-- 封面走 thumbUrl：移动端只拉 400px 的缩略图，省流量省内存 -->
            <img
              :src="thumbUrl(store.current, r.cover, 400)"
              referrerpolicy="no-referrer"
              loading="lazy"
              alt=""
              @error="coverErr"
            />
            <span v-if="r.online" class="mh-hot">{{ fmt(r.online) }}</span>
          </div>
          <div class="mh-title">{{ r.title }}</div>
          <div class="mh-sub-line">
            <span class="mh-dot" :style="{ background: store.platformColor(store.current) }"></span>
            {{ r.streamer }}
          </div>
        </button>
      </div>

      <div v-if="loading && rooms.length" class="mh-more">加载中…</div>
      <div v-else-if="done && rooms.length" class="mh-more">没有更多了</div>
      <div v-else-if="!rooms.length && !err && !showSkeleton" class="mh-more">
        这个分区暂时没有在播的直播间
      </div>
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

/* 横向滑动的标签条：手机上比换行的标签墙省一半高度 */
.mh-cats,
.mh-subs {
  display: flex;
  gap: 8px;
  overflow-x: auto;
  flex-shrink: 0;
  scrollbar-width: none;
  padding: 10px 12px 6px;
  background: var(--panel);
}
.mh-subs {
  padding-top: 0;
  padding-bottom: 10px;
  border-bottom: 1px solid var(--border);
}
.mh-cats::-webkit-scrollbar,
.mh-subs::-webkit-scrollbar {
  display: none;
}
.mh-chip {
  flex: 0 0 auto;
  height: 34px;
  padding: 0 14px;
  border-radius: 999px;
  border: 1px solid transparent;
  background: var(--chip);
  color: var(--fg-2);
  font-size: 14px;
}
.mh-chip.on {
  background: var(--brand-soft);
  color: var(--brand);
  border-color: var(--brand);
  font-weight: 600;
}
.mh-sub {
  flex: 0 0 auto;
  height: 30px;
  padding: 0 12px;
  border-radius: 999px;
  border: 0;
  background: none;
  color: var(--fg-dim);
  font-size: 13px;
}
.mh-sub.on {
  color: var(--brand);
  font-weight: 600;
  background: var(--brand-soft);
}

.mh-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  /* 关掉浏览器/WebView 自带的整页下拉刷新，否则会和我们的手势打架 */
  overscroll-behavior-y: contain;
  padding: 10px 10px 16px;
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
/* 跟随手指拖动时不要过渡，否则提示条会「追不上」手指显得发飘 */
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

/* 固定两列：手机上三列标题就看不清了 */
.mh-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.mh-card {
  display: flex;
  flex-direction: column;
  gap: 5px;
  border: 0;
  background: none;
  padding: 0;
  text-align: left;
  color: var(--fg);
}
.mh-thumb {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  border-radius: 8px;
  overflow: hidden;
  background: var(--chip);
}
.mh-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.mh-hot {
  position: absolute;
  right: 5px;
  top: 5px;
  padding: 1px 6px;
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  font-size: 11px;
}
.mh-title {
  font-size: 13px;
  line-height: 1.35;
  /* 两行截断，卡片高度才整齐 */
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.mh-sub-line {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: var(--fg-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mh-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
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

/* ---------- 骨架屏 ----------
   缩略图占位复用 .mh-thumb 的 16:9，标题占位两行（第二行短），与真实卡片
   一一对齐，数据到位时不会跳版。shimmer 靠灰阶渐变横扫实现，不引动画库。 */
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
