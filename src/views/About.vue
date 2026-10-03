<script setup lang="ts">
/**
 * GitHub 介绍页。
 *
 * 和「设置」并排放在顶栏右侧。内容对齐仓库 README：
 * 项目简介、下载入口、平台支持表、功能、技术栈、致谢。
 *
 * 外部链接统一走 tauri 的 opener 插件（在系统默认浏览器里打开），
 * 纯浏览器环境（vite dev 直接开）没有插件，退回 window.open。
 */
import { openUrl } from "@tauri-apps/plugin-opener";

const VERSION = "1.1.5";
const REPO = "https://github.com/xinyuzjj/junlive";
const RELEASES = `${REPO}/releases/latest`;
const ISSUES = `${REPO}/issues`;

/** 作者 */
const AUTHOR = {
  name: "峻峻尼",
  alias: "junjunni",
  role: "JunLive 作者 · 全栈 / 直播协议逆向",
  contacts: [
    { k: "GitHub", v: "@xinyuzjj", href: "https://github.com/xinyuzjj" },
    { k: "推特", v: "@hll404357315674", href: "https://x.com/hll404357315674" },
    { k: "电报", v: "@junjunnizxcz", href: "https://t.me/junjunnizxcz" },
    { k: "邮箱", v: "hallozjj@Outlook.com", href: "mailto:hallozjj@Outlook.com" },
  ] as { k: string; v: string; href: string }[],
};

/** 开发方式：全程用 Hermes Agent 完成 */
const HERMES = {
  name: "Hermes Agent",
  url: "https://hermes-agent.nousresearch.com",
  by: "Nous Research",
};

function open(u: string) {
  openUrl(u).catch(() => window.open(u, "_blank"));
}

/** 平台支持矩阵（和 README 保持一致，都是实测结果） */
const PLATFORMS = [
  { name: "哔哩哔哩", color: "#fb7299", area: "✅", search: "✅", play: "✅ HLS/FLV", dm: "✅ 需 Cookie", proxy: "—" },
  { name: "斗鱼", color: "#ff5d23", area: "✅", search: "✅", play: "✅ HLS/FLV", dm: "⚠️ 视网络", proxy: "—" },
  { name: "虎牙", color: "#f5a623", area: "✅", search: "✅", play: "✅ FLV", dm: "✅", proxy: "—" },
  { name: "抖音", color: "#fe2c55", area: "✅", search: "房间号", play: "✅ HLS/FLV", dm: "✅", proxy: "—" },
  { name: "YouTube", color: "#ff0000", area: "✅ 分类", search: "✅", play: "✅ 官方播放器", dm: "✅ 自建", proxy: "需要" },
  { name: "SOOP", color: "#0f7fff", area: "✅ 分类", search: "✅", play: "✅ 自建流 / 官方播放器", dm: "✅ 自建 WebSocket", proxy: "需要" },
  { name: "Twitch", color: "#9146ff", area: "✅ 游戏分类", search: "✅", play: "✅ 官方播放器", dm: "✅ 代理隧道", proxy: "需要" },
];

/** 亮点功能 */
const FEATURES = [
  ["七个平台一个窗口", "浅色卡片流，平台一键切换，国内四平台直连观看"],
  ["自建弹幕通道", "六平台弹幕不走官方 iframe；可调不透明度 / 颜色 / 字号 / 速度 / 区域 / 屏蔽词"],
  ["关注列表", "跨平台收藏，绿点=直播中；在线状态持久化，进页面顺序就是对的"],
  ["本地流代理", "内置 HTTP 代理补 Referer / UA / Cookie，逐行重写 m3u8，绕开跨域"],
  ["窗口全屏", "CSS 全屏铺满应用窗口，不与 iframe 播放器抢全屏，弹幕层保留"],
  ["多线路切换", "同一画质下多 CDN 线路可选，实时测速"],
];

/** 技术栈 */
const STACK = [
  ["桌面外壳", "Tauri 2（Rust）"],
  ["前端", "Vue 3 + Vite + TypeScript"],
  ["播放内核", "hls.js（HLS）+ mpegts.js（FLV）"],
  ["后端", "Rust：reqwest + axum + tokio"],
];

