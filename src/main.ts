import { createApp, watch } from "vue";
import App from "./App.vue";
import router from "./router";
import { store, systemPrefersDark } from "./store";
import "./style.css";

/**
 * 一次性迁移：应用原名 AllLive，本地数据的键前缀是 `alllive.`。
 * 改名 JunLive 后前缀改成 `junlive.`，这里把老键搬过去，避免用户
 * 升级后关注列表 / 音量 / 弹幕设置全部丢失。
 *
 * 跑完打标记，之后不再执行。
 */
function migrateStorage() {
  if (localStorage.getItem("junlive.migrated")) return;
  const keys = [
    "follows",
    "platform",
    "volume",
    "dm_on",
    "dm_opacity",
    "dm_size",
    "dm_speed",
    "dm_area",
    "dm_block",
    "dm_color_mode",
    "dm_custom",
  ];
  for (const k of keys) {
    const v = localStorage.getItem(`alllive.${k}`);
    if (v !== null && localStorage.getItem(`junlive.${k}`) === null) {
      localStorage.setItem(`junlive.${k}`, v);
    }
  }
  localStorage.setItem("junlive.migrated", "1");
}

migrateStorage();

/**
 * 深色主题：为什么在 main.ts 里注入 <style>，而不是改 App.vue / style.css？
 *
 *  1. 本次改动**不允许碰 App.vue**；而 App.vue / style.css 里 `:root` 定义了
 *     浅色变量、组件又大量引用 var(--bg) 等 —— 那两处都不能动。
 *  2. 于是把深色变量写成一段独立的样式文本，启动时插到 <head> 末尾。
 *     它比 style.css 后插入，且选择器用 `html.dark`（优先级 0,1,1）
 *     高于 `:root`（0,1,0），能稳定覆盖浅色变量，不依赖样式表顺序。
 *  3. 开关落在 <html> 的 class 上，而不是某个组件根元素 —— 这样 PC 壳和
 *     移动端壳都会被覆盖；刷新后由 localStorage 里的 store.dark 恢复。
 *
 * 只覆盖颜色/背景类变量；--brand / --accent 是平台品牌色（App.vue 会动态
 * 设成当前平台色），属于品牌资产，深色下保持不变，这里刻意不写。
 */
const DARK_CSS = `
html.dark {
  --bg: #0f1115;
  --panel: #171a21;
  --card: #1c2029;
  --chip: #232833;
  --chip-hover: #2b3140;
  --border: #262b36;
  --border-2: #333a47;
  --fg: #e6e9ef;
  --fg-2: #b6bdc9;
  --fg-dim: #7e8798;
  --fg-mute: #565e6d;

  /* 兼容早期组件用过的变量名，一并给深色值，避免局部仍泛白 */
  --bg-2: #1c2029;
  --bg-3: #232833;
  --bg-4: #2b3140;

  --danger: #ff5c5c;
  --shadow: 0 4px 12px rgba(0, 0, 0, 0.5);
  --shadow-sm: 0 2px 8px rgba(0, 0, 0, 0.4);

  /* 让原生控件（滚动条、表单）跟着走深色 */
  color-scheme: dark;
}
`;

/** 把深色样式表插到 <head>（只插一次，幂等） */
function injectDarkStyle() {
  if (document.getElementById("junlive-dark-theme")) return;
  const el = document.createElement("style");
  el.id = "junlive-dark-theme";
  el.textContent = DARK_CSS;
  document.head.appendChild(el);
}

/**
 * 当前实际是否要用深色：
 *   - "dark" / "light" 强制
 *   - "system" 跟随系统偏好（systemPrefersDark 由下方 matchMedia 维护）
 */
function resolvedDark(): boolean {
  if (store.theme === "dark") return true;
  if (store.theme === "light") return false;
  return systemPrefersDark.value;
}

/** 把实际主题同步到 <html> 的 class 上（立即生效） */
function applyTheme() {
  document.documentElement.classList.toggle("dark", resolvedDark());
}

/*
 * 跟随系统：监听 prefers-color-scheme。
 * 结果写进 store.systemPrefersDark，theme="system" 时下面的 watch 会重算并生效。
 * 只在启动时注册一次（main.ts 模块只执行一次）。
 */
const mql = window.matchMedia("(prefers-color-scheme: dark)");
systemPrefersDark.value = mql.matches;
mql.addEventListener("change", (e) => {
  systemPrefersDark.value = e.matches;
});

injectDarkStyle();
applyTheme();

// 立即生效的两条触发路径：
//   1) 用户在三档里切换 → store.theme 变
//   2) theme="system" 时系统偏好变 → systemPrefersDark 变
// 用 watch 而非让 store 自己操作 DOM，是为了保持 store 与渲染环境解耦。
watch(() => store.theme, applyTheme);
watch(systemPrefersDark, applyTheme);

createApp(App).use(router).mount("#app");
