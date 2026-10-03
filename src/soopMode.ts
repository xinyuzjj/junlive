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
 * - `official` **官方播放页**：嵌 `https://play.sooplive.com/{主播ID}`（**不带 `/embed`**）。
 *   带 `/embed` 的那个页面官方把画质选择器 CSS 关掉了（HTML 注释
 *   `<!-- 화질선택 임베디드는 미노출 -->`），实测锁死 640x360；
 *   普通播放页的画质菜单是开的（`1080p화질/자동화질/원본화질/고화질/일반화질/저화질`）。
 *   代价：会带出 SOOP 自己的顶部导航栏和频道/聊天区，而且 iframe 会吞掉画面区域的点击。
 *   这个模式下我们的弹幕层不启动（官方页面自带聊天）。
 */

import { ref, watch } from "vue";

export type SoopMode = "native" | "official";

const KEY = "junlive.soop_mode";

function load(): SoopMode {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "official" || v === "native") return v;
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

/** 官方播放页地址：普通页（画质可调），不是 /embed（画质被锁） */
export function soopOfficialSrc(bjid: string): string {
  return `https://play.sooplive.com/${encodeURIComponent(bjid)}`;
}
