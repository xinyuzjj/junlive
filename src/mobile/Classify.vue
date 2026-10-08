<script setup lang="ts">
/**
 * 移动端「分类」页 —— 一级 / 二级分区浏览。
 *
 * 为什么独立成页（对齐 Simple Live）：
 *   Simple Live 把真分类放在独立的「分类」tab，首页顶部只留平台
 *   （调研 A2：首页 AppBar 是平台 TabBar，一级/二级分类在 category 页）。
 *   旧版把分类横向条绑在首页顶部，导致首页既要切平台又要切分类、层级错乱。
 *   卡片规格与首页完全一致（固定高 110 封面 / 最少 2 列 / 只留四项）。
 */
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { getCategories, getRooms, thumbUrl, type Category, type Room } from "../api";
import { store } from "../store";

const router = useRouter();

const cats = ref<Category[]>([]);
const curCat = ref("");
const sub = ref("");
const rooms = ref<Room[]>([]);
const page = ref(1);
const loading = ref(false);
const done = ref(false);
const err = ref("");

const subs = computed(() => cats.value.find((c) => c.id === curCat.value)?.children ?? []);

/** 首屏无数据时才用骨架屏：翻页时列表已在屏上，抽掉内容反而倒退 */
const showSkeleton = computed(() => loading.value && rooms.value.length === 0);

async function loadCats() {
  err.value = "";
  cats.value = [];
  curCat.value = "";
  sub.value = "";
  try {
    const cs = await getCategories(store.current);
    cats.value = cs;
    // 恢复上次浏览的版块：Shell 用 v-if 切页，从直播间返回时本组件会重建，
    // 不恢复就会跳回第一个分类（等于「返回就回平台首页」）。
    const saved = store.catSel[store.current];
    const hit = saved ? cs.find((c) => c.id === saved.parent) : undefined;
    if (hit) {
      curCat.value = hit.id;
      sub.value =
        saved && hit.children.some((x) => x.id === saved.sub) ? saved.sub : "";
    } else {
      curCat.value = cs[0]?.id ?? "";
      sub.value = "";
    }
  } catch (e) {
    err.value = String(e);
  }
  await loadRooms(true);
}

