import { invoke } from "@tauri-apps/api/core";

export interface PlatformInfo {
  id: string;
  name: string;
  color: string;
  danmaku: boolean;
}

export interface Category {
  id: string;
  name: string;
  children: Category[];
}

export interface Room {
  platform: string;
  room_id: string;
  title: string;
  streamer: string;
  cover: string;
  area: string;
  online: string;
  live: boolean;
  avatar: string;
}

export interface PlayUrl {
  url: string;
  proxy: string;
  format: string;
  quality: string;
}

export interface RoomDetail extends Room {
  description: string;
  plays: PlayUrl[];
}

export interface RoomList {
  rooms: Room[];
  page: number;
  has_more: boolean;
}

export const listPlatforms = () => invoke<PlatformInfo[]>("list_platforms");

export const getCategories = (platform: string) =>
  invoke<Category[]>("get_categories", { platform });

export const getRooms = (platform: string, category: string, page = 1) =>
  invoke<RoomList>("get_rooms", { platform, category, page });

export const searchRooms = (platform: string, keyword: string, page = 1) =>
  invoke<RoomList>("search_rooms", { platform, keyword, page });

export const getRoom = (platform: string, roomId: string) =>
  invoke<RoomDetail>("get_room", { platform, roomId });

export const getProxySetting = () => invoke<string | null>("get_proxy_setting");

export const setProxySetting = (proxy: string | null) =>
  invoke<void>("set_proxy_setting", { proxy });

export const getProxyPort = () => invoke<number>("get_proxy_port");

export const getYoutubeCookie = () =>
  invoke<string | null>("get_youtube_cookie");

export const setYoutubeCookie = (cookie: string | null) =>
  invoke<void>("set_youtube_cookie", { cookie });

/** 弹幕 */
export interface DanmakuMsg {
  platform: string;
  user: string;
  text: string;
  color: string;
  kind: string;
  ts: number;
}

export const getBilibiliCookie = () => invoke<string | null>("get_bilibili_cookie");

export const setBilibiliCookie = (cookie: string | null) =>
  invoke<void>("set_bilibili_cookie", { cookie });

export const startDanmaku = (platform: string, roomId: string) =>
  invoke<void>("start_danmaku", { platform, roomId });

export const stopDanmaku = () => invoke<void>("stop_danmaku");

/**
 * 登记「续流上下文」：播放地址到期后，由本地代理自己重新解析一次，
 * 播放器无需重载（斗鱼直链固定 300 秒就被 CDN 主动断开，不续的话看 5 分钟必断）。
 * roomId 传空字符串表示清除。
 */
export const setStreamRenew = (platform: string, roomId: string, quality: string) =>
  invoke<void>("set_stream_renew", { platform, roomId, quality });

/** 从各种输入里猜平台和房间号 */
export function parseInput(
  input: string,
): { platform: string; roomId: string } | null {
  const s = input.trim();
  if (!s) return null;

  const rules: Array<[RegExp, string, (m: RegExpMatchArray) => string]> = [
    [/live\.bilibili\.com\/(\d+)/, "bilibili", (m) => m[1]],
    [/b23\.tv\/(\w+)/, "bilibili", (m) => m[1]],
    [/douyu\.com\/(\d+)/, "douyu", (m) => m[1]],
    [/huya\.com\/(\d+)/, "huya", (m) => m[1]],
    [/live\.douyin\.com\/(\w+)/, "douyin", (m) => m[1]],
    [/youtube\.com\/watch\?v=([\w-]+)/, "youtube", (m) => m[1]],
    [/youtu\.be\/([\w-]+)/, "youtube", (m) => m[1]],
    [/sooplive\.co\.kr\/(\w+)/, "soop", (m) => m[1]],
    [/afreecatv\.com\/(\w+)/, "soop", (m) => m[1]],
    [/twitch\.tv\/(\w+)/, "twitch", (m) => m[1]],
  ];

  for (const [re, platform, pick] of rules) {
    const m = s.match(re);
    if (m) return { platform, roomId: pick(m) };
  }
  // 纯数字 → 默认当作 B 站
  if (/^\d+$/.test(s)) return { platform: "bilibili", roomId: s };
  return null;
}

/**
 * 头像地址兜底。
 * 斗鱼接口返回的是**相对路径**（形如 `avatar_v3/202004/xxxx`），
 * 必须拼成 `https://apic.douyucdn.cn/upload/<av>_big.jpg`，
 * 否则图片 404、界面只能显示首字母占位。
 * 后端已修，这里再兜一次老 localStorage 数据。
 */
export function avatarUrl(platform: string, avatar?: string | null): string {
  const a = (avatar || "").trim();
  if (!a) return "";
  if (a.startsWith("http") || a.startsWith("//")) return a;
  if (platform === "douyu") return `https://apic.douyucdn.cn/upload/${a}_big.jpg`;
  return a;
}

