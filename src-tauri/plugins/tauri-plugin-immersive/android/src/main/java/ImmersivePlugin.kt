package com.junlive.immersive

import android.app.Activity
import android.content.pm.ActivityInfo
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

@InvokeArg
class SetOrientationArgs {
    // landscape / portrait / auto
    var orientation: String = "auto"
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

    /**
     * 强制屏幕方向。
     *
     * 为什么不用 Web 的 screen.orientation.lock()：安卓 WebView 上它经常直接失败
     * （非全屏元素、或系统禁止），结果就是「用户点了全屏，画面还是竖的，
     * 只能一直提示他自己转手机」。
     *
     * SENSOR_LANDSCAPE / SENSOR_PORTRAIT 而不是 LANDSCAPE / PORTRAIT：
     * 允许正反两种握持方向，用户怎么拿都行。
     */
    @Command
    fun setOrientation(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SetOrientationArgs::class.java)
            activity.runOnUiThread {
                activity.requestedOrientation = when (args.orientation) {
                    "landscape" -> ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
                    "portrait" -> ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT
                    else -> ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
                }
                invoke.resolve()
            }
        } catch (ex: Exception) {
            invoke.reject(ex.message)
        }
    }
}