/** 致谢 */
const CREDITS = [
  {
    name: "lemon-live",
    url: "https://github.com/lemonfog/lemon-live",
    desc: "聚合形态的起点",
    detail:
      "聚合直播网站，支持虎牙 / 斗鱼 / 抖音 / 哔哩哔哩，带弹幕。zrfme-live 的作者原话是「我长期使用 lemon-live 看直播，它用不了之后才有了这个项目」—— 本项目是同一类形态的桌面端实现：七个平台放进一个窗口，列表 / 搜索 / 播放 / 弹幕四件事都自己接。",
  },
  {
    name: "pure_live",
    url: "https://github.com/liuchuancong/pure_live",
    desc: "平台内核与能力模型",
    detail:
      "平台内核、能力模型和播放器行为的主要参考。本项目 src-tauri/src/model.rs 里那套统一数据模型（Room / RoomDetail / PlayUrl）和「同一画质下多 CDN 线路可选」的播放器行为，就是照这套能力模型设计的。",
  },
  {
    name: "Simple Live",
    url: "https://github.com/xiaoyaocz/dart_simple_live",
    desc: "站点接口抓取思路",
    detail:
      "站点接口抓取思路参考。各平台的分区树、房间列表、搜索、房间详情、播放地址这几条链路都顺着它摸过一遍；斗鱼 / 虎牙 / B站 / 抖音的接口这几年改了很多次，也是靠它的实现对照定位的。",
  },
  {
    name: "douyinLive",
    url: "https://github.com/jwwsjlm/douyinLive",
    desc: "抖音 Webcast 弹幕接入",
    detail:
      "抖音 Webcast 弹幕接入参考（配套的 douyinlive-proto 提供 proto 定义）。本项目的抖音弹幕通道 —— wss 长连接 + PushFrame / Response / Message 三层 protobuf 解包 + X-Bogus 签名 + 握手必须带 ttwid Cookie —— 就是照这个实现的。",
  },
  {
    name: "Multistream",
    url: "https://github.com/ilanzgx/multistream",
    desc: "桌面形态参考",
    detail:
      "单窗口聚合多个平台的形态，以及「流从官方播放器加载」的思路（README 原话：Streams load from the official players）。本项目的 YouTube / Twitch / SOOP 三个平台直接沿用了这个做法。",
  },
  {
    name: "DTV",
    url: "https://github.com/chen-zeong/DTV",
    desc: "关注列表与全屏",
    detail:
      "关注列表的在线状态持久化做法（把 liveStatus 存进关注记录、进页面立刻排好序，而不是每次临时拉）、窗口全屏（CSS fullscreen 而不是 Fullscreen API）、以及抖音房间列表的 a_bogus 签名实现。",
  },
];
</script>

