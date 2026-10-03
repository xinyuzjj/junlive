<script setup lang="ts">
/**
 * 移动端外壳 —— 与 PC 端**完全两套界面**，不共用布局。
 *
 * 导航模型**照 Simple Live 的成熟做法**重做（调研 A1 节，证据：
 * dart_simple_live/simple_live_app/lib/modules/indexed/indexed_page.dart:12-78）：
 *   底部 4 个 tab：首页 / 关注 / 分类 / 我的；
 *   NavigationBar 高度 56，且**始终隐藏文字标签、只留图标**
 *   （labelBehavior = alwaysHide），省下的高度全给内容 —— 手机上一屏能多看到一行卡片。
 * 图标一律用内联 SVG，不用 emoji：emoji 在不同系统里字形/大小/基线都不一致，
 * 也没法用 currentColor 跟随选中态。
 *
 * 顶栏只保留「搜索」与「深色开关」。平台切换搬到首页顶部的横滑条
 * （对齐 Simple Live：平台 TabBar 是首页 AppBar 的一部分，不在全局外壳里），
 * 避免首页同时出现两个平台选择器。
 *
 * /settings 与 /about 都必须真的能显示 —— 旧版把 /about 也判成设置页，
 * 点「项目介绍」又回到设置页，形成死循环。这里拆成独立分支，并在「我的」页
 * 给它们各一个入口；三个路径（/me、/settings、/about）共用「我的」tab 的高亮。
 */
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { listPlatforms, parseInput } from "../api";
import { store } from "../store";
import MobileHome from "./Home.vue";
import MobileRoom from "./Room.vue";
import MobileClassify from "./Classify.vue";
import Follow from "./Follow.vue";
import Settings from "../views/Settings.vue";
import About from "../views/About.vue";

const router = useRouter();
const route = useRoute();
const kw = ref("");

/**
 * 移动端内部按路由切换视图。
 *
 * 为什么不用 <router-view> 再挂一套 /m/* 路由：手机上这几个页面本来就少，
 * 直接按路径分发更简单，也避免维护两套路由表导致 PC 端误跳。
 * `/classify`、`/me` 都已在 router.ts 注册（否则 vue-router 会打 No match 告警）。
 */
const view = computed(() => {
  const p = route.path;
  if (p.startsWith("/room/")) return "room";
  if (p.startsWith("/follow")) return "follow";
  if (p.startsWith("/classify")) return "classify";
  if (p.startsWith("/settings")) return "settings";
  if (p.startsWith("/about")) return "about";
  if (p.startsWith("/me")) return "me";
  return "home";
});

/** 「我的」tab 的高亮范围：我的页 + 设置页 + 关于页都算「我的」 */
const isMe = computed(() => ["me", "settings", "about"].includes(view.value));

const roomArgs = computed(() => {
  const seg = route.path.split("/").filter(Boolean); // ["room", platform, id]
  return { platform: decodeURIComponent(seg[1] ?? ""), id: decodeURIComponent(seg[2] ?? "") };
});

/**
 * 搜索框占位符。抖音没有可用的匿名关键词搜索接口（只能按房间号），
 * 文案区分开，避免用户以为能搜关键词。
 */
const searchPlaceholder = computed(() =>
  store.current === "douyin" ? "输入抖音直播间号或链接" : "搜索主播 / 房间号 / 粘链接",
);

function go(path: string) {
  router.push(path);
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
    // 关键词塞进 ?q=，由首页 Home.vue 监听后调 searchRooms（移动端搜索真正的实现）
    router.push({ path: "/", query: { q: k } });
  }
}

