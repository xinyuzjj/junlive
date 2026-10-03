<script setup lang="ts">
/**
 * 设置页。
 *
 * 原则：**只留真正会写进后端 / 影响行为的设置**。
 * 之前这里有三张纯展示的卡片（本地流代理端口、支持平台表、致谢说明），
 * 既不可操作也不影响任何行为，已删除。
 *
 * 现在四组：
 *   1. 代理        —— 海外平台（YouTube / Twitch / SOOP）走这里
 *   2. 弹幕        —— 全局默认，改完立刻生效（和播放器里的「弹」按钮同一份数据）
 *   3. 账号 Cookie —— YouTube（含一键登录）、B站（弹幕风控用）
 *   4. 数据管理    —— 清空关注 / 清空 Cookie
 */
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  getBilibiliCookie,
  getProxySetting,
  getYoutubeCookie,
  setBilibiliCookie,
  setProxySetting,
  setYoutubeCookie,
  youtubeLogin,
} from "../api";
import { store } from "../store";
import { soopMode } from "../soopMode";
import {
  DM_COLOR_MODES,
  dmArea,
  dmBlock,
  dmColorMode,
  dmCustom,
  dmOn,
  dmOpacity,
  dmSize,
  dmSpeed,
} from "../dmSettings";

const VERSION = "1.0.1";

const router = useRouter();

/** 预览用的颜色（auto 模式平台各不同，给个中性示意） */
const previewColor = computed(() => {
  if (dmColorMode.value === "white") return "#ffffff";
  if (dmColorMode.value === "custom") return dmCustom.value || "#ffffff";
  return "#7ec8ff";
});

/* ---------------- 代理 ---------------- */
const proxy = ref("");
const proxyMsg = ref("");
/** 当前是否配了代理（留空 = 跟随系统代理） */
const proxyOn = computed(() => !!proxy.value.trim());

async function saveProxy() {
  await setProxySetting(proxy.value.trim() || null);
  proxy.value = (await getProxySetting()) || "";
  proxyMsg.value = proxyOn.value
    ? "已保存，海外平台即刻生效"
    : "已清空，将自动使用系统代理";
  setTimeout(() => (proxyMsg.value = ""), 3000);
}

async function autoProxy() {
  await setProxySetting(null);
  proxy.value = (await getProxySetting()) || "";
  proxyMsg.value = proxy.value
    ? `已探测到系统代理：${proxy.value}`
    : "没有探测到系统代理（国内平台不受影响，海外平台需手动填）";
  setTimeout(() => (proxyMsg.value = ""), 4500);
}

/* ---------------- 弹幕 ---------------- */
/** 和播放器共用同一份设置，改完立刻生效（dmSettings 里的 watch 已落盘） */
const dmMsg = ref("");
function resetDm() {
  dmOn.value = true;
  dmOpacity.value = 0.9;
  dmSize.value = 20;
  dmSpeed.value = 9;
  dmArea.value = 0.5;
  dmBlock.value = "";
  dmColorMode.value = "auto";
  dmCustom.value = "#ffd400";
  dmMsg.value = "已恢复默认";
  setTimeout(() => (dmMsg.value = ""), 2500);
}

/* ---------------- Cookie ---------------- */
const ytCookie = ref("");
const ytMsg = ref("");
const ytLogging = ref(false);
const biliCookie = ref("");
const biliMsg = ref("");

onMounted(async () => {
  proxy.value = (await getProxySetting()) || "";
  ytCookie.value = (await getYoutubeCookie()) || "";
  biliCookie.value = (await getBilibiliCookie()) || "";
});

async function loginYt() {
  ytLogging.value = true;
  ytMsg.value = "";
  try {
    ytMsg.value = await youtubeLogin();
    ytCookie.value = (await getYoutubeCookie()) || "";
  } catch (e) {
    ytMsg.value = `登录未完成：${e}`;
  } finally {
    ytLogging.value = false;
  }
}

