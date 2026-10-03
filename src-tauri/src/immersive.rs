//! 安卓沉浸式全屏：隐藏系统状态栏与导航栏。
//!
//! # 为什么需要这个
//!
//! 用户反馈「手机横屏全屏时顶部还有系统状态栏，看起来像网页被横过来」。
//! 根因在 tao 自己就写明了 —— `tao-0.37.1/src/platform_impl/android/mod.rs:823`：
//!
//! ```ignore
//! pub fn set_fullscreen(&self, _monitor: Option<window::Fullscreen>) {
//!     warn!("Cannot set fullscreen on Android");
//! }
//! ```
//!
//! 也就是 `set_fullscreen` 在安卓上是**空实现**，只打一条警告。
//! Tauri 2 也没有任何隐藏系统栏的 API（`config.schema.json` 的
//! `WindowConfig` 里只有 `fullscreen`/`decorations` 这类通用项）。
//! 所以这一层必须自己做。
//!
//! # 为什么用纯 Rust + JNI，而不是写 Kotlin
//!
//! 写 Kotlin 的话需要 NDK 才能编译验证，环境里没有 NDK，
//! 写错了整个 APK 构建直接失败。
//! 而 Tauri 2 提供了 `RunEvent::RunTimeHandle::run_on_android_context`，
//! 能拿到 `JNIEnv` + Activity + WebView —— 纯 Rust 就能调 Android 平台 API，
//! 不需要碰 Kotlin、不需要 NDK 工具链。
//!
//! # 调的是什么
//!
//! `android.view.WindowInsetsController`（API 30+ 的平台 API，不是 AndroidX）。
//! 之所以不用 `androidx.core.view.WindowInsetsControllerCompat`：
//! 后者需要 AndroidX 在 classpath 上，而本机没有 `src-tauri/gen/android`
//! （CI 才生成），无法确认它在不在；平台 API 则只要 minSdk ≥ 30 就行，
//! 而 Tauri 安卓模板的 minSdk 远高于 30。
//!
//! 类找不到（老系统）时**优雅降级**：记一条日志、返回 applied=false，
//! 前端据此知道「没隐藏成」，CSS 全屏仍然生效，不会白屏或崩。

use serde::Serialize;

// `jni` 只在安卓分支里用，所以只在安卓目标下导入。
// 版本与 tao/wry 拉的严格一致（见 Cargo.toml 里的注释）——jni 的
// JObject/JValue 不保证跨 crate 版本兼容，对不上会在运行时段错误。
#[cfg(target_os = "android")]
use jni::objects::{JObject, JValue};

/// 结果如实回传给前端 —— 不假装成功。
#[derive(Serialize, Default)]
pub struct ImmersiveResult {
    /// 是否真的执行了（false = 非安卓、或系统版本太低、或调用失败）
    pub applied: bool,
    /// 失败原因，仅用于日志排查
    pub detail: String,
}

#[cfg(target_os = "android")]
mod imp {
    use super::ImmersiveResult;
    use jni::objects::{JObject, JValue};
    use tauri::Manager;