onMounted(async () => {
  // App.vue 也会加载平台；这里兜一次，保证单独渲染外壳时平台条/主题色不缺数据
  try {
    if (!store.platforms.length) store.platforms = await listPlatforms();
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
    <!--
      播放页（/room/...）要「沉浸」：顶栏和底部标签栏都隐藏，让移动端播放页
      自己用画面上的浮层按钮做返回/全屏、底部操作条做弹幕交互。
      这是 Shell 里唯一为播放页做的让步（调研：原来一个竖屏页面套了 4 层 chrome）。
    -->
    <header v-if="view !== 'room'" class="m-top">
      <div class="m-search">
        <input
          v-model="kw"
          :placeholder="searchPlaceholder"
          enterkeyhint="search"
          @keyup.enter="doSearch"
        />
        <button class="m-search-btn" aria-label="搜索" @click="doSearch">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
            <circle cx="11" cy="11" r="7" />
            <path d="M20 20l-3.6-3.6" />
          </svg>
        </button>
      </div>

      <!-- 深色模式开关：不改 Settings.vue（PC 共用页），且主题切换高频，顶栏一键最顺手 -->
      <button
        class="m-dark"
        :title="store.dark ? '切换浅色模式' : '切换深色模式'"
        :aria-label="store.dark ? '切换浅色模式' : '切换深色模式'"
        @click="store.toggleDark()"
      >
        <svg
          v-if="store.dark"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
        >
          <circle cx="12" cy="12" r="4.2" />
          <path d="M12 3v2.2M12 18.8V21M3 12h2.2M18.8 12H21M5.6 5.6l1.6 1.6M16.8 16.8l1.6 1.6M18.4 5.6l-1.6 1.6M7.2 16.8l-1.6 1.6" />
        </svg>
        <svg
          v-else
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M20.5 14.5A8.2 8.2 0 0 1 9.5 3.5a8.2 8.2 0 1 0 11 11z" />
        </svg>
      </button>
    </header>

    <main class="m-body">
      <MobileHome v-if="view === 'home'" />
      <MobileRoom v-else-if="view === 'room'" :platform="roomArgs.platform" :id="roomArgs.id" />
      <MobileClassify v-else-if="view === 'classify'" />
      <Follow v-else-if="view === 'follow'" />
      <Settings v-else-if="view === 'settings'" />
      <About v-else-if="view === 'about'" />

      <!-- 「我的」页：给设置与关于各一个入口，两者都能真正打开（修掉旧版 /about 死循环） -->
      <div v-else-if="view === 'me'" class="m-me">
        <button class="m-me-item" @click="go('/settings')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="3.2" />
            <path d="M19.4 13a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2 2 2 0 1 1-4 0 1.7 1.7 0 0 0-2.9-1.2l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 3 13a2 2 0 1 1 0-4 1.7 1.7 0 0 0 1.2-2.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 10 2.4a2 2 0 1 1 4 0 1.7 1.7 0 0 0 2.9 1.2l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1A1.7 1.7 0 0 0 21 9a2 2 0 1 1 0 4z" />
          </svg>
          <span class="m-me-t">设置</span>
          <span class="m-me-c">›</span>
        </button>
        <button class="m-me-item" @click="go('/about')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="9" />
            <path d="M12 11v5.5M12 7.6v.2" />
          </svg>
          <span class="m-me-t">关于 / GitHub</span>
          <span class="m-me-c">›</span>
        </button>
      </div>
    </main>

    <!-- 底部 4 个 tab（对齐 Simple Live）：只留图标不显示文字，高度 56 + 底部安全区 -->
    <nav v-if="view !== 'room'" class="m-tabs">
      <button :class="{ on: view === 'home' }" aria-label="首页" @click="go('/')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3 10.5 12 3l9 7.5" />
          <path d="M5 9.5V20a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V9.5" />
        </svg>
      </button>
      <button :class="{ on: view === 'follow' }" aria-label="关注" @click="go('/follow')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 3.6l2.6 5.3 5.8.85-4.2 4.1 1 5.8L12 16.9l-5.2 2.75 1-5.8-4.2-4.1 5.8-.85z" />
        </svg>
      </button>
      <button :class="{ on: view === 'classify' }" aria-label="分类" @click="go('/classify')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3.5" y="3.5" width="7" height="7" rx="1.6" />
          <rect x="13.5" y="3.5" width="7" height="7" rx="1.6" />
          <rect x="3.5" y="13.5" width="7" height="7" rx="1.6" />
          <rect x="13.5" y="13.5" width="7" height="7" rx="1.6" />
        </svg>
      </button>
      <button :class="{ on: isMe }" aria-label="我的" @click="go('/me')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="8" r="3.6" />
          <path d="M4.5 20c0-3.6 3.4-6 7.5-6s7.5 2.4 7.5 6" />
        </svg>
      </button>
    </nav>
  </div>
</template>

<style scoped>
.m-app {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}

/* ---------- 顶栏：搜索 + 深色开关 ----------
   补 env(safe-area-inset-top)：否则顶栏会顶到状态栏/刘海下面（审查报告「严重」项）。 */
.m-top {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: calc(env(safe-area-inset-top, 0px) + 8px) 12px 8px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.m-search {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
}
.m-search input {
  width: 100%;
  height: 40px;
  padding: 0 46px 0 14px;
  border: 1px solid var(--border-2);
  border-radius: 10px;
  background: var(--bg);
  color: var(--fg);
  /* 必须 ≥16px：iOS 在 <16px 的输入框聚焦时会整页放大（旧版写 15px，注释自相矛盾） */
  font-size: 16px;
  outline: none;
}
.m-search input:focus {
  border-color: var(--brand);
}
.m-search-btn {
  position: absolute;
  right: 2px;
  width: 44px; /* 热区 ≥44 */
  height: 40px;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 0;
  background: none;
  color: var(--fg-dim);
}
.m-search-btn svg {
  width: 20px;
  height: 20px;
}
.m-search-btn:active {
  color: var(--brand);
}

/* 深色开关：热区 44×44 */
.m-dark {
  flex-shrink: 0;
  width: 44px;
  height: 44px;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-2);
  border-radius: 10px;
  background: var(--chip);
  color: var(--fg);
}
.m-dark svg {
  width: 20px;
  height: 20px;
}
.m-dark:active {
  background: var(--chip-hover);
}

.m-body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

/* ---------- 底部 4 tab ----------
   高度 56 + 底部安全区；每个 tab 热区 = 高 56 × 宽 1/4（≥48）。
   只显示图标（Simple Live 的做法），不带文字标签。 */
.m-tabs {
  display: flex;
  flex-shrink: 0;
  height: 56px;
  background: var(--panel);
  border-top: 1px solid var(--border);
  padding-bottom: env(safe-area-inset-bottom, 0px);
}
.m-tabs button {
  flex: 1;
  min-height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 0;
  background: none;
  color: var(--fg-dim);
}
.m-tabs button svg {
  width: 24px;
  height: 24px;
}
.m-tabs button.on {
  color: var(--brand);
}
.m-tabs button:active {
  background: var(--chip);
}

/* ---------- 「我的」页 ---------- */
.m-me {
  height: 100%;
  overflow-y: auto;
  background: var(--bg);
  padding: 8px 0 calc(16px + env(safe-area-inset-bottom, 0px));
}
.m-me-item {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  min-height: 56px; /* 热区 ≥44 */
  padding: 0 18px;
  border: 0;
  background: none;
  color: var(--fg);
  font-size: 16px;
  text-align: left;
}
.m-me-item:active {
  background: var(--chip);
}
.m-me-item svg {
  width: 22px;
  height: 22px;
  color: var(--fg-dim);
  flex: none;
}
.m-me-t {
  flex: 1;
  min-width: 0;
}
.m-me-c {
  color: var(--fg-dim);
  font-size: 18px;
}
</style>
