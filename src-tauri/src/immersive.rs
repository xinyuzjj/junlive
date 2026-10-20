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
//! # 现状：已改为移动插件实现
//!
//! 早期这里是个空壳（恒返回 applied=false），因为当时本机没有 NDK，
//! 也拿不到 runtime handle。现在走官方支持的**移动插件**路线：
//!
//!   - Rust：`plugins/tauri-plugin-immersive` —— `register_android_plugin`
//!   - Kotlin：`ImmersivePlugin.kt` —— `WindowInsetsControllerCompat`
//!     `.hide(WindowInsetsCompat.Type.systemBars())`
//!     + `BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE`（边缘滑一下系统栏临时浮出，
//!     松手自动缩回，不会把用户困在黑边里）
//!
//! 前端调用点（`src/mobile/Room.vue` 的 `androidImmersive`）没有变。

use serde::Serialize;
use tauri::AppHandle;

/// 结果如实回传给前端 —— 不假装成功。
#[derive(Serialize, Default)]
pub struct ImmersiveResult {
    /// 是否真的执行了。当前安卓上恒为 false（见文件头说明）。
    pub applied: bool,
    /// 原因，仅用于日志排查
    pub detail: String,
}

/// 前端调用：进入 / 退出沉浸式。
#[tauri::command]
pub fn android_immersive(app: AppHandle, enable: bool) -> ImmersiveResult {
    let applied = tauri_plugin_immersive::set(&app, enable);
    let detail = if applied {
        String::new()
    } else {
        "非安卓平台（桌面端用 window 全屏即可），或插件未注册".into()
    };
    if !applied {
        eprintln!("[immersive] 未下发: {}", detail);
    }
    ImmersiveResult { applied, detail }
}