async function saveYt() {
  await setYoutubeCookie(ytCookie.value.trim() || null);
  ytCookie.value = (await getYoutubeCookie()) || "";
  ytMsg.value = ytCookie.value ? "已保存" : "已清空";
  setTimeout(() => (ytMsg.value = ""), 3000);
}

async function saveBili() {
  await setBilibiliCookie(biliCookie.value.trim() || null);
  biliCookie.value = (await getBilibiliCookie()) || "";
  biliMsg.value = biliCookie.value
    ? "已保存，进入直播间会重连弹幕"
    : "已清空，恢复匿名访问";
  setTimeout(() => (biliMsg.value = ""), 3000);
}

/* ---------------- 数据管理 ---------------- */
const dataMsg = ref("");
function clearFollows() {
  const n = store.follows.length;
  if (!n) {
    dataMsg.value = "关注列表已经是空的";
  } else {
    store.clearFollows();
    dataMsg.value = `已清空 ${n} 个关注`;
  }
  setTimeout(() => (dataMsg.value = ""), 3000);
}

async function clearCookies() {
  await setYoutubeCookie(null);
  await setBilibiliCookie(null);
  ytCookie.value = "";
  biliCookie.value = "";
  dataMsg.value = "已清空 YouTube 和 B站 Cookie";
  setTimeout(() => (dataMsg.value = ""), 3000);
}
</script>

