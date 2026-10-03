import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
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

createApp(App).use(router).mount("#app");
