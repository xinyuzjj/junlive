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
import { getCategories, getRooms, type Category, type Room } from "../api";
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

function open(r: Room) {
  router.push(`/room/${store.current}/${encodeURIComponent(r.room_id)}`);
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

    <div class="mh-scroll" @scroll="onScroll">
      <div v-if="err" class="mh-warn">{{ err }}</div>

      <div class="mh-grid">
        <button v-for="r in rooms" :key="r.room_id" class="mh-card" @click="open(r)">
          <div class="mh-thumb">
            <img :src="r.cover" referrerpolicy="no-referrer" loading="lazy" alt="" />
            <span v-if="r.online" class="mh-hot">{{ fmt(r.online) }}</span>
          </div>
          <div class="mh-title">{{ r.title }}</div>
          <div class="mh-sub-line">
            <span class="mh-dot" :style="{ background: store.platformColor(store.current) }"></span>
            {{ r.streamer }}
          </div>
        </button>
      </div>

      <div v-if="loading" class="mh-more">加载中…</div>
      <div v-else-if="done && rooms.length" class="mh-more">没有更多了</div>
      <div v-else-if="!rooms.length && !err" class="mh-more">这个分区暂时没有在播的直播间</div>
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
  padding: 10px 10px 16px;
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
  background: var(--bg-3);
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
  color: var(--danger);
  font-size: 13px;
}
.mh-more {
  padding: 18px;
  text-align: center;
  color: var(--fg-dim);
  font-size: 13px;
}
</style>
