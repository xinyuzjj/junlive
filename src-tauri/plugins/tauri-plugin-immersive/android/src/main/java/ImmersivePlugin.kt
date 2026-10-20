package com.junlive.immersive

import android.app.Activity
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class SetImmersiveArgs {
    var enable: Boolean = false
}

/**
 * 隐藏 / 恢复安卓的系统状态栏与导航栏。
 *
 * BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE：从边缘滑一下，系统栏临时浮出来，
 * 松手自动缩回去 —— 用户不会被困在「没有返回手势」的黑屏里。
 */
@TauriPlugin
class ImmersivePlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun setImmersive(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SetImmersiveArgs::class.java)
            activity.runOnUiThread {
                val window = activity.window
                if (window == null) {
                    invoke.reject("拿不到 window")
                    return@runOnUiThread
                }
                val controller = WindowInsetsControllerCompat(window, window.decorView)
                if (args.enable) {
                    controller.systemBarsBehavior =
                        WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                    controller.hide(WindowInsetsCompat.Type.systemBars())
                } else {
                    controller.show(WindowInsetsCompat.Type.systemBars())
                }
                invoke.resolve()
            }
        } catch (ex: Exception) {
            invoke.reject(ex.message)
        }
    }
}
