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

const VERSION = "0.1.0";
const REPO = "https://github.com/xinyuzjj/junlive";
const RELEASES = `${REPO}/releases/latest`;
const ISSUES = `${REPO}/issues`;

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
  { name: "SOOP", color: "#0f7fff", area: "✅ 分类", search: "✅", play: "✅ 官方播放器", dm: "—", proxy: "需要" },
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
  ["Multistream", "桌面形态与「交给官方播放器」的思路", "https://github.com/ilanzgx/multistream"],
  ["DTV", "关注列表在线状态持久化、窗口全屏", "https://github.com/chen-zeong/DTV"],
  ["zrfme-live", "接口抓取思路索引", "https://github.com/shaoyouvip/zrfme-live"],
];
</script>

<template>
  <div class="about">
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
        YouTube / Twitch / SOOP 走各自的官方播放器：YouTube 分片强制 PO token，
        自建播放器一律 403；Twitch 自建播放器清晰度会被 ABR 反复切换。
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
      <div v-for="[n, d, u] in CREDITS" :key="n" class="credit">
        <a @click="open(u)">{{ n }}</a>
        <span class="cd">{{ d }}</span>
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

/* 致谢 */
.credit {
  display: flex;
  align-items: baseline;
  gap: 10px;
  padding: 6px 0;
  font-size: 12.5px;
}
.credit a {
  color: var(--accent);
  cursor: pointer;
  font-weight: 600;
  flex: none;
}
.credit a:hover {
  text-decoration: underline;
}
.cd {
  color: var(--fg-dim);
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
