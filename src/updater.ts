/**
 * 自动更新。
 *
 * 走 Tauri 官方的 updater 插件：检查 GitHub Releases 上的 `latest.json`
 * （CI 打包时由 `createUpdaterArtifacts` 自动生成），
 * 用**内置公钥验签**后再安装 —— 签名对不上就拒绝安装，
 * 所以即使有人往 Releases 传了假包也装不上。
 *
 * 几个设计取舍：
 *
 * 1. **启动后延迟检查，不弹窗**。看直播时被更新弹窗打断很烦，
 *    所以只在标题栏挂一个不显眼的小点，用户点开设置才看到详情。
 *
 * 2. **下载和安装分开**。用户点了「下载」之后可以继续看直播，
 *    下完了才提示「重启生效」—— 而不是下完自动把应用重启掉。
 *
 * 3. **Android 直接跳过**。APK 是侧载安装的，没有应用商店的更新通道，
 *    Tauri 的 updater 在 Android 上也不可用，硬调会报错。
 */
import { ref } from "vue";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateState =
  | "idle" // 还没查过
  | "checking" // 正在查
  | "latest" // 已是最新
  | "available" // 有新版本，等用户点下载
  | "downloading" // 下载中
  | "ready" // 下好了，等重启
  | "error";

/** 更新的全局状态（设置页和标题栏都读它） */
export const updateState = ref<UpdateState>("idle");
/** 新版本号，如 "1.2.8" */
export const newVersion = ref("");
/** 更新说明（Release notes，markdown） */
export const updateNotes = ref("");
/** 下载进度 0~100 */
export const updateProgress = ref(0);
/** 出错信息 */
export const updateError = ref("");

/** 当前挂着的 Update 句柄，下载要用它 */
let pending: Update | null = null;

/**
 * Android 上不支持自动更新（APK 侧载，没有商店通道）。
 *
 * 用 UA 判断而不是 import 平台模块 —— 少一个依赖，
 * 而且这里只需要一个布尔值。
 */
const isAndroid = /android/i.test(navigator.userAgent);

/** 有没有可下载的新版本 */
export function hasUpdate() {
  return updateState.value === "available" || updateState.value === "ready";
}

/**
 * 检查更新。
 *
 * @param silent 静默模式：只在**确实有更新**时改变状态。
 *   启动时自动调用用 silent=true —— 没更新就什么都不做，
 *   不要在界面上留下「已是最新」这种没人关心的信息。
 */
export async function checkUpdate(silent = false) {
  if (isAndroid) {
    if (!silent) {
      updateState.value = "error";
      updateError.value = "Android 版请到 GitHub Releases 手动下载 APK 更新";
    }
    return;
  }
  // 已经在下载/待重启就别重入，会把进度打乱
  if (updateState.value === "downloading" || updateState.value === "ready") return;

  if (!silent) {
    updateState.value = "checking";
    updateError.value = "";
  }
  try {
    const u = await check();
    if (u) {
      pending = u;
      newVersion.value = u.version;
      updateNotes.value = u.body ?? "";
      updateState.value = "available";
    } else if (!silent) {
      updateState.value = "latest";
    }
  } catch (e) {
    // 静默检查失败就完全不出声：可能是没网、也可能是 GitHub 不通，
    // 这两种情况用户都不需要知道（他正在看直播）。
    if (!silent) {
      updateState.value = "error";
      updateError.value = String(e);
    }
  }
}

/** 下载并安装更新包 */
export async function downloadUpdate() {
  if (!pending) return;
  updateState.value = "downloading";
  updateProgress.value = 0;
  updateError.value = "";
  try {
    await pending.downloadAndInstall((ev) => {
      switch (ev.event) {
        case "Started":
          updateProgress.value = 0;
          break;
        case "Progress":
          // ev.data 在 Progress 时是 { chunkLength }，
          // 插件不给总大小，所以只能显示「已下载多少」而不是百分比。
          // 这里按累计字节粗算，拿不到 Content-Length 就停在 0。
          break;
        case "Finished":
          updateProgress.value = 100;
          break;
      }
    });
    // Windows 的 passive 模式装完会自动重启，走到这里一般是
    // Linux/AppImage —— 需要用户手动重启
    updateState.value = "ready";
  } catch (e) {
    updateState.value = "error";
    updateError.value = String(e);
  }
}

/** 重启应用让更新生效 */
export async function restartApp() {
  await relaunch();
}

/** 启动后延迟静默检查（给 App.vue 调） */
export function scheduleStartupCheck(delayMs = 5000) {
  if (isAndroid) return;
  window.setTimeout(() => void checkUpdate(true), delayMs);
}
