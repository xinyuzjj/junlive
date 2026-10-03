<script setup lang="ts">
/**
 * 移动端外壳 —— 与 PC 端**完全两套界面**，不共用布局。
 *
 * PC 端那套是「顶部七平台 + 搜索 + GitHub/设置 + 左侧关注栏 + 五列卡片 + 自绘窗口按钮」，
 * 那是给 1200px+ 的窗口设计的。手机上（360~430px）无论怎么压缩都塞不下，
 * 所以移动端单独做一套：
 *
 *   顶栏  只有「当前平台名 + 搜索」一行，点平台名弹出平台选择浮层
 *   内容  全屏单列，没有跟随侧栏
 *   底部  四大项标签栏（发现 / 首页 / 关注 / 设置），这是手机上唯一顺手的导航位置
 *
 * 播放页也是独立实现（mobile/Room.vue）：播放器在上、弹幕在下，不是 PC 的左右分栏。
 */
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { listPlatforms, parseInput } from "../api";
import { store } from "../store";
import MobileHome from "./Home.vue";
import MobileRoom from "./Room.vue";
import MobileDiscover from "./Discover.vue";
import Follow from "./Follow.vue";
import Settings from "../views/Settings.vue";

const router = useRouter();
const route = useRoute();
const kw = ref("");
const showPlat = ref(false);

/**
 * 移动端内部按路由切换视图。
 *
 * 为什么不用 <router-view> 再挂一套 /m/* 路由：手机上这几个页面本来就少，
 * 直接按路径分发更简单，也避免维护两套路由表导致 PC 端误跳。
 * 首页、播放页、关注页都是**移动端专属实现**（各自 src/mobile/*.vue）；
 * 只有设置页本身就是单列表单，窄屏下沿用 PC 版（不涉及「并排挤死」的问题）。
 */
const view = computed(() => {
  const p = route.path;
  if (p.startsWith("/room/")) return "room";
  if (p.startsWith("/discover")) return "discover";
  if (p.startsWith("/follow")) return "follow";
  if (p.startsWith("/settings") || p.startsWith("/about")) return "settings";
  return "home";
});
const roomArgs = computed(() => {
  const seg = route.path.split("/").filter(Boolean); // ["room", platform, id]
  return { platform: decodeURIComponent(seg[1] ?? ""), id: decodeURIComponent(seg[2] ?? "") };
});

const cur = computed(() => store.platforms.find((p) => p.id === store.current));

const searchPlaceholder = computed(() =>
  store.current === "douyin"
    ? "输入抖音直播间号或链接"
    : "搜索主播 / 房间号 / 粘链接",
);

function go(path: string) {
  showPlat.value = false;
  router.push(path);
}

function switchPlatform(id: string) {
  store.setPlatform(id);
  showPlat.value = false;
  if (router.currentRoute.value.path !== "/") router.push("/");
}

function doSearch() {
  const k = kw.value.trim();
  if (!k) return;
  // 粘链接/房间号直接进房间，否则当关键词搜
  const hit = parseInput(k);
  if (hit && hit.roomId) {
    router.push(`/room/${hit.platform}/${encodeURIComponent(hit.roomId)}`);
    kw.value = "";
  } else {
    router.push({ path: "/", query: { q: k } });
  }
}

onMounted(async () => {
  try {
    store.platforms = await listPlatforms();
    if (!store.platforms.some((p) => p.id === store.current)) {
      store.setPlatform(store.platforms[0]?.id || "bilibili");
    }
  } catch (e) {
    console.error(e);
  }
});
</script>

