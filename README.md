# JunLive 聚合直播

把 **斗鱼 / 虎牙 / 哔哩哔哩 / 抖音 / YouTube / SOOP / Twitch** 七个平台装进同一个窗口的桌面客户端。

基于 Tauri 2 + Vue 3，内置本地流代理和自建弹幕通道，国内四个平台可以直连观看（不需要浏览器、不需要代理）。

[![Release](https://img.shields.io/github/v/release/xinyuzjj/junlive?label=%E4%B8%8B%E8%BD%BD&style=flat-square)](https://github.com/xinyuzjj/junlive/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/xinyuzjj/junlive/release.yml?style=flat-square&label=%E6%9E%84%E5%BB%BA)](https://github.com/xinyuzjj/junlive/actions)

---

## 下载安装

去 **[Releases](https://github.com/xinyuzjj/junlive/releases/latest)** 页面下载对应系统的安装包：

| 系统 | 文件 | 说明 |
|---|---|---|
| **Windows** | `JunLive_x.y.z_x64-setup.exe` | NSIS 安装包，双击安装 |
| | `JunLive_x.y.z_x64_en-US.msi` | MSI 安装包，适合批量部署 |
| **macOS** | `JunLive_x.y.z_aarch64.dmg` | Apple Silicon（M 系列芯片） |
| | `JunLive_x.y.z_x64.dmg` | Intel 芯片 |
| **Linux** | `JunLive_x.y.z_amd64.AppImage` | 免安装，`chmod +x` 后直接运行 |
| | `JunLive_x.y.z_amd64.deb` | Debian / Ubuntu：`sudo dpkg -i xxx.deb` |
| | `JunLive_x.y.z_x86_64.rpm` | Fedora / RHEL：`sudo rpm -i xxx.rpm` |

> **macOS 首次打开提示「无法验证开发者」**：右键点应用 → 打开 → 再点「打开」，
> 或执行 `xattr -dr com.apple.quarantine /Applications/JunLive.app`（未签名应用的正常提示）。

---

## 平台支持现状（均为本机实测）

| 平台 | 分区浏览 | 搜索 | 播放 | 弹幕 | 需要代理 |
|---|---|---|---|---|---|
| 哔哩哔哩 | ✅ | ✅ | ✅ HLS / FLV | ✅ 需 Cookie | — |
| 斗鱼 | ✅ 537 个分区 | ✅ | ✅ HLS / FLV | ⚠️ 视网络 | — |
| 虎牙 | ✅ | ✅ | ✅ FLV（HLS 常 403） | ✅ | — |
| 抖音 | ✅ | 房间号直达¹ | ✅ HLS / FLV | ✅ | — |
| YouTube | ✅ 分类 | ✅ | ✅ 官方播放器² | ✅ 自建 live_chat | 需要 |
| SOOP | ✅ 分类 | ✅ | ✅ 官方播放器² | 暂未实现 | 需要 |
| Twitch | ✅ 游戏分类 | ✅ | ✅ 官方播放器² | ✅ 走代理隧道 | 需要 |

¹ 抖音没有可用的匿名搜索接口（`/aweme/v1/web/live/search/` 一律返回
`{"status_code":2483,"status_msg":"请先登录，再继续搜索吧"}`，是平台硬限制）。
所以搜索框直接输**直播间号**或粘直播间链接。

² YouTube / Twitch / SOOP 走各自的官方播放器（iframe 嵌入）。
原因：YouTube 的分片现在强制 PO token，自建播放器一律 403；
Twitch 用自建播放器清晰度会被 ABR 反复切换。
交给官方播放器处理鉴权、清晰度和重连最稳。

---

## 功能

- **七个平台统一界面**：浅色卡片流，平台一键切换
- **关注列表**：跨平台收藏主播，绿点=直播中 / 灰点=未开播，
  **在线状态持久化**，一进页面顺序就是对的（不用等异步刷新）
- **弹幕**：斗鱼 / 虎牙 / B站 / 抖音 / YouTube / Twitch 六平台自建弹幕通道
  - 可调不透明度 / 字号 / 速度 / 显示区域 / 屏蔽词
  - **颜色可选**：跟随平台 / 纯白 / 自定义取色
  - 只显示进入直播间之后的消息
  - 高频房间用 `requestAnimationFrame` 批量合并渲染，走 GPU 合成，不拖累视频
- **窗口全屏**：CSS 全屏铺满应用窗口（不动系统窗口、不与 iframe 播放器抢全屏，弹幕层保留）
- **自绘标题栏**：无边框窗口 + 自定义最小化 / 最大化 / 关闭
- **线路切换**：同一画质下多 CDN 线路可选，实时测速

---

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面外壳 | Tauri 2（Rust） |
| 前端 | Vue 3 + Vite + TypeScript |
| 播放内核 | hls.js（HLS）+ mpegts.js（FLV） |
| 后端 | Rust：reqwest + axum + tokio |
| 流代理 | 内置 axum 本地 HTTP 服务器 |

**为什么需要内置流代理**：国内平台的流都带防盗链（Referer / UA / Cookie），
浏览器里 hls.js 没法自定义这些请求头，还有跨域限制。
所以每个上游地址都在本地注册成一个短 id，前端只播 `http://127.0.0.1:<port>/s/<id>`，
由 Rust 侧带上正确请求头回源；m3u8 播放列表逐行重写，分片地址同样走代理。

---

## 从源码构建

```bash
# 依赖：Node 20+ / Rust stable / 各平台的 Tauri 系统依赖
# https://tauri.app/start/prerequisites/

npm install
npm run tauri dev      # 开发模式（Vite + Rust，自动开窗口）
npm run tauri build    # 打包当前系统的安装包
```

产物在 `src-tauri/target/release/bundle/`。

### 命令行自测（不开 GUI，验证接口通不通）

```bash
cd src-tauri && cargo build

./target/debug/junlive.exe --test platforms              # 平台列表
./target/debug/junlive.exe --test categories bilibili     # 分区
./target/debug/junlive.exe --test rooms douyu 1_1         # 分区下的房间
./target/debug/junlive.exe --test search huya 英雄联盟     # 搜索
./target/debug/junlive.exe --test room twitch yuyuta0702  # 房间详情 + 播放地址
./target/debug/junlive.exe --test danmaku huya 825290 20  # 实测拉 20 秒弹幕

# 端到端：起本地代理、拉 m3u8、再实测拉一个分片
./target/debug/junlive.exe --test serve bilibili 26406842
```

### 自动发布

推一个 tag 会触发 [`.github/workflows/release.yml`](.github/workflows/release.yml)，
自动构建三平台安装包并创建 Release：

```bash
git tag v0.1.0 && git push origin v0.1.0
```

---

## 设置项

应用内「设置」页：

1. **代理**：国内四平台直连不走代理；YouTube / Twitch / SOOP 走这里配置的代理。
   留空 = 自动探测系统代理。
2. **弹幕**：全局默认值（开关 / 不透明度 / 颜色 / 字号 / 速度 / 区域 / 屏蔽词），
   和播放器控制栏里的「弹」按钮是同一份设置。
3. **账号 Cookie**：
   - **YouTube**：点「打开 YouTube 登录窗口」直接登录，Cookie 自动保存
   - **B站**：弹幕接口有风控，匿名访问返回 `-352`，需要贴 `buvid3` + `SESSDATA`
4. **数据管理**：清空关注列表 / 清空全部 Cookie

---

## 目录结构

```
junlive/
├── src/                          # Vue 前端
│   ├── api.ts                    # invoke 封装 + 直播间链接解析
│   ├── store.ts                  # 平台 / 关注列表状态
│   ├── dmSettings.ts             # 弹幕设置（全局单例 + localStorage）
│   ├── embed.ts                  # 官方播放器嵌入地址（YouTube / Twitch / SOOP）
│   ├── components/
│   │   ├── Player.vue            # hls.js / mpegts.js 播放器 + 弹幕飞屏
│   │   └── DmSet.vue             # 弹幕设置面板
│   └── views/
│       ├── Home.vue              # 分区 + 房间网格 + 左侧关注栏
│       ├── Room.vue              # 播放页（多线路 + 弹幕）
│       ├── Follow.vue            # 关注列表（JSON 导入导出）
│       └── Settings.vue
└── src-tauri/
    ├── Cargo.toml
    ├── rust-toolchain.toml       # 固定 GNU 工具链（原因见下）
    └── src/
        ├── lib.rs                # Tauri commands + CLI 自测
        ├── model.rs              # 统一数据模型
        ├── net.rs                # HTTP 客户端（直连 / 走代理 / 回源）
        ├── proxy.rs              # 本地流代理（m3u8 重写 + 分片透传）
        ├── danmaku/              # 六个平台的弹幕通道
        │   ├── mod.rs
        │   ├── bilibili.rs       # WBI 签名 + getDanmuInfo
        │   ├── douyu.rs          # 自有 TCP 协议
        │   ├── huya.rs           # 自有 WS 协议（uid 取房间页 lp）
        │   ├── douyin.rs         # X-Bogus 签名（QuickJS）+ WS
        │   ├── twitch.rs         # IRC over WS（走代理隧道）
        │   └── youtube.rs        # live_chat 长轮询
        └── platforms/            # 七个平台的实现
            ├── bilibili.rs
            ├── douyu.rs          # 含 getEncryption + auth 签名（移植自 streamlink）
            ├── huya.rs           # anti_code 签名
            ├── douyin.rs         # a_bogus 签名（纯 Rust）
            ├── youtube.rs
            ├── soop.rs
            └── twitch.rs         # 匿名 GQL
```

---

## 编译环境说明（Windows 特有）

如果本机没装 Windows SDK（`C:\Program Files (x86)\Windows Kits\10\Lib` 为空），
MSVC 工具链链接时会报 `LNK1181: cannot open input file 'kernel32.lib'`。
项目用 `src-tauri/rust-toolchain.toml` 固定了 **`stable-x86_64-pc-windows-gnu`** 工具链来绕开。

想切回 MSVC：

1. 安装 Windows SDK（VS Installer → 修改 → 单个组件 → Windows 11 SDK）
2. 删掉 `src-tauri/rust-toolchain.toml`
3. 跑 `cargo build` 前先执行 `vcvars64.bat`
   （Git-Bash 里必须经由 cmd 调用，否则会误用 MSYS 自带的 `link` 导致 `link: extra operand`）

> CI（GitHub Actions）上用 `RUSTUP_TOOLCHAIN: stable` 环境变量覆盖这个固定值，
> 所以云端构建走的是各平台默认工具链，不受影响。

GNU 工具链下还有一个坑：`crate-type` 不能带 `cdylib`，否则链接报
`export ordinal too large`，所以 Cargo.toml 里是 `["staticlib", "rlib"]`。

---

## 已知限制

- **抖音搜索**：平台限制，只能输房间号（见上）。
- **斗鱼弹幕**：部分网络环境下 8506 端口被中间设备拦截（TCP 能连但应用层零响应），
  代码本身没问题，视网络而定。
- **虎牙 HLS**：`anti_code` 拼出来的 HLS 地址常返回 403，默认走 FLV（实测稳定）。
- **SOOP 弹幕**：协议已探明（`wss://chat-<hash>.sooplive.com:9001`，前置网关
  `bridge.sooplive.com`），但 Python ssl 直连报 `SSLEOFError`，暂未实现。
- **B站分区房间列表**：官方 `second/getList` 返回 `-352`（风控），
  改成用分区名调搜索接口，结果基本等价。

---

## 作者

<table>
<tr>
<td width="72"><img src="https://github.com/xinyuzjj.png" width="64" alt="峻峻尼"></td>
<td>

**峻峻尼**（junjunni） —— JunLive 作者 · 全栈 / 直播协议逆向

| 联系方式 | |
|---|---|
| GitHub | [@xinyuzjj](https://github.com/xinyuzjj) |
| 推特 | [@hll404357315674](https://x.com/hll404357315674) |
| 电报 | [@junjunnizxcz](https://t.me/junjunnizxcz) |
| 微信 | junjunnizz |
| QQ | 1742259821 |
| 邮箱 | 1742259821@qq.com |

有问题、想提需求，或者想聊直播协议逆向，欢迎开 issue 或直接联系。

</td>
</tr>
</table>

---

## 致谢

接口抓取思路参考了这些项目：

### [lemon-live](https://github.com/lemonfog/lemon-live) · 聚合形态的起点

聚合直播网站，支持虎牙 / 斗鱼 / 抖音 / 哔哩哔哩，带弹幕。
zrfme-live 的作者原话是「我长期使用 lemon-live 看直播，它用不了之后才有了这个项目」——
本项目是同一类形态的桌面端实现：七个平台放进一个窗口，列表 / 搜索 / 播放 / 弹幕四件事都自己接。

### [pure_live](https://github.com/liuchuancong/pure_live) · 平台内核与能力模型

平台内核、能力模型和播放器行为的主要参考。
本项目 `src-tauri/src/model.rs` 里那套统一数据模型（`Room` / `RoomDetail` / `PlayUrl`）
和「同一画质下多 CDN 线路可选」的播放器行为，就是照这套能力模型设计的。

### [Simple Live](https://github.com/xiaoyaocz/dart_simple_live) · 站点接口抓取思路

站点接口抓取思路参考。各平台的分区树、房间列表、搜索、房间详情、播放地址
这几条链路都顺着它摸过一遍；斗鱼 / 虎牙 / B站 / 抖音的接口这几年改了很多次，
也是靠它的实现对照定位的。

### [douyinLive](https://github.com/jwwsjlm/douyinLive) · 抖音 Webcast 弹幕接入

抖音 Webcast 弹幕接入参考（配套的 [douyinlive-proto](https://github.com/jwwsjlm/douyinlive-proto)
提供 proto 定义）。本项目的抖音弹幕通道 —— wss 长连接 + `PushFrame` / `Response` / `Message`
三层 protobuf 解包 + X-Bogus 签名 + 握手必须带 `ttwid` Cookie —— 就是照这个实现的。

### [Multistream](https://github.com/ilanzgx/multistream) · 桌面形态参考

单窗口聚合多个平台的形态，以及「流从官方播放器加载」的思路
（README 原话：*Streams load from the official players*）。
本项目的 YouTube / Twitch / SOOP 三个平台直接沿用了这个做法。

### [DTV](https://github.com/chen-zeong/DTV) · 关注列表与全屏

关注列表的在线状态持久化做法（把 `liveStatus` 存进关注记录、进页面立刻排好序，
而不是每次临时拉）、窗口全屏（CSS fullscreen 而不是 Fullscreen API）、
以及抖音房间列表的 `a_bogus` 签名实现。

> 上游线索来自 [zrfme-live](https://github.com/shaoyouvip/zrfme-live) 的致谢列表。

---

## 免责声明

仅供个人学习与技术交流，请遵守各平台的服务条款。本项目不存储、不分发任何直播内容。
