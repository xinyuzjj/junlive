/**
 * 应用版本号。
 *
 * **不要硬编码**。之前 Settings.vue 和 About.vue 里各写了一个
 * `const VERSION = "1.2.4"`，然后从 1.2.5 一路到 1.3.1 都没人记得同步，
 * 结果界面上一直显示 1.2.4 —— 用户看到「已是最新版本（v1.2.4）」，
 * 而实际装的是 1.3.1。
 *
 * 现在直接问 Tauri 要（它读的是 tauri.conf.json 里的 version，
 * 也就是打包时真正写进程序的那个值），永远不会对不上。
 */
import { ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";

export const appVersion = ref("");

/** 取一次真实版本号（组件 onMounted 里调） */
export async function loadVersion() {
  if (appVersion.value) return;
  try {
    appVersion.value = await getVersion();
  } catch {
    // 非 Tauri 环境（比如浏览器里跑 vite dev）拿不到，给个占位
    appVersion.value = "未知";
  }
}