<template>
  <div class="about">
    <!-- 作者（放在最上面） -->
    <section class="card">
      <div class="card-title">作者</div>
      <div class="who">
        <div class="ava">峻</div>
        <div>
          <div class="nm">
            {{ AUTHOR.name }}
            <span class="alias">{{ AUTHOR.alias }}</span>
          </div>
          <div class="role">{{ AUTHOR.role }}</div>
        </div>
      </div>
      <div class="contact">
        <div v-for="c in AUTHOR.contacts" :key="c.k" class="crow">
          <span class="ck">{{ c.k }}</span>
          <a v-if="c.href" @click="open(c.href)">{{ c.v }}</a>
          <span v-else class="cv">{{ c.v }}</span>
        </div>
      </div>

      <!-- 开发方式 -->
      <div class="built">
        <span class="ck">开发</span>
        <span class="cv">
          本项目全程使用
          <a @click="open(HERMES.url)">{{ HERMES.name }}</a>
          （{{ HERMES.by }}）完成 —— 从平台接口抓取、签名逆向、弹幕通道，
          到前端界面与三平台发布流水线，均由 AI Agent 协作实现。
        </span>
      </div>

      <p class="hint" style="margin: 12px 0 0">
        有问题、想提需求，或者想聊直播协议逆向，欢迎开 issue 或直接联系。
      </p>
    </section>

    <!-- 头部 -->
    <section class="hero">
      <div class="brand">
        <span class="mark">▶</span>
        <div>
          <h1>JunLive</h1>
          <div class="ver">v{{ VERSION }} · 多平台直播聚合桌面客户端</div>
        </div>
      </div>
      <p class="desc">
        把 <b>斗鱼 / 虎牙 / 哔哩哔哩 / 抖音 / YouTube / SOOP / Twitch</b>
        七个平台装进同一个窗口。基于 Tauri 2 + Vue 3，内置本地流代理和自建弹幕通道，
        国内四个平台可以直连观看。
      </p>
      <div class="acts">
        <button class="primary" @click="open(REPO)">打开仓库</button>
        <button @click="open(RELEASES)">下载最新版</button>
        <button @click="open(ISSUES)">反馈问题</button>
      </div>
      <div class="repo mono">{{ REPO }}</div>
    </section>

    <!-- 平台支持 -->
    <section class="card">
      <div class="card-title">平台支持</div>
      <p class="hint">下表都是本机实测结果，不是「理论上支持」。</p>
      <table class="tbl">
        <thead>
          <tr>
            <th>平台</th><th>分区浏览</th><th>搜索</th><th>播放</th><th>弹幕</th><th>需要代理</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in PLATFORMS" :key="p.name">
            <td>
              <span class="dot" :style="{ background: p.color }"></span>{{ p.name }}
            </td>
            <td>{{ p.area }}</td>
            <td>{{ p.search }}</td>
            <td>{{ p.play }}</td>
            <td>{{ p.dm }}</td>
            <td>{{ p.proxy }}</td>
          </tr>
        </tbody>
      </table>
      <p class="hint foot">
        抖音没有可用的匿名搜索接口（平台硬限制），搜索框直接输<b>直播间号</b>或粘直播间链接。<br />
        YouTube / Twitch 走各自的官方播放器：YouTube 分片强制 PO token，
        自建播放器一律 403；Twitch 自建播放器清晰度会被 ABR 反复切换。<br />
        SOOP 比较特殊 —— 官方没给「又干净、画质又能调」的选项，
        所以播放方式做成三档可选（自建流 / 官方播放器 / 官方完整播放页）。
      </p>
    </section>

    <!-- 功能 -->
    <section class="card">
      <div class="card-title">功能</div>
      <div class="feats">
        <div v-for="[t, d] in FEATURES" :key="t" class="feat">
          <div class="ft">{{ t }}</div>
          <div class="fd">{{ d }}</div>
        </div>
      </div>
    </section>

    <!-- 技术栈 -->
    <section class="card">
      <div class="card-title">技术栈</div>
      <table class="tbl">
        <tbody>
          <tr v-for="[k, v] in STACK" :key="k">
            <td class="k">{{ k }}</td>
            <td>{{ v }}</td>
          </tr>
        </tbody>
      </table>
    </section>

    <!-- 致谢 -->
    <section class="card">
      <div class="card-title">致谢</div>
      <p class="hint">接口抓取思路参考了这些项目：</p>
      <div v-for="c in CREDITS" :key="c.name" class="credit">
        <div class="chead">
          <a @click="open(c.url)">{{ c.name }}</a>
          <span class="tag">{{ c.desc }}</span>
        </div>
        <div class="cd">{{ c.detail }}</div>
      </div>
    </section>

    <!-- 免责 -->
    <section class="card disclaim">
      仅供个人学习与技术交流，请遵守各平台的服务条款。本项目不存储、不分发任何直播内容。
    </section>
  </div>
</template>

<style scoped>
.about {
  height: 100%;
  overflow-y: auto;
  padding: 20px 24px 48px;
  max-width: 1080px;
  margin: 0 auto;
}

