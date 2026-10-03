/**
 * 走「官方 iframe 播放器」的平台。
 *
 * 这些平台不用我们自己拉流，而是嵌平台的官方播放器。
 *
 * - **youtube**：YouTube 现在对 videoplayback 分片强制校验 PO token，
 *   自己拿到的 hlsManifestUrl 能拉列表、但分片一律 403（表现是一直缓冲）。
 *   这不是我们能绕过的，只能用官方播放器。
 *
 * - **twitch**：用官方播放器。实测它更流畅（自建流虽然能到 1080p，
 *   但 usher 的 token 有时效、ABR 切档偶尔会卡；官方播放器自己管
 *   清晰度选择、鉴权与重连，稳定性更好）。
 *   ⚠️ 代价：Twitch 对 embed 有画质上限，实测锁在 **640x360**
 *   （自建流能到 1920x1080）。这是 Twitch 的政策，改不了。
 *
 * **SOOP 不走 embed**（试过，已撤回）：
 * 官方 embed 的画质选择器被官方硬关掉了（HTML 里 `<!-- 화질선택 임베디드는 미노출 -->`），
 * 实测落在 640x360 SD 档，而且 iframe 会把点击全吃掉，进去以后界面点不动。
 * 自建流能拿到 1080p/720p/540p/360p 四档（`--test room soop <id>` 实测）。
 *
 * 代价：官方播放器自带控件，我们的画质/线路选择器对它无效；
 * 它自带的全屏是 iframe 内部全屏，弹幕层会被盖住，所以全屏要用我们自己的按钮。
 */
import { soopEmbedSrc, soopMode, soopOfficialSrc } from "./soopMode";
import { twitchMode } from "./twitchMode";

export const EMBED_PLATFORMS = ["youtube"];

/**
 * SOOP / Twitch 例外：**是否走官方播放器由用户设置决定**
 * （见 `soopMode.ts` / `twitchMode.ts`）。
 * 所以它们既不在 EMBED_PLATFORMS 里（默认自建流），又可能临时走 embed。
 */
export function isEmbedPlatform(p?: string): boolean {
  if (!p) return false;
  // SOOP：只有「自建流」不走 iframe，另外两种官方方式都走
  if (p === "soop") return soopMode.value !== "native";
  // Twitch：用户可自选官方播放器（流畅）或自建流（1080p）
  if (p === "twitch") return twitchMode.value === "official";
  return EMBED_PLATFORMS.includes(p);
}

/**
 * 哪些平台需要代理。
 *
 * ⚠️ 必须与 Rust 侧保持一致：`net::relay()`（直连）给国内平台用，
 * `net::relay_proxy()`（走代理）给海外平台用。两边混用是国内平台变慢、
 * 海外平台连不上的根源。
 *
 * 国内：bilibili / douyu / huya / douyin —— 直连
 * 海外：twitch / youtube / soop —— 必须走代理
 */
const OVERSEAS_PLATFORMS = ["twitch", "youtube", "soop"];

/** 当前是不是在看海外平台 —— 决定 WebView 要不要用代理 */
export function isOverseas(platform: string | undefined): boolean {
  return !!platform && OVERSEAS_PLATFORMS.includes(platform);
}

/**
 * 父页面 host —— Twitch 的 embed 要求 `parent` 与嵌入页同域，否则拒绝渲染。
 *
 * ⚠️ 这里踩过两次坑，都因为「dev 能播 ≠ 安装版能播」：
 *
 * ① 原来写的是 `if (h && !h.includes("tauri")) return h; return "localhost"`
 *    而 Tauri 的 hostname 恰恰就是 `tauri.localhost`，所以判断永远为假、
 *    永远返回 "localhost"。—— 结果反而是对的，但纯属巧合。
 *
 * ② 后来我改成「Tauri 的特殊值原样透传」：
 *    `if (h === "tauri.localhost" || h === "localhost" || h === "127.0.0.1") return h`
 *    dev 版 origin 是 `http://localhost:1420` → 返回 "localhost" → **能播**；
 *    打包后 origin 是 `http://tauri.localhost` → 返回 "tauri.localhost"
 *    → **Twitch 拒绝**，报「player.twitch.tv 拒绝连接」。
 *    也就是说：本地 dev 一切正常，装成 exe 就坏 —— 最难查的一类问题。
 *
 * **结论：Twitch 的 parent 白名单里没有 `tauri.localhost` 这个值。**
 * 官方播放器在 Tauri 里只能用 `localhost`（Tauri 官方就是用这个值过审的）。
 * 所以现在无条件返回 `localhost` —— 两种环境都指向它，都能播。
 */
export function parentHost(): string {
  return "localhost";
}

/** 拼官方播放器地址；roomId 是视频 ID（YouTube）或频道 login（Twitch） */
export function embedSrc(platform: string, roomId: string): string {
  if (!roomId) return "";
  if (platform === "twitch") {
    // Twitch 用频道 login 名（不是数字房间号），room_id 存的正是 login
    return (
      `https://player.twitch.tv/?channel=${encodeURIComponent(roomId)}` +
      `&parent=${parentHost()}&autoplay=true&muted=false`
    );
  }
  if (platform === "soop") {
    // 两种官方方式二选一：
    //  - embed   官方播放器控件（干净、铺满，但画质框被官方注释掉了）
    //  - official 官方完整播放页（画质可调，但带整套官网界面）
    return soopMode.value === "embed" ? soopEmbedSrc(roomId) : soopOfficialSrc(roomId);
  }
  // YouTube：必须用 www.youtube.com 而不是 youtube-nocookie.com ——
  // nocookie 是另一个域名，登录窗拿到的 Cookie 在 youtube.com 上，带不过去，
  // embed 里会提示「需要登录」。
  // fs=0：关掉官方自带的全屏按钮（iframe 内部全屏，弹幕层会被盖住）。
  return `https://www.youtube.com/embed/${roomId}?autoplay=1&hl=zh-CN&fs=0`;
}

/** 官方 live_chat / 聊天 iframe 地址（目前只有 YouTube 需要） */
export function chatSrc(platform: string, roomId: string): string {
  if (platform !== "youtube" || !roomId) return "";
  return (
    `https://www.youtube.com/live_chat?v=${roomId}` +
    `&embed_domain=${parentHost()}&dark_theme=0`
  );
}