/**
 * 把封面 URL 换成「手机够用的小图」，降低移动端的流量与内存。
 *
 * 设计原则（重要）：
 *   1. 各平台规则**完全独立**，不做任何跨平台兜底猜测——本项目硬规矩。
 *   2. 拿不准的平台一律**原样返回**，宁可不变也不要拼出裂图（列表一裂就是满屏破图）。
 *   3. 纯前端字符串处理，不改后端、不加依赖、不发请求。
 *
 * 各平台实测后确定的规则（w 为目标宽度，按 16:9 推导高度）：
 *   bilibili  i0.hdslb.com 图床支持 `@<宽>w_<高>h_1c.jpg` 处理后缀（1c=居中裁剪），
 *             实测 400w_225h_1c 能把 200KB 原图降到 ~22KB，安全。
 *   huya      msstatic 图床支持 OSS 处理参数 `x-oss-process=image/resize,w_<宽>`，
 *             实测 live-cover 主机 162KB→17KB；anchorpost 主机不认但会原样返回原图，
 *             仍然 200，不会裂图，所以统一拼上。
 *   soop      liveimg.sooplive.com 的档位在**路径段**里：l=240x135、m=480x270，
 *             其它字母都会回落到 480x270。按目标宽度选最小够用的一档。
 *   twitch    static-cdn.jtvnw.net 的 `...-<宽>x<高>.jpg` 支持任意尺寸，直接改。
 *   youtube   i.ytimg.com/vi/<id>/<规格>.jpg，换成更小的规格文件名（m 系列恒为 16:9）。
 *
 * 原样返回（实测无法安全缩图）：
 *   douyu     列表用的 `rs16` 本身就是最小档 `/dy1`（仅 ~8KB）；试过的
 *             `/dy2`、`/320/180/…` 等路径规则要么更大、要么 404。没有安全参数，原样。
 *   douyin    封面是带 `x-signature` 的**签名 URL**，实测改动 resize 模板
 *             (`:360:`→`:320:`/`:400:`) 或去掉模板都会 403。签名绑定了尺寸，原样。
 */
export function thumbUrl(platform: string, cover: string, w = 400): string {
  const raw = (cover || "").trim();
  if (!raw) return "";

  // 统一成绝对 https（协议相对地址补 https）；非 http 资源不处理
  const u = raw.startsWith("//") ? "https:" + raw : raw;
  if (!/^https?:\/\//i.test(u)) return raw;

  const h = Math.round((w * 9) / 16); // 16:9，和移动端卡片比例一致

  switch (platform) {
    case "bilibili": {
      // 只处理 hdslb 图床，且没带过处理后缀（带了就说明后端/别处已处理，别叠加）
      if (!/(^|\.)hdslb\.com\//i.test(u) || u.includes("@")) return u;
      return `${u}@${w}w_${h}h_1c.jpg`;
    }

    case "douyu":
      // rs16 已是最小档 /dy1，无安全缩放参数
      return u;

    case "huya": {
      if (!/(^|\.)msstatic\.com\//i.test(u)) return u;
      const sep = u.includes("?") ? "&" : "?";
      return `${u}${sep}x-oss-process=image/resize,w_${w}`;
    }

    case "douyin":
      // 签名 URL，改尺寸会 403
      return u;

    case "soop": {
      const m = u.match(/^(https?:\/\/liveimg\.sooplive\.com)\/[a-z]+\/(.+)$/i);
      if (!m) return u;
      const tier = w <= 240 ? "l" : "m";
      return `${m[1]}/${tier}/${m[2]}`;
    }

    case "twitch": {
      if (!/static-cdn\.jtvnw\.net\/previews-ttv\//i.test(u)) return u;
      return u.replace(/-(\d+)x(\d+)\.jpg/i, `-${w}x${h}.jpg`);
    }

    case "youtube": {
      const m = u.match(/^(.*\/vi[_a-z]*\/[^/]+\/)[^/?]+\.(jpg|webp)(\?.*)?$/i);
      if (!m) return u;
      // mqdefault 恒为 320x180 真 16:9；hqdefault(480x360) 是 4:3 带黑边，
      // 在 16:9 卡片里会露黑边，所以宽度够也优先用 mq。
      const spec = w <= 120 ? "default" : w <= 480 ? "mqdefault" : "hqdefault";
      return `${m[1]}${spec}.jpg${m[3] || ""}`;
    }

    default:
      return u;
  }
}

/** 打开 YouTube 登录窗口；登录成功后后端会自动保存 Cookie */
export const youtubeLogin = () => invoke<string>("youtube_login");

/**
 * 安卓沉浸式全屏：隐藏系统状态栏与导航栏。
 *
 * 为什么需要它：tao 在安卓上把 `set_fullscreen` 实现成了**空函数**
 * （tao-0.37.1/src/platform_impl/android/mod.rs:823 只打了句
 * "Cannot set fullscreen on Android"），Tauri 2 也没有任何隐藏系统栏的 API。
 * 所以这一层必须自己经 JNI 调 Android 的 WindowInsetsController。
 *
 * 返回值里的 `applied` 如实反映有没有生效（老系统会 false），
 * 不要当成一定成功。
 */
export async function androidImmersive(enable: boolean): Promise<{ applied: boolean; detail: string }> {
  return invoke<{ applied: boolean; detail: string }>("android_immersive", { enable });
}
