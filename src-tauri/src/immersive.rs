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
    use tauri::Manager;

    /// 真机实测前先看这里对不对。
    /// `setSystemUiVisibility` 那套是 API 30 以下的旧路，
    /// `WindowInsetsController` 是 API 30+ 的新路，两套都调一遍最稳。
    pub fn apply(enable: bool) -> ImmersiveResult {
        let app = match tauri::AppHandle::try_current() {
            Ok(a) => a,
            Err(e) => {
                return ImmersiveResult { applied: false, detail: format!("无 AppHandle: {e}") };
            }
        };
        let handle = match app.runtime().handle().try_into() {
            Ok(h) => h,
            Err(e) => {
                return ImmersiveResult { applied: false, detail: format!("取不到 runtime handle: {e}") };
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        let _ = handle.run_on_android_context(move |env, activity, _webview| {
            let r = (|| -> Result<(), String> {
                // ---- 旧路（API < 30）：Window.setSystemUiVisibility ----
                // IMMERSIVE_STICKY(4096) | FULLSCREEN(4) | HIDE_NAVIGATION(2) | LAYOUT_* (不挡内容)
                #[allow(unused_variables)]
                {
                    let decor = env
                        .call_method(
                            activity,
                            "getWindow",
                            "()Landroid/view/Window;",
                            &[],
                        )
                        .map_err(|e| format!("getWindow 失败: {e}"))?;
                    let win = decor.l()?;

                    if enable {
                        // 0x00000006=IMMERSIVE_STICKY|FULLSCREEN/HIDE_NAVIGATION 组合
                        const FLAGS: i32 = 0x0000_0206 | 0x0000_0400 | 0x0000_0800;
                        env.call_method(
                            win,
                            "setSystemUiVisibility",
                            "(I)V",
                            &[JValue::Int(FLAGS)],
                        )
                        .map_err(|e| format!("setSystemUiVisibility 失败: {e}"))?;
                        // 再加一条：让内容铺到系统栏后面，避免隐藏时页面跳一下
                        let _ = env.call_method(
                            win,
                            "addFlags",
                            "(I)V",
                            &[JValue::Int(0x0000_0400)], // FLAG_LAYOUT_NO_LIMITS
                        );
                    } else {
                        env.call_method(
                            win,
                            "setSystemUiVisibility",
                            "(I)V",
                            &[JValue::Int(0)],
                        )
                        .map_err(|e| format!("恢复 setSystemUiVisibility 失败: {e}"))?;
                        let _ = env.call_method(
                            win,
                            "clearFlags",
                            "(I)V",
                            &[JValue::Int(0x0000_0400)],
                        );
                    }
                }

                // ---- 新路（API >= 30）：WindowInsetsController ----
                // 老系统上 find_class 返回 Err，跳过即可，不影响上面的旧路。
                if let Ok(ic_class) = env.find_class("android/view/WindowInsetsController") {
                    if let Ok(ctrl) =
                        env.call_method(win, "getInsetsController", "()Landroid/view/WindowInsetsController;", &[])
                    {
                        if !ctrl.is_null() {
                            let ctrl = ctrl.l()?;
                            // BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE：沉浸式 —— 从边缘滑一下
                            // 系统栏会临时出来，松手自动回去，不会把用户永久困在黑边里。
                            if enable {
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
                                    // statusBars(1) | navigationBars(2)
                                    &[JValue::Int(1 | 2)],
                                );
                            } else {
                                let _ = env.call_method(
                                    ctrl,
                                    "show",
                                    "(I)V",
                                    &[JValue::Int(1 | 2)],
                                );
                            }
                        }
                    }
                }

                Ok(())
            })();
            let _ = tx.send(r.map_err(|e| e));
        });

        match rx.recv_timeout(std::time::Duration::from_millis(1500)) {
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
    /// 桌面端与 iOS 没有安卓系统栏可隐藏，如实返回 false，
    /// 前端据此跳过 UI 变化，不假装成功。
    pub fn apply(_enable: bool) -> ImmersiveResult {
        ImmersiveResult { applied: false, detail: "非安卓平台".into() }
    }
}

/// 前端调用：进入/退出沉浸式。
#[tauri::command]
pub fn android_immersive(enable: bool) -> ImmersiveResult {
    let r = imp::apply(enable);
    if !r.applied {
        eprintln!("[immersive] 未生效: {}", r.detail);
    }
    r
}
