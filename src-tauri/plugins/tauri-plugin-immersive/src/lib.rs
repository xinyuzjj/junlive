//! 隐藏安卓系统状态栏与导航栏（沉浸式全屏）。
//!
//! # 为什么需要插件
//!
//! tao 在安卓上把 `set_fullscreen` 写成了空实现（只打一条 warn），
//! Tauri 2 也没有隐藏系统栏的 API，所以要自己写一层 Kotlin。
//!
//! 为什么不能用「纯 Rust 经 JNI 调」：拿 runtime handle 的三条路都被封死
//! ——`AppHandle::runtime_handle` 是私有字段、`try_current()` 已移除、
//! `RuntimeOrDispatch` 在 `pub(crate) mod sealed` 里无法命名。
//! 所以只能走官方支持的移动插件路线：Rust 侧注册，Kotlin 侧实现。

use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime};

#[derive(serde::Deserialize)]
struct SetImmersiveArgs {
    enable: bool,
}

/// 安卓插件句柄。非安卓平台不会注册，`try_state` 取不到。
pub struct ImmersiveHandle<R: Runtime>(PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::<R>::new("immersive")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                let handle = api
                    .register_android_plugin("com.junlive.immersive", "ImmersivePlugin")?;
                app.manage(ImmersiveHandle(handle));
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = (app, api);
            }
            Ok(())
        })
        .build()
}

/// 返回 true 表示真的下发了命令。安卓之外恒为 false。
pub fn set<R: Runtime>(app: &AppHandle<R>, enable: bool) -> bool {
    #[cfg(target_os = "android")]
    {
        if let Some(h) = app.try_state::<ImmersiveHandle<R>>() {
            return h
                .0
                .run_mobile_plugin::<()>("setImmersive", SetImmersiveArgs { enable })
                .is_ok();
        }
        false
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, enable);
        false
    }
}