    /// 真机实测前先看这里对不对。
    /// `setSystemUiVisibility` 那套是 API 30 以下的旧路，
    /// `WindowInsetsController` 是 API 30+ 的新路，两套都调一遍最稳。
    ///
    /// 用法参照 Tauri 自己的 `src/plugin/mobile.rs:504`：
    /// `handle.runtime_handle.run_on_android_context(|env, activity, _webview| …)`
    pub fn apply(handle: &tauri::AppHandle, enable: bool) -> ImmersiveResult {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = handle.runtime_handle.run_on_android_context(move |env, activity, _webview| {
            let r = (|| -> Result<(), String> {
                // ---- 旧路（API < 30）：Window.setSystemUiVisibility ----
                // IMMERSIVE_STICKY(0x1000) | FULLSCREEN(4) | HIDE_NAVIGATION(2)
                // | LAYOUT_STABLE(0x100) | LAYOUT_HIDE_NAVIGATION(0x200) | LAYOUT_FULLSCREEN(0x400)
                const FLAGS: i32 = 0x1000 | 0x4 | 0x2 | 0x100 | 0x200 | 0x400;
                const LAYOUT_NO_LIMITS: i32 = 0x200; // FLAG_LAYOUT_NO_LIMITS

                let win = env
                    .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])
                    .map_err(|e| format!("getWindow 失败: {e}"))?
                    .l()
                    .map_err(|e| format!("取 Window 对象失败: {e}"))?;

                if enable {
                    env.call_method(
                        win,
                        "setSystemUiVisibility",
                        "(I)V",
                        &[JValue::Int(FLAGS)],
                    )
                    .map_err(|e| format!("setSystemUiVisibility 失败: {e}"))?;
                    // 让内容铺到系统栏后面，避免隐藏瞬间页面跳一下
                    let _ = env.call_method(
                        win,
                        "addFlags",
                        "(I)V",
                        &[JValue::Int(LAYOUT_NO_LIMITS)],
                    );
                } else {
                    env.call_method(win, "setSystemUiVisibility", "(I)V", &[JValue::Int(0)])
                        .map_err(|e| format!("恢复 setSystemUiVisibility 失败: {e}"))?;
                    let _ = env.call_method(
                        win,
                        "clearFlags",
                        "(I)V",
                        &[JValue::Int(LAYOUT_NO_LIMITS)],
                    );
                }

                // ---- 新路（API >= 30）：WindowInsetsController ----
                // 老系统上 find_class 返回 Err，跳过即可，不影响上面的旧路。
                if let Ok(_ic_class) = env.find_class("android/view/WindowInsetsController") {
                    if let Ok(ctrl) = env.call_method(
                        win,
                        "getInsetsController",
                        "()Landroid/view/WindowInsetsController;",
                        &[],
                    ) {
                        if !ctrl.is_null() {
                            let ctrl: JObject = ctrl
                                .l()
                                .map_err(|e| format!("取 WindowInsetsController 失败: {e}"))?;
                            // statusBars(1) | navigationBars(2)
                            const SYSTEM_BARS: i32 = 1 | 2;
                            if enable {
                                // BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE：沉浸式 ——
                                // 从边缘滑一下系统栏会临时出来、松手自动回去，
                                // 不会把用户永久困在黑边里。
                                let _ = env.call_method(
                                    ctrl,
                                    "setSystemBarsBehavior",
                                    "(I)V",
                                    &[JValue::Int(2)],
                                );
                                let _ = env.call_method(
                                    ctrl,
                                    "hide",
                                    "(I)V",
                                    &[JValue::Int(SYSTEM_BARS)],
                                );
                            } else {
                                let _ = env.call_method(
                                    ctrl,
                                    "show",
                                    "(I)V",
                                    &[JValue::Int(SYSTEM_BARS)],
                                );
                            }
                        }
                    }
                }

                Ok(())
            })();
            let _ = tx.send(r);
        });

        match rx.recv_timeout(std::time::Duration::from_millis(2000)) {
            Ok(Ok(())) => ImmersiveResult { applied: true, detail: "ok".into() },
            Ok(Err(e)) => ImmersiveResult { applied: false, detail: e },
            Err(e) => ImmersiveResult {
                applied: false,
                detail: format!("等待 JNI 回调超时/失败: {e}"),
            },
        }
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    use super::ImmersiveResult;
    use tauri::AppHandle;
    /// 桌面端与 iOS 没有安卓系统栏可隐藏，如实返回 false，
    /// 前端据此跳过 UI 变化，不假装成功。
    pub fn apply(_handle: &AppHandle, _enable: bool) -> ImmersiveResult {
        ImmersiveResult { applied: false, detail: "非安卓平台".into() }
    }
}

/// 前端调用：进入/退出沉浸式。
///
/// `handle` 由 Tauri 自动注入 —— 命令函数里只要有 `AppHandle` 类型的参数，
/// Tauri 就会把当前应用的句柄传进来，不用自己去找全局实例。
#[tauri::command]
pub fn android_immersive(handle: tauri::AppHandle, enable: bool) -> ImmersiveResult {
    let r = imp::apply(&handle, enable);
    if !r.applied {
        eprintln!("[immersive] 未生效: {}", r.detail);
    }
    r
}
