/**
 * 弹幕设置（全局单例）。
 *
 * 单独抽出来是因为播放器要在两个地方用同一份设置：
 * - 普通平台：控制栏里的「弹」按钮
 * - YouTube（官方 iframe）：iframe 自带控件，没有自定义控制栏，
 *   要在画面角上浮一个「弹」按钮
 *
 * 存 localStorage，刷新/重启后仍在。
 */
import { ref, watch } from "vue";

export const dmOn = ref(localStorage.getItem("dm_on") !== "0");
export const dmOpacity = ref(Number(localStorage.getItem("dm_opacity") || 0.9));
export const dmSize = ref(Number(localStorage.getItem("dm_size") || 20));
export const dmSpeed = ref(Number(localStorage.getItem("dm_speed") || 9));
/** 显示区域：弹幕只占画面上方这一部分 */
export const dmArea = ref(Number(localStorage.getItem("dm_area") || 0.5));
/** 屏蔽词，逗号分隔 */
export const dmBlock = ref(localStorage.getItem("dm_block") || "");
/**
 * 弹幕颜色模式：
 *   auto   跟随平台（平台带什么色用什么色，默认）
 *   white  统一纯白
 *   custom 统一用 dmCustom 指定的颜色
 */
export const dmColorMode = ref(localStorage.getItem("dm_color_mode") || "auto");
/** 自定义颜色（mode = custom 时生效） */
export const dmCustom = ref(localStorage.getItem("dm_custom") || "#ffd400");

watch(
  [dmOn, dmOpacity, dmSize, dmSpeed, dmArea, dmBlock, dmColorMode, dmCustom],
  () => {
    localStorage.setItem("dm_on", dmOn.value ? "1" : "0");
    localStorage.setItem("dm_opacity", String(dmOpacity.value));
    localStorage.setItem("dm_size", String(dmSize.value));
    localStorage.setItem("dm_speed", String(dmSpeed.value));
    localStorage.setItem("dm_area", String(dmArea.value));
    localStorage.setItem("dm_block", dmBlock.value);
    localStorage.setItem("dm_color_mode", dmColorMode.value);
    localStorage.setItem("dm_custom", dmCustom.value);
  },
);

/** 命中屏蔽词就不显示 */
export function isBlocked(text: string) {
  const kws = dmBlock.value
    .split(/[,，\s]+/)
    .map((k) => k.trim())
    .filter(Boolean);
  return kws.some((k) => text.includes(k));
}

/**
 * 太暗的颜色在视频上根本看不清，统一换成白色。
 *
 * 斗鱼的 `col` 有时是**索引值**（1/2/3）而不是 RGB，直接当颜色用会得到接近黑色，
 * 表现就是「黑色的弹幕」。这里用感知亮度（BT.601）兜一层，所有平台都受益。
 */
export function readable(color: string): string {
  const m = /^#([0-9a-fA-F]{6})$/.exec(color || "");
  if (!m) return "#ffffff";
  const n = parseInt(m[1], 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  return (299 * r + 587 * g + 114 * b) / 1000 >= 60 ? color : "#ffffff";
}

/**
 * 一条弹幕最终显示的颜色。
 *   auto   用平台给的色（过暗兜底成白色）
 *   white  统一纯白
 *   custom 统一用自定义色
 */
export function dmPickColor(platformColor: string): string {
  if (dmColorMode.value === "white") return "#ffffff";
  if (dmColorMode.value === "custom") return dmCustom.value || "#ffffff";
  return readable(platformColor);
}

/** 颜色模式的可选项（设置页和播放器面板共用） */
export const DM_COLOR_MODES: { id: string; name: string }[] = [
  { id: "auto", name: "跟随平台" },
  { id: "white", name: "纯白" },
  { id: "custom", name: "自定义" },
];
