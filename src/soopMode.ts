/**
 * SOOP 播放方式（用户可自选）。
 *
 * SOOP 是唯一一个「官方播放器和自建流各有明显取舍」的平台，所以交给用户选：
 *
 * - `native`（默认）**自建流**：我们解析直播流，用 hls.js 播，
 *   画质选择器有 1080p / 720p / 540p / 360p 四档，弹幕是我们自己接的
 *   （`danmaku/soop.rs`，WebSocket + `Sec-WebSocket-Protocol: chat`）。
 *   界面完全是我们的，不会被 iframe 吞点击。
 *
 * - `official` **官方完整播放页**：嵌 `https://play.sooplive.com/{主播ID}`（**不带 `/embed`**）。
 *   画质菜单是开的（`1080p화질/자동화질/원본화질/고화질/일반화질/저화질`），能选到 1080p。
 *   代价：会带出 SOOP 整套网站（顶部导航 + 频道信息 + 聊天区），
 *   而且 iframe 会吞掉画面区域的点击。实测 1280 宽时视频只占 816×459。
 *
 * - `embed` **官方播放器控件**：`https://play.sooplive.com/{主播ID}/embed`。
 *   最干净的一种 —— 铺满画面、没有官网导航/聊天区，看着就是「官方播放器本身」。
 *   **但官方把画质调节删掉了**：服务端模板里直接写着
 *   `<!-- 화질선택 임베디드는 미노출 --> <div class="quality_box">`，
 *   该页除了 `fromApi=` 不读任何 URL 参数，外面没法把画质框叫回来。画质交给官方 auto。
 *
 * `embed` / `official` 两种模式下不启动我们的弹幕层（官方页面自带聊天/无弹幕位）。
 */

import { ref, watch } from "vue";

export type SoopMode = "native" | "embed" | "official";

const KEY = "junlive.soop_mode";

function load(): SoopMode {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "native" || v === "embed" || v === "official") return v;
  } catch {
    /* 忽略 */
  }
  return "native";
}

export const soopMode = ref<SoopMode>(load());

watch(soopMode, (v) => {
  try {
    localStorage.setItem(KEY, v);
  } catch {
    /* 忽略 */
  }
});

/** 官方播放器控件（最干净，铺满画面，但画质不可调 —— 官方把画质框注释掉了） */
export function soopEmbedSrc(bjid: string): string {
  return `https://play.sooplive.com/${encodeURIComponent(bjid)}/embed`;
}

/** 官方完整播放页（画质可调，但带整套官网界面） */
export function soopOfficialSrc(bjid: string): string {
  return `https://play.sooplive.com/${encodeURIComponent(bjid)}`;
}