<template>
  <div class="settings">
    <h2>设置</h2>

    <!-- 1. 代理 -->
    <section class="card">
      <div class="card-title">
        代理
        <span class="chip" :class="{ on: proxyOn }">
          {{ proxyOn ? "已配置" : "跟随系统" }}
        </span>
      </div>
      <p class="hint">
        国内平台（B站 / 斗鱼 / 虎牙 / 抖音）直连，不走代理。<br />
        海外平台（YouTube / Twitch / SOOP）走这里配置的代理；留空表示自动使用系统代理。
      </p>
      <div class="row">
        <input v-model="proxy" placeholder="http://127.0.0.1:7897" />
        <button class="primary" @click="saveProxy">保存</button>
        <button @click="autoProxy">自动探测</button>
      </div>
      <div v-if="proxyMsg" class="ok">{{ proxyMsg }}</div>
    </section>

    <!-- 2. 弹幕 -->
    <!-- 2. SOOP 播放方式 -->
    <section class="card">
      <div class="card-title">
        SOOP 播放方式
        <span class="chip" :class="{ on: soopMode === 'native' }">
          {{ soopMode === "native" ? "自建流" : soopMode === "embed" ? "官方播放器" : "官方完整页" }}
        </span>
      </div>
      <p class="hint">
        SOOP 官方没给一个「又干净、画质又能调」的选项，三种方式各有取舍，所以交给你选。<br />
        <b>自建流</b>：画质四档可选（1080p / 720p / 540p / 360p），弹幕由本软件接入，界面完全可控。<br />
        <b>官方播放器</b>：SOOP 的 embed 播放器，最干净 —— 铺满画面、没有官网导航和聊天区。
        但官方把画质调节删掉了（模板里直接注释掉），画质由官方自动决定。<br />
        <b>官方完整播放页</b>：画质菜单是官方的、能选到 1080p，代价是会带出 SOOP 整套网站界面。
      </p>
      <div class="slider">
        <label>
          <span class="k">播放方式</span>
          <span class="colwrap">
            <select v-model="soopMode">
              <option value="native">自建流（画质可调 + 弹幕）</option>
              <option value="embed">官方播放器（最干净，画质官方定）</option>
              <option value="official">官方完整播放页（画质可调，带官网界面）</option>
            </select>
            <span class="pv">切换后重新进入 SOOP 房间生效</span>
          </span>
        </label>
      </div>
    </section>

    <!-- 3. 弹幕 -->
    <section class="card">
      <div class="card-title">弹幕</div>
      <p class="hint">
        全局默认值，进任何直播间都生效；播放器控制栏里的「弹」按钮改的是同一份设置。
      </p>

      <label class="sw">
        <input v-model="dmOn" type="checkbox" />
        <span>显示弹幕</span>
      </label>

      <div class="slider" :class="{ dim: !dmOn }">
        <label>
          <span class="k">不透明度</span>
          <input v-model.number="dmOpacity" type="range" min="0.2" max="1" step="0.05" />
          <span class="v">{{ Math.round(dmOpacity * 100) }}%</span>
        </label>
        <label>
          <span class="k">颜色</span>
          <span class="colwrap">
            <select v-model="dmColorMode">
              <option v-for="m in DM_COLOR_MODES" :key="m.id" :value="m.id">
                {{ m.name }}
              </option>
            </select>
            <input
              v-if="dmColorMode === 'custom'"
              v-model="dmCustom"
              class="pick"
              type="color"
              title="选择弹幕颜色"
            />
            <span class="pv" :style="{ color: previewColor }">弹幕效果</span>
          </span>
        </label>
        <label>
          <span class="k">字号</span>
          <input v-model.number="dmSize" type="range" min="12" max="36" step="1" />
          <span class="v">{{ dmSize }}px</span>
        </label>
        <label>
          <span class="k">速度</span>
          <input v-model.number="dmSpeed" type="range" min="4" max="20" step="1" />
          <span class="v">{{ dmSpeed }}s</span>
        </label>
        <label>
          <span class="k">显示区域</span>
          <input v-model.number="dmArea" type="range" min="0.2" max="1" step="0.05" />
          <span class="v">上方 {{ Math.round(dmArea * 100) }}%</span>
        </label>
      </div>

      <div class="row" style="margin-top: 10px">
        <input v-model="dmBlock" placeholder="屏蔽词，逗号分隔（如：广告, 关注, 抽奖）" />
        <button @click="resetDm">恢复默认</button>
      </div>
      <div v-if="dmMsg" class="ok">{{ dmMsg }}</div>
    </section>

    <!-- 3. 账号 Cookie -->
    <section class="card">
      <div class="card-title">账号 Cookie</div>

      <div class="sub">
        <div class="sub-head">
          <span>YouTube</span>
          <span class="chip" :class="{ on: !!ytCookie }">
            {{ ytCookie ? "已登录" : "未登录" }}
          </span>
        </div>
        <p class="hint">
          弹幕（live_chat）需要它；经常提示「确认你不是机器人」时也靠它。推荐直接点下面的登录窗口。
        </p>
        <div class="row">
          <input v-model="ytCookie" placeholder="粘贴 YouTube Cookie（可留空）" />
          <button class="primary" @click="saveYt">保存</button>
        </div>
        <div class="row" style="margin-top: 8px">
          <button :disabled="ytLogging" @click="loginYt">
            {{
              ytLogging
                ? "等待登录中…（登录后窗口自动关闭）"
                : "打开 YouTube 登录窗口"
            }}
          </button>
        </div>
        <div v-if="ytMsg" class="ok">{{ ytMsg }}</div>
      </div>

      <div class="sub">
        <div class="sub-head">
          <span>B站</span>
          <span class="chip" :class="{ on: !!biliCookie }">
            {{ biliCookie ? "已登录" : "未登录" }}
          </span>
        </div>
        <p class="hint">
          弹幕接口 <code>getDanmuInfo</code> 有风控，匿名访问返回 <b>-352</b>
          （表现是「正在连接弹幕…」但一条都不来）。至少要有 <code>buvid3</code>、<code>SESSDATA</code>。
        </p>
        <div class="row">
          <input v-model="biliCookie" placeholder="粘贴 B站 Cookie（可留空）" />
          <button class="primary" @click="saveBili">保存</button>
        </div>
        <div v-if="biliMsg" class="ok">{{ biliMsg }}</div>
      </div>
    </section>

    <!-- 4. 数据管理 -->
    <section class="card">
      <div class="card-title">数据管理</div>
      <p class="hint">清理本地保存的数据，不可撤销。</p>
      <div class="row">
        <button @click="clearFollows">
          清空关注列表（{{ store.follows.length }}）
        </button>
        <button @click="clearCookies">清空全部 Cookie</button>
      </div>
      <div v-if="dataMsg" class="ok">{{ dataMsg }}</div>
    </section>

    <!-- 5. 关于 -->
    <section class="card about">
      <div>
        <b>JunLive</b> v{{ VERSION }} · Tauri 2 + Vue 3 · 本地流代理 + 自建弹幕
      </div>
      <div class="dim">仅供个人学习交流，请遵守各平台的服务条款。</div>
      <div class="row" style="margin-top: 10px">
        <button @click="router.push('/about')">项目介绍 / GitHub</button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.settings {
  height: 100%;
  overflow-y: auto;
  padding: 20px 24px 48px;
  max-width: 1080px;
  margin: 0 auto;
}
h2 {
  margin: 0 0 16px;
  font-size: 17px;
}
.card {
  background: var(--bg-2);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 14px 16px;
  margin-bottom: 14px;
}
.card-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: 600;
  margin-bottom: 8px;
}
.chip {
  font-size: 11px;
  font-weight: 400;
  padding: 1px 8px;
  border-radius: 9px;
  background: var(--chip);
  color: var(--fg-dim);
}
.chip.on {
  background: rgba(0, 200, 83, 0.12);
  color: var(--green);
}
.hint {
  color: var(--fg-dim);
  font-size: 12.5px;
  line-height: 1.7;
  margin: 0 0 10px;
}
.row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.row input {
  flex: 1;
  min-width: 220px;
  max-width: 420px;
}
.ok {
  margin-top: 10px;
  color: var(--green);
  font-size: 12.5px;
}
.hint code {
  background: var(--chip);
  border-radius: 4px;
  padding: 1px 5px;
  font-family: Consolas, monospace;
  font-size: 11.5px;
}