/* 头部 */
.hero {
  margin-bottom: 16px;
}
.brand {
  display: flex;
  align-items: center;
  gap: 12px;
}
.mark {
  color: var(--brand);
  font-size: 30px;
  line-height: 1;
}
h1 {
  margin: 0;
  font-size: 22px;
  letter-spacing: 0.3px;
}
.ver {
  color: var(--fg-dim);
  font-size: 12.5px;
  margin-top: 3px;
}
.desc {
  color: var(--fg-2);
  font-size: 13px;
  line-height: 1.85;
  margin: 14px 0 14px;
  max-width: 760px;
}
.acts {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.repo {
  margin-top: 10px;
  color: var(--fg-dim);
  font-size: 12px;
}

.card {
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 14px 16px;
  margin-bottom: 14px;
}
.card-title {
  font-weight: 600;
  margin-bottom: 8px;
}
.hint {
  color: var(--fg-dim);
  font-size: 12.5px;
  line-height: 1.75;
  margin: 0 0 10px;
}
.hint.foot {
  margin: 12px 0 0;
}
.hint b {
  color: var(--fg-2);
}

.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 12.5px;
}
.tbl th,
.tbl td {
  text-align: left;
  padding: 8px;
  border-bottom: 1px solid var(--border);
}
.tbl th {
  color: var(--fg-dim);
  font-weight: 500;
}
.tbl tbody tr:last-child td {
  border-bottom: none;
}
.tbl .k {
  color: var(--fg-dim);
  width: 96px;
}
.dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  margin-right: 7px;
  vertical-align: 1px;
}

/* 功能 */
.feats {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 12px;
}
.feat {
  background: var(--chip);
  border-radius: 8px;
  padding: 10px 12px;
}
.ft {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 4px;
}
.fd {
  font-size: 12px;
  color: var(--fg-dim);
  line-height: 1.7;
}

/* 作者 */
.who {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 14px;
}
.ava {
  width: 46px;
  height: 46px;
  border-radius: 50%;
  display: grid;
  place-items: center;
  flex: none;
  background: var(--brand-soft);
  color: var(--brand);
  font-size: 20px;
  font-weight: 700;
}
.nm {
  font-size: 15px;
  font-weight: 600;
}
.alias {
  color: var(--fg-dim);
  font-size: 12px;
  font-weight: 400;
  margin-left: 6px;
}
.role {
  color: var(--fg-dim);
  font-size: 12.5px;
  margin-top: 3px;
}
.contact {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 6px 16px;
}
.crow {
  display: flex;
  align-items: baseline;
  gap: 10px;
  font-size: 12.5px;
}
.ck {
  color: var(--fg-dim);
  width: 46px;
  flex: none;
}
.crow a,
.cv {
  color: var(--fg-2);
}
.crow a {
  color: var(--accent);
  cursor: pointer;
}
.crow a:hover {
  text-decoration: underline;
}

/* 开发方式（用 Hermes Agent 完成） */
.built {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin-top: 12px;
  padding: 10px 12px;
  background: var(--brand-soft);
  border-radius: 8px;
  font-size: 12.5px;
  line-height: 1.8;
  color: var(--fg-2);
}
.built .ck {
  color: var(--fg-dim);
  flex: none;
}
.built a {
  color: var(--accent);
  cursor: pointer;
  font-weight: 600;
}
.built a:hover {
  text-decoration: underline;
}

/* 致谢 */
.credit {
  padding: 10px 0;
  border-top: 1px solid var(--border);
}
.credit:first-of-type {
  border-top: none;
  padding-top: 4px;
}
.chead {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 5px;
}
.chead a {
  color: var(--accent);
  cursor: pointer;
  font-weight: 600;
  font-size: 13px;
}
.chead a:hover {
  text-decoration: underline;
}
.tag {
  font-size: 11px;
  color: var(--fg-dim);
  background: var(--chip);
  padding: 1px 8px;
  border-radius: 9px;
}
.cd {
  color: var(--fg-dim);
  font-size: 12.5px;
  line-height: 1.8;
}

.disclaim {
  background: transparent;
  border: none;
  padding: 4px 2px;
  color: var(--fg-dim);
  font-size: 12.5px;
  line-height: 1.7;
}
</style>
