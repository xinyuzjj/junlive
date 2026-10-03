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
//! # 现状：暂时做不了，原因如实记录在这里
//!
//! 要在安卓隐藏系统栏只有两条路，两条本项目现在都走不通：
//!
//! **① 纯 Rust 经 JNI 调 `WindowInsetsController`**
//!    Tauri 2 的 `RuntimeHandle::run_on_android_context(|env, activity, webview| …)`
//!    正是干这个的（官方自己在 `tauri/src/plugin/mobile.rs:504` 就用它）。
//!    但拿 handle 的三条路全被封死：
//!      - `AppHandle::runtime_handle` 是**私有字段**（E0616，外部不可访问）；
//!      - `AppHandle::try_current()` 在 Tauri 2 **已移除**；
//!      - `tauri::RuntimeOrDispatch` 位于 `pub(crate) mod sealed`，
//!        外部**无法命名该类型**，连 `if let` 匹配都写不出来。
//!    官方能写 `match RuntimeOrDispatch::…` 是因为它在 tauri 自己的模块内部，
//!    外部应用抄不到 —— 这不是写法问题，是 Tauri 的封装边界。
//!
//! **② 自定义 tauri-plugin + Kotlin**
//!    `PluginApi::register_android_plugin` 是公开的，但 Kotlin 必须经 NDK 编译，
//!    而本机没有 NDK；写错了不是「少个功能」，是**整个 APK 构建失败**。
//!
//! 所以这里**不写任何未经验证的原生代码**，只如实返回「没生效」。
//! 前端拿到 `applied=false` 后不假装成功，CSS 全屏照常工作
//! （`Room.vue` 的 `.mr.full` 已经是 `position:fixed; inset:0`，画面铺满整屏，
//! 我们自己的顶栏与标签栏也已 `v-if` 隐藏）。
//! 唯一剩下的是安卓那一层系统状态栏/导航栏仍会显示 —— 属于 Tauri 的能力边界之外。
//!
//! # 后续怎么做
//!
//! 需要一台有 Android NDK 的机器（或用 CI 加一个能跑安卓构建的任务），
//! 走上面第 ② 条路：写一个最小 Kotlin 插件，用
//! `WindowInsetsControllerCompat.hide(WindowInsetsCompat.Type.systemBars())`
//! 加 `BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE`（边缘滑一下系统栏临时出来、
//! 松手自动回去，不会把用户困在黑边里），**先用 CI 验证编译通过再合进来**。
//!
//! 结论先说清楚：**在有 NDK 之前，这层做不了。** 不塞没验证过的代码进去。

use serde::Serialize;

/// 结果如实回传给前端 —— 不假装成功。
#[derive(Serialize, Default)]
pub struct ImmersiveResult {
    /// 是否真的执行了。当前安卓上恒为 false（见文件头说明）。
    pub applied: bool,
    /// 原因，仅用于日志排查
    pub detail: String,
}

/// 前端调用：进入/退出沉浸式。
///
/// `enable` 当前没有实际效果，但签名保留 —— 将来 Kotlin 插件落地后
/// 直接在这里接上，前端调用点不用改。
#[tauri::command]
pub fn android_immersive(enable: bool) -> ImmersiveResult {
    let _ = enable;
    let r = ImmersiveResult {
        applied: false,
        detail: "Tauri 在安卓上无隐藏系统栏的 API，且 runtime handle 拿不到（私有字段）；\
                 需自定义 Kotlin 插件 + NDK 才能实现。详见 src-tauri/src/immersive.rs 文件头"
            .into(),
    };
    eprintln!("[immersive] 未生效: {}", r.detail);
    r
}
