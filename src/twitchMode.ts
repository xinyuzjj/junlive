/**
 * Twitch 播放方式（用户可自选）。
 *
 * Twitch 的两种方式各有明显取舍，所以交给用户选 —— 与 SOOP 同样的模式
 * （见 `soopMode.ts`）。
 *
 * - `official`（默认）**官方播放器**：`https://player.twitch.tv/?channel=<login>`
 *   嵌 Twitch 官方 iframe 播放器。
 *   好处：更流畅。官方播放器自己管清晰度选择、鉴权与重连，
 *   usher token 过期时它内部会自己换新，我们不用管。
 *   代价：① Twitch 对 embed 有画质上限，实测锁在 **640x360**
 *   （自建流能到 1920x1080）；② 官方自带控件，我们的画质选择器对它无效；
 *   ③ iframe 内的请求由 WebView 发出、**不经过 Rust**，所以依赖
 *   WebView 自己走系统代理（WebView2 默认就读系统代理，实测有效）。
 *
 * - `native` **自建流**：我们用 GQL 取 `streamPlaybackAccessToken`、
 *   经 usher 换到 m3u8，交给 hls.js 播（`platforms/twitch.rs` 已经实现好）。
 *   好处：画质能到 **1920x1080**、弹幕与画质选择器都是我们自己的、
 *   界面完全可控、请求走 Rust 所以代理可控。
 *   代价：usher 的 token 有时效（实测 `expires` 字段约 1 小时内），
 *   长时间看需要重新取；ABR 切档偶尔会卡一下。
 *
 * 默认选 `official`（用户要的是流畅）。
 */
import { ref, watch } from "vue";

export type TwitchMode = "official" | "native";

const KEY = "junlive.twitch_mode";

function load(): TwitchMode {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "official" || v === "native") return v;
  } catch {
    /* 忽略 */
  }
  return "official";
}

export const twitchMode = ref<TwitchMode>(load());

watch(twitchMode, (v) => {
  try {
    localStorage.setItem(KEY, v);
  } catch {
    /* 忽略 */
  }
});
