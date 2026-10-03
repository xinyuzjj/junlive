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

/** 打开 YouTube 登录窗口；登录成功后后端会自动保存 Cookie */
export const youtubeLogin = () => invoke<string>("youtube_login");