/* 弹幕 */
.sw {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 13px;
  margin-bottom: 10px;
  cursor: pointer;
}
.slider {
  display: flex;
  flex-direction: column;
  gap: 8px;
  transition: opacity 0.15s;
}
.slider.dim {
  opacity: 0.4;
  pointer-events: none;
}
.slider label {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12.5px;
}
.slider .k {
  width: 68px;
  color: var(--fg-2);
  flex: none;
}
.slider input[type="range"] {
  flex: 1;
  max-width: 320px;
  accent-color: var(--accent);
}
.slider .v {
  width: 76px;
  color: var(--fg-dim);
  flex: none;
}
/* 颜色：模式下拉 + 自定义色块 + 实时预览 */
.slider .colwrap {
  display: flex;
  align-items: center;
  gap: 10px;
}
.slider select {
  background: var(--chip);
  border: 1px solid var(--border-2);
  color: var(--fg);
  border-radius: 6px;
  font-size: 12.5px;
  padding: 3px 6px;
}
.slider .pick {
  width: 30px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--border-2);
  border-radius: 6px;
  background: transparent;
  cursor: pointer;
}
.slider .pick::-webkit-color-swatch-wrapper {
  padding: 2px;
}
.slider .pick::-webkit-color-swatch {
  border: none;
  border-radius: 4px;
}
/* 弹幕是压在视频上的，所以预览也给深底，颜色才看得出来 */
.slider .pv {
  padding: 3px 12px;
  border-radius: 6px;
  background: #16181c;
  font-weight: 600;
  font-size: 12.5px;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.9);
}

/* Cookie 子块 */
.sub + .sub {
  margin-top: 14px;
  padding-top: 14px;
  border-top: 1px solid var(--border);
}
.sub-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: 600;
  font-size: 13px;
  margin-bottom: 6px;
}

/* 关于 */
.about {
  background: transparent;
  border: none;
  padding: 4px 2px;
  font-size: 12.5px;
  color: var(--fg-2);
}
.about .dim {
  color: var(--fg-dim);
  margin-top: 4px;
}
</style>