<template>
  <div class="m-app">
    <header class="m-top">
      <button class="m-plat" @click="showPlat = true">
        <span class="dot" :style="{ background: cur?.color || '#ff5d23' }"></span>
        {{ cur?.name || "平台" }}
        <span class="caret">▾</span>
      </button>

      <div class="m-search">
        <input v-model="kw" :placeholder="searchPlaceholder" @keyup.enter="doSearch" />
        <button @click="doSearch">🔍</button>
      </div>

      <!--
        深色模式开关。放在这里而不是设置页：不改 Settings.vue（那是 PC 共用页），
        且主题切换是高频操作，顶栏一键最顺手。
        图标语义：当前是浅色 → 显示 🌙（点了变深色）；当前深色 → 显示 ☀️。
      -->
      <button
        class="m-dark"
        :title="store.dark ? '切换浅色模式' : '切换深色模式'"
        :aria-label="store.dark ? '切换浅色模式' : '切换深色模式'"
        @click="store.toggleDark()"
      >
        {{ store.dark ? "☀️" : "🌙" }}
      </button>
    </header>

    <main class="m-body">
      <MobileHome v-if="view === 'home'" />
      <MobileRoom v-else-if="view === 'room'" :platform="roomArgs.platform" :id="roomArgs.id" />
      <MobileDiscover v-else-if="view === 'discover'" />
      <Follow v-else-if="view === 'follow'" />
      <Settings v-else />
    </main>

    <nav class="m-tabs">
      <button :class="{ on: $route.path === '/discover' }" @click="go('/discover')">
        <span class="ti">🧭</span><span>发现</span>
      </button>
      <button :class="{ on: $route.path === '/' }" @click="go('/')">
        <span class="ti">🏠</span><span>首页</span>
      </button>
      <button :class="{ on: $route.path.startsWith('/follow') }" @click="go('/follow')">
        <span class="ti">⭐</span><span>关注</span>
      </button>
      <button
        :class="{ on: $route.path.startsWith('/settings') || $route.path.startsWith('/about') }"
        @click="go('/settings')"
      >
        <span class="ti">⚙️</span><span>设置</span>
      </button>
    </nav>

    <!-- 平台选择浮层：手机上一个下拉列表比一排标签靠谱 -->
    <div v-if="showPlat" class="m-sheet" @click.self="showPlat = false">
      <div class="m-sheet-in">
        <div class="m-sheet-head">选择平台</div>
        <button
          v-for="p in store.platforms"
          :key="p.id"
          class="m-sheet-item"
          :class="{ on: p.id === store.current }"
          @click="switchPlatform(p.id)"
        >
          <span class="dot" :style="{ background: p.color }"></span>{{ p.name }}
          <span v-if="p.id === store.current" class="tick">✓</span>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.m-app {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}

/* ---------- 顶栏：只留平台名 + 搜索 ---------- */
.m-top {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.m-plat {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 38px;
  padding: 0 12px;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: var(--chip);
  color: var(--fg);
  font-size: 14px;
  font-weight: 600;
  white-space: nowrap;
  flex-shrink: 0;
}
.m-plat .dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}
.m-plat .caret {
  font-size: 10px;
  color: var(--fg-dim);
}
.m-search {
  flex: 1;
  min-width: 0;
  position: relative;
  display: flex;
  align-items: center;
}
.m-search input {
  width: 100%;
  height: 38px;
  padding: 0 40px 0 12px;
  border: 1px solid var(--border-2);
  border-radius: 8px;
  background: var(--bg);
  color: var(--fg);
  font-size: 15px; /* 小于 16px 时 iOS 会自动放大页面 */
  outline: none;
}
.m-search input:focus {
  border-color: var(--brand);
}
.m-search button {
  position: absolute;
  right: 2px;
  width: 36px;
  height: 34px;
  border: 0;
  background: none;
  font-size: 15px;
}

/* 深色开关：热区 44×44，满足最小触控面积要求 */
.m-dark {
  flex-shrink: 0;
  width: 44px;
  height: 44px;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 17px;
  border: 1px solid var(--border-2);
  border-radius: 10px;
  background: var(--chip);
  color: var(--fg);
}
.m-dark:active {
  background: var(--chip-hover);
}

.m-body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

/* ---------- 底部标签栏 ---------- */
.m-tabs {
  display: flex;
  flex-shrink: 0;
  height: 56px;
  background: var(--panel);
  border-top: 1px solid var(--border);
  padding-bottom: env(safe-area-inset-bottom, 0);
}
.m-tabs button {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  border: 0;
  background: none;
  font-size: 11px;
  color: var(--fg-dim);
}
.m-tabs button.on {
  color: var(--brand);
}
.m-tabs .ti {
  font-size: 19px;
  line-height: 1;
}

/* ---------- 平台选择浮层 ---------- */
.m-sheet {
  position: fixed;
  inset: 0;
  z-index: 9000;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: flex-end;
}
.m-sheet-in {
  width: 100%;
  background: var(--panel);
  border-radius: 14px 14px 0 0;
  padding: 8px 0 calc(8px + env(safe-area-inset-bottom, 0));
  max-height: 70vh;
  overflow-y: auto;
}
.m-sheet-head {
  padding: 10px 18px;
  font-size: 13px;
  color: var(--fg-dim);
}
.m-sheet-item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  height: 48px;
  padding: 0 18px;
  border: 0;
  background: none;
  font-size: 15px;
  color: var(--fg);
  text-align: left;
}
.m-sheet-item.on {
  color: var(--brand);
  font-weight: 600;
}
.m-sheet-item .dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
}
.m-sheet-item .tick {
  margin-left: auto;
}
</style>
