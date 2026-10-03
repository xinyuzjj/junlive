/**
 * 走「官方 iframe 播放器」的平台。
 *
 * 这些平台不用我们自己拉流，而是嵌平台的官方播放器 —— 参考
 * github.com/ilanzgx/multistream（README 原话 "Streams load from the official players"）。
 *
 * - **youtube**：YouTube 现在对 videoplayback 分片强制校验 PO token，
 *   自己拿到的 hlsManifestUrl 能拉列表、但分片一律 403（表现是一直缓冲）。
 * - **twitch**：自己拉 HLS 时清晰度会随 ABR 一直变，且 usher 的 token 有时效，
 *   容易出网络错误。官方播放器自己管清晰度选择、鉴权和重连。
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

export const EMBED_PLATFORMS = ["youtube", "twitch"];

/**
 * SOOP 例外：**是否走官方播放器由用户设置决定**（见 `soopMode.ts`）。
 * 所以它既不在 EMBED_PLATFORMS 里（默认自建流），又可能临时走 embed。
 */
export function isEmbedPlatform(p?: string): boolean {
  if (!p) return false;
  // SOOP：只有「自建流」不走 iframe，另外两种官方方式都走
  if (p === "soop") return soopMode.value !== "native";
  return EMBED_PLATFORMS.includes(p);
}

/** 父页面 host —— Twitch 的 embed 要求 parent 和嵌入页同域，否则拒绝渲染 */
export function parentHost(): string {
  try {
    const h = window.location.hostname;
    if (h && !h.includes("tauri")) return h;
  } catch {
    /* 忽略 */
  }
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
