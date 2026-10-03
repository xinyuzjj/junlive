# JunLive v1.1.6

**修复安装版 Twitch 官方播放器无法播放（仅 dev 正常）。**

---

## 🔧 核心修复：安装版 Twitch 官方播放器「拒绝连接」

### 现象

同一个版本，**开发环境（dev）播放正常，打包安装后必然报「player.twitch.tv 拒绝连接」**。

### 根因

Twitch 的官方播放器（`player.twitch.tv`）要求 URL 里的 `parent` 参数必须与嵌入页的**真实域名**一致，不匹配就整页拒绝。而 Tauri 打包后与 dev 的 origin 完全不同：

| 环境 | 页面 origin | 实际传入的 `parent` | Twitch 是否接受 |
|---|---|---|---|
| dev（`npm run tauri dev`） | `http://localhost:1420` | `localhost` | ✅ 接受 |
| 打包安装版 | `http://tauri.localhost` | `tauri.localhost` | ❌ **拒绝** |

**Twitch 的白名单里没有 `tauri.localhost` 这个值。**

### 为什么之前没发现

之前的实现是「把 Tauri 的特殊值原样透传」：

```ts
if (h === "tauri.localhost" || h === "localhost" || h === "127.0.0.1") return h;
```

在 dev 里 `window.location.hostname` 恰好是 `localhost`，所以返回 `localhost`，一切正常 —— **只有打包后才会暴露**。这类「dev 能用、发布后失效」的问题是本项目最难查的一类。

### 修法

```ts
export function parentHost(): string {
  return "localhost";
}
```

无条件返回 `localhost` —— dev 与安装版都指向它，两边都能播。这也是 Tauri 官方为 Twitch embed 推荐的写法。

---

## ✅ 安卓 APK 终于能构建了

前两个版本（v1.1.3 / v1.1.4）的安卓构建都失败了 —— 为了在安卓隐藏系统状态栏而加入的 JNI 代码编译不过：

```
error[E0433]: use of undeclared type `JValue`
error[E0616]: field `runtime_handle` of struct `AppHandle` is private
```

### 三次尝试都失败，根因是 Tauri 的封装边界

要在安卓隐藏系统栏只有两条路，两条都被封死：

1. **纯 Rust 经 JNI 调 `WindowInsetsController`**
   Tauri 的 `RuntimeHandle::run_on_android_context()` 正是干这个的，但拿 handle 只有三条路：
   - `AppHandle::runtime_handle` —— **私有字段**，外部不可访问
   - `AppHandle::try_current()` —— Tauri 2 **已移除**
   - `tauri::RuntimeOrDispatch` —— 位于 `pub(crate) mod sealed`，**外部无法命名该类型**

   官方自己的代码能 `match RuntimeOrDispatch::…`，是因为它在 tauri 自己的模块内部；外部应用抄不到。**这不是写法问题。**

2. **自定义 tauri-plugin + Kotlin**
   `PluginApi::register_android_plugin` 是公开的，配 Kotlin 用
   `WindowInsetsControllerCompat.hide(systemBars())` 可以实现，**但 Kotlin 必须经 NDK 编译**。

### 本版做法：安全降级

删除全部 JNI 代码（含 `jni` 依赖），只保留一个如实返回 `applied=false` 的命令，并把三次失败的原因与后续方案完整写进 `src-tauri/src/immersive.rs` 文件头。`src/` 下现在**没有任何安卓专属代码**，构建不会再因它失败。

**代价说明白**：安卓那一层**系统状态栏/导航栏仍会显示**。CSS 全屏本身是好的（画面铺满整屏、我们自己的顶栏与标签栏已隐藏），390×844 与 844×390 双视口实测通过。剩下的只是 Tauri 能力边界之外那一层 —— 需要有 NDK 的环境才能做，方案已写进代码注释。

---

## 🎮 Twitch 两种播放方式（用户可自选）

| 方式 | 好处 | 代价 |
|---|---|---|
| **官方播放器**（默认） | 更流畅，官方自己管清晰度、鉴权、重连 | 画质被 Twitch **锁在 640×360**（政策，改不了） |
| **自建流** | 画质能到 **1920×1080**，弹幕与画质选择器都是我们自己的 | usher token 有时效（约 1 小时），长时间看要重新进房间 |

切换入口两处：**设置页 → Twitch 播放方式**；**移动端播放页 →「更多」**。

---

## 🔧 其他修复

- **代理持久化** —— 之前代理只存在内存里，**重启应用就丢**（表现为「设置里填了代理也白填」）。现在写入 `%APPDATA%/JunLive/proxy.txt`，启动时读回。
- **删除多余的 WebView 代理注入** —— 查证后确认 **WebView2 默认就读系统代理**，强塞 `--proxy-server` 反而会**覆盖**系统设置，导致「改了系统代理也没用」。
- **国内外分流明确** —— `OVERSEAS_PLATFORMS = [twitch, youtube, soop]` 作为唯一真相源，与 `net.rs` 的 `relay()`/`relay_proxy()` 一致：国内平台直连，海外平台走代理。
- **移动端全屏修复** —— 根元素类名写成了数组形式 `:class="[full, ...]"`，而 **Vue 数组 class 里的 `true` 会被直接忽略**，导致 `.mr.full` 永远匹配不上、全屏样式一条都没生效。已改为对象形式。
- **Twitch 房间号直链支持** —— 输入 Twitch 频道名或链接可直接进房。

---

## 📥 下载

| 平台 | 文件 |
|---|---|
| **Android** | **`.apk`（arm64，Debug 未签名，仅侧载）** |
| Windows | `.exe`（安装器）/ `.msi` |
| macOS（Intel / M 芯片） | `.dmg` |
| Linux | `.AppImage` / `.deb` / `.rpm` |

---

## ⚠️ 已知限制

- **APK 未签名** —— 只能侧载，不能上架应用商店
- **安卓系统栏仍显示**（全屏时）—— 见上文「安全降级」一节
- **iOS 暂不出包** —— 需要 Mac 构建环境与开发者账号
- **Twitch 官方播放器画质锁 640×360** —— Twitch 对 embed 的政策
- **自建流亮度是画面滤镜**，不是系统亮度
