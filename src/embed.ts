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
 * 代价：官方播放器自带控件，我们的画质/线路选择器对它无效；
 * 它自带的全屏是 iframe 内部全屏，弹幕层会被盖住，所以全屏要用我们自己的按钮。
 *
 * **SOOP 不在这个列表里**（曾经在，已撤回）：官方 embed 的 HTML 里明确写着
 * `<!-- 화질선택 임베디드는 미노출 -->`（画质选择在 embed 里不显示），
 * 画质被锁死在 auto —— 实测在 WebView 里直接掉到 640x360 SD 档
 * （分片 URL `/640x360/xxx-common-sd-hls_*.TS`），而且菜单点不到。
 * 自建流反而能拿到 1080p/720p/540p/360p 四档（`--test room soop <id>` 实测），
 * 交给我们的 hls.js + 清晰度锁定（≤1080p 取最高）就能上 1080p。
 */
export const EMBED_PLATFORMS = ["youtube", "twitch"];

export function isEmbedPlatform(p?: string): boolean {
  return !!p && EMBED_PLATFORMS.includes(p);
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
    // SOOP 已经不走 embed 了（画质被官方锁死，见上面的说明）。
    // 保留这个分支只是防御：正常不会再走到这里。
    return "";
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