async function loadRooms(reset = false) {
  if (loading.value) return;
  if (!curCat.value) {
    rooms.value = [];
    return;
  }
  loading.value = true;
  err.value = "";
  if (reset) {
    page.value = 1;
    rooms.value = [];
    done.value = false;
  }
  try {
    const r = await getRooms(store.current, sub.value || curCat.value, page.value);
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

function pickCat(id: string) {
  curCat.value = id;
  sub.value = "";
  store.setCatSel(store.current, curCat.value, sub.value);
  void loadRooms(true);
}

function pickSub(id: string) {
  sub.value = id;
  store.setCatSel(store.current, curCat.value, sub.value);
  void loadRooms(true);
}

function onScroll(e: Event) {
  const el = e.target as HTMLElement;
  if (el.scrollHeight - el.scrollTop - el.clientHeight < 300) {
    if (!done.value && !loading.value) void loadRooms();
  }
}

function open(r: Room) {
  router.push(`/room/${r.platform || store.current}/${encodeURIComponent(r.room_id)}`);
}

/** 封面加载失败兜底：藏起破图，露出卡片底色，避免一排破图图标 */
function coverErr(e: Event) {
  (e.target as HTMLImageElement).style.visibility = "hidden";
}

onMounted(loadCats);
// 切平台后分区表整个换掉，重新拉
watch(() => store.current, loadCats);
</script>

<template>
  <div class="mc">
    <div class="mc-head">分类 · {{ store.platformName(store.current) }}</div>

    <!-- 一级分类：横滑 -->
    <div class="mc-row">
      <button
        v-for="c in cats"
        :key="c.id"
        class="mc-chip"
        :class="{ on: c.id === curCat }"
        @click="pickCat(c.id)"
      >
        {{ c.name }}
      </button>
    </div>

    <!-- 二级分区：横滑 -->
    <div v-if="subs.length" class="mc-row mc-row-sub">
      <button class="mc-sub" :class="{ on: !sub }" @click="pickSub('')">全部</button>
      <button
        v-for="s in subs"
        :key="s.id"
        class="mc-sub"
        :class="{ on: s.id === sub }"
        @click="pickSub(s.id)"
      >
        {{ s.name }}
      </button>
    </div>

    <div class="mc-scroll" @scroll="onScroll">
      <div v-if="err" class="mc-warn">{{ err }}</div>

      <div v-if="showSkeleton" class="mc-grid">
        <div v-for="i in 6" :key="i" class="mc-card">
          <div class="mc-thumb sk"></div>
          <div class="sk-line"></div>
          <div class="sk-line short"></div>
        </div>
      </div>

      <div v-else class="mc-grid">
        <button
          v-for="r in rooms"
          :key="r.platform + r.room_id"
          class="mc-card"
          @click="open(r)"
        >
          <div class="mc-thumb">
            <img
              :src="thumbUrl(store.current, r.cover, 400)"
              referrerpolicy="no-referrer"
              loading="lazy"
              alt=""
              @error="coverErr"
            />
            <!-- 在线人数直接显示后端给的字符串（后端已格式化成「1.2万」），不再二次 Number() -->
            <span v-if="r.online" class="mc-hot">{{ r.online }}</span>
          </div>
          <div class="mc-title">{{ r.title || "（无标题）" }}</div>
          <div class="mc-streamer">{{ r.streamer || "-" }}</div>
        </button>
      </div>

      <div v-if="loading && rooms.length" class="mc-more">加载中…</div>
      <div v-else-if="done && rooms.length" class="mc-more">没有更多了</div>
      <div v-else-if="!rooms.length && !err && !showSkeleton" class="mc-more">
        这个分区暂时没有在播的直播间
      </div>
    </div>
  </div>
</template>

<style scoped>
.mc {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: var(--bg);
}
.mc-head {
  flex-shrink: 0;
  padding: 12px 14px 4px;
  font-size: 15px;
  font-weight: 700;
  color: var(--fg);
  background: var(--panel);
}

/* 横滑分类条：手机上比换行的标签墙省一半高度 */
.mc-row {
  display: flex;
  gap: 8px;
  overflow-x: auto;
  flex-shrink: 0;
  scrollbar-width: none;
  padding: 10px 12px;
  background: var(--panel);
}
.mc-row-sub {
  padding-top: 0;
  padding-bottom: 10px;
  border-bottom: 1px solid var(--border);
}
.mc-row::-webkit-scrollbar {
  display: none;
}
.mc-chip {
  flex: 0 0 auto;
  height: 44px;
  padding: 0 16px;
  border-radius: 999px;
  border: 1px solid transparent;
  background: var(--chip);
  color: var(--fg-2);
  font-size: 14px;
}
.mc-chip.on {
  background: var(--brand-soft);
  color: var(--brand);
  border-color: var(--brand);
  font-weight: 600;
}
.mc-sub {
  flex: 0 0 auto;
  height: 44px;
  padding: 0 14px;
  border-radius: 999px;
  border: 0;
  background: none;
  color: var(--fg-dim);
  font-size: 13px;
}
.mc-sub.on {
  color: var(--brand);
  font-weight: 600;
  background: var(--brand-soft);
}

.mc-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior-y: contain;
  padding: 8px 8px 16px;
}

/* 列数=floor(宽/200)最少 2 列：用 auto-fill + minmax(170px) 近似
   （170 保证 390px 宽下仍是 2 列，不会塌成 1 列）。对齐调研 B1。 */
.mc-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
  gap: 8px;
}
.mc-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  border: 0;
  background: none;
  padding: 0;
  text-align: left;
  color: var(--fg);
}
/* 封面：固定高 110、宽撑满、cover（不是 16:9）—— 调研 B3，手机上一行能多看到内容 */
.mc-thumb {
  position: relative;
  width: 100%;
  height: 110px;
  border-radius: 6px;
  overflow: hidden;
  background: var(--chip);
}
.mc-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
/* 在线人数角标：用主题变量而非写死的黑色——深浅色下都读得清，也符合「只用 CSS 变量」 */
.mc-hot {
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
/* 标题只留一行（不是两行），卡片高度才整齐 */
.mc-title {
  font-size: 13px;
  line-height: 1.4;
  color: var(--fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mc-streamer {
  font-size: 12px;
  color: var(--fg-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mc-warn {
  padding: 10px 12px;
  color: var(--accent);
  font-size: 13px;
}
.mc-more {
  padding: 18px;
  text-align: center;
  color: var(--fg-dim);
  font-size: 13px;
}

/* 骨架屏：缩略图占位同样固定高 110，数据到位时不跳版 */
.mc-thumb.sk,
.sk-line {
  background: linear-gradient(
    100deg,
    var(--chip) 30%,
    var(--chip-hover) 50%,
    var(--chip) 70%
  );
  background-size: 200% 100%;
  animation: mc-shimmer 1.3s ease-in-out infinite;
}
@keyframes mc-shimmer {
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
