/**
 * Twitch 播放方式。
 *
 * ⚠️ 曾经还有第二种「官方播放器」（嵌 player.twitch.tv 的 iframe），已删除。
 * 删除原因（实测，不是猜测）：
 *   Twitch 会校验 parent 参数必须与**顶层页面真实 host** 一致，而 Tauri 打包后
 *   顶层是 `tauri.localhost`，对不上 → iframe 建好了但里面 0 条网络请求，
 *   Twitch 侧显示「拒绝连接」。dev 模式恰好在 localhost 所以能播，
 *   只有打包版坏 —— 这类 bug 最难查。
 *   改用 tauri-plugin-localhost 确实能解决（实测 iframe 正常加载），但它会
 *   把整个应用 origin 换掉，进而影响 localStorage 与其它平台，得不偿失。
 *   所以这里只保留自建流。
 *
 * 现在只有 `native` 一种：GQL 取 streamPlaybackAccessToken → usher 换 m3u8
 * → 交给 hls.js 播（见 src-tauri/src/platforms/twitch.rs）。
 * 好处：请求走 Rust 所以代理可控、画质能到 1920×1080、弹幕与画质选择器都是
 * 自己的。代价：usher token 约 1 小时过期，长时间看需重新进房间。
 */
import { ref, watch } from "vue";

export type TwitchMode = "native";

const KEY = "junlive.twitch_mode";

function load(): TwitchMode {
  // 旧版本可能存过 "official"，一律归到 native。
  return "native";
}

export const twitchMode = ref<TwitchMode>(load());

watch(twitchMode, (v) => {
  try {
    localStorage.setItem(KEY, v);
  } catch {
    /* 忽略 */
  }
});
