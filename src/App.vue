<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listPlatforms, parseInput } from "./api";
import { store } from "./store";
import MobileShell from "./mobile/Shell.vue";

const router = useRouter();
const kw = ref("");

/**
 * 移动端判定。
 *
 * 手机屏放不下 PC 那套「顶栏七平台 + 搜索 + 跟随侧栏 + 五列卡片」——
 * 实测在 360~430px 宽度下侧栏和卡片全挤成一团、文字小到看不清。
 * 所以窄屏整一套换成移动布局：底部标签栏 + 单列/两列卡片 + 播放页上下分层。
 * 820px 是平板竖屏的门槛。
 */
const isMobile = ref(false);
function syncViewport() {
  isMobile.value = window.innerWidth <= 820;
}

/** 底部标签栏的当前项 */
const tab = computed(() => {
  const p = router.currentRoute.value.path;
  if (p.startsWith("/follow")) return "follow";
  if (p.startsWith("/settings") || p.startsWith("/about")) return "settings";
  return "home";
});

/* ---------------- 窗口按钮 ----------------
 * 原生标题栏已关掉（tauri.conf.json 里 decorations:false），
 * 顶部工具栏自己当标题栏：空白处可拖动，右侧三个自绘按钮。
 */
const win = getCurrentWindow();
const isMax = ref(false);

async function winMin() {
  await win.minimize();
}
async function winMax() {
  await win.toggleMaximize();
  isMax.value = await win.isMaximized();
}
async function winClose() {
  await win.close();
}

onMounted(async () => {
  try {
    isMax.value = await win.isMaximized();
    // 拖动边缘/双击最大化后同步图标（最大化时显示「还原」）
    await win.onResized(async () => {
      isMax.value = await win.isMaximized();
    });
  } catch {
    // 纯浏览器里跑（vite dev 直接开）没有 window API，忽略
  }
});

/**
 * 搜索框占位符。
 *
 * 抖音只能输房间号 —— 平台没有可用的匿名搜索接口
 * （`/aweme/v1/web/live/search/` 一律返回 2483「请先登录」），
 * DTV 也一样：它的抖音占位符就写着「搜索直播间号」。
 */
const searchPlaceholder = computed(() =>
  store.current === "douyin"
    ? "输入抖音直播间号（纯数字）或直播间链接"
    : "搜索主播 / 粘贴直播间链接 / 房间号",
);

function applyBrand() {
  const c = store.platformColor(store.current);
  const root = document.documentElement;
  root.style.setProperty("--brand", c);
  // 品牌色 10% 透明版，用于选中底
  const r = parseInt(c.slice(1, 3), 16);
  const g = parseInt(c.slice(3, 5), 16);
  const b = parseInt(c.slice(5, 7), 16);
  root.style.setProperty("--brand-soft", `rgba(${r}, ${g}, ${b}, 0.12)`);
  root.style.setProperty("--accent", c);
}

onMounted(async () => {
  syncViewport();
  window.addEventListener("resize", syncViewport);
  try {
    store.platforms = await listPlatforms();
    if (!store.platforms.some((p) => p.id === store.current)) {
      store.setPlatform(store.platforms[0]?.id || "bilibili");
    }
    applyBrand();
  } catch (e) {
    console.error(e);
  }
});

watch(() => store.current, applyBrand);

function go(path: string) {
  router.push(path);
}

function doSearch() {
  const text = kw.value.trim();
  if (!text) return;
  const parsed = parseInput(text);
  if (parsed && /[\/:.]/.test(text)) {
    router.push(`/room/${parsed.platform}/${encodeURIComponent(parsed.roomId)}`);
    return;
  }
  router.push({ path: "/", query: { kw: text, p: store.current, t: Date.now() } });
}

function switchPlatform(id: string) {
  store.setPlatform(id);
  router.replace({ path: "/", query: { p: id, t: Date.now() } });
}
</script>

<template>
  <!--
    移动端走**完全独立的一套界面**（src/mobile/Shell.vue + mobile/Home.vue + mobile/Room.vue），
    跟 PC 端不共用布局 —— 不是把 PC 界面缩小，而是两套设计。
    PC 那套「顶部七平台 + 搜索 + GitHub/设置 + 窗口按钮 + 左侧关注栏 + 五列卡片」
    在手机宽度下无论怎么压缩都放不下。
  -->
  <MobileShell v-if="isMobile" />

  <div v-else class="app">
    <header class="topbar" data-tauri-drag-region>
      <div class="logo" @click="go('/')">
        <span class="mark">▶</span>
        <span class="name">JunLive</span>
      </div>

      <nav class="platforms">
        <button
          v-for="p in store.platforms"
          :key="p.id"
          class="plat"
          :class="{ active: p.id === store.current }"
          :style="
            p.id === store.current
              ? { color: p.color, background: p.color + '18', borderColor: p.color + '55' }
              : {}
          "
          @click="switchPlatform(p.id)"
        >
          <span class="dot" :style="{ background: p.color }"></span>{{ p.name }}
        </button>
      </nav>

      <div class="spacer"></div>

      <div class="searchbox">
        <input
          v-model="kw"
          :placeholder="searchPlaceholder"
          @keyup.enter="doSearch"
        />
        <button class="icon-btn" title="搜索" @click="doSearch">🔍</button>
      </div>

      <button class="ghost round gh" title="GitHub 项目主页" @click="go('/about')">
        <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
          <path
            fill="currentColor"
            d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z"
          />
        </svg>
        GitHub
      </button>

      <button class="ghost round" @click="go('/settings')">设置</button>

      <!-- 窗口按钮（自绘标题栏）—— 移动端没有系统窗口，整组隐藏 -->
      <div v-if="!isMobile" class="wctrl">
        <button class="wc" title="最小化" @click="winMin">
          <svg viewBox="0 0 12 12"><path d="M2 6h8" /></svg>
        </button>
        <button
          class="wc"
          :title="isMax ? '向下还原' : '最大化'"
          @click="winMax"
        >
          <svg v-if="!isMax" viewBox="0 0 12 12">
            <rect x="2.5" y="2.5" width="7" height="7" rx="1.5" />
          </svg>
          <svg v-else viewBox="0 0 12 12">
            <rect x="2" y="4" width="6" height="6" rx="1.5" />
            <path d="M4.5 4V3.2A1.2 1.2 0 0 1 5.7 2h3.1A1.2 1.2 0 0 1 10 3.2v3.1A1.2 1.2 0 0 1 8.8 7.5H8" />
          </svg>
        </button>
        <button class="wc close" title="关闭" @click="winClose">
          <svg viewBox="0 0 12 12">
            <path d="M3 3l6 6M9 3l-6 6" />
          </svg>
        </button>
      </div>
    </header>

    <main class="content">
      <router-view :key="$route.fullPath" />
    </main>

    <!-- 移动端底部标签栏：PC 版靠顶栏切页，手机屏幕放不下，改成底部三大项 -->
    <nav v-if="isMobile" class="tabbar">
      <button :class="{ on: tab === 'home' }" @click="go('/')">
        <span class="ti">🏠</span><span>首页</span>
      </button>
      <button :class="{ on: tab === 'follow' }" @click="go('/follow')">
        <span class="ti">⭐</span><span>关注</span>
      </button>
      <button :class="{ on: tab === 'settings' }" @click="go('/settings')">
        <span class="ti">⚙️</span><span>设置</span>
      </button>
    </nav>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}

.topbar {
  display: flex;
  align-items: center;
  gap: 12px;
  height: 56px;
  /* 右侧留窄一点，让窗口按钮贴住窗口边缘 */
  padding: 0 8px 0 20px;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  flex: none;
}

/* 自绘标题栏按钮（原生标题栏已关掉） */
.wctrl {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-left: 6px;
  flex: none;
}
.wc {
  width: 34px;
  height: 30px;
  display: grid;
  place-items: center;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--fg-dim);
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}
.wc svg {
  width: 12px;
  height: 12px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.4;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.wc:hover {
  background: var(--chip);
  color: var(--fg);
}
.wc.close:hover {
  background: var(--danger);
  color: #fff;
}

.logo {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  user-select: none;
  flex: none;
}
.mark {
  color: var(--brand);
  font-size: 15px;
  transition: color 0.2s;
}
.name {
  font-weight: 700;
  font-size: 16px;
  letter-spacing: 0.2px;
  color: var(--fg);
}

.platforms {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  scrollbar-width: none;
  flex: none;
  max-width: 48%;
  margin-left: 6px;
}
.platforms::-webkit-scrollbar {
  display: none;
}
.plat {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: none;
  height: 30px;
  padding: 0 12px;
  font-size: 13px;
  border-radius: 999px;
  background: var(--chip);
  border: 1px solid transparent;
  color: var(--fg-2);
}
.plat:hover {
  background: var(--chip-hover);
  color: var(--fg);
}
.plat.active {
  font-weight: 600;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  display: inline-block;
  flex: none;
}

.spacer {
  flex: 1;
  min-width: 10px;
}

.searchbox {
  position: relative;
  flex: none;
  width: 300px;
}
.searchbox input {
  width: 100%;
  height: 36px;
  padding-right: 40px;
  font-size: 13px;
}
.icon-btn {
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
  width: 30px;
  height: 30px;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  font-size: 13px;
  opacity: 0.65;
}
.icon-btn:hover {
  opacity: 1;
  background: var(--chip-hover);
  border-radius: 50%;
}

.ghost.round {
  height: 32px;
  border-radius: 999px;
  position: relative;
  flex: none;
}

/* GitHub 按钮：图标 + 文字，和「设置」并排 */
.ghost.round.gh {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 13px;
}
.ghost.round.gh svg {
  flex: none;
}

.content {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
</style>
