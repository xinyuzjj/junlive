<script setup lang="ts">
/**
 * 弹幕设置按钮 + 面板。
 *
 * 普通平台放在播放器控制栏里；YouTube 走官方 iframe、没有自定义控制栏，
 * 由父组件浮在画面角上。两处共用同一份设置（见 ../dmSettings）。
 */
import { ref } from "vue";
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

/** drop=true 时面板向下展开（用于按钮贴在画面顶部的场景，否则会被裁掉） */
defineProps<{ drop?: boolean }>();

const show = ref(false);
</script>

<template>
  <div class="dmset">
    <button
      class="ico"
      :class="{ dim: !dmOn }"
      title="弹幕设置"
      @click.stop="show = !show"
    >
      弹
    </button>
    <div v-if="show" class="dmset-menu" :class="{ down: drop }" @click.stop>
      <label class="row">
        <span>显示弹幕</span>
        <input v-model="dmOn" type="checkbox" />
      </label>
      <label class="row">
        <span>不透明度</span>
        <input v-model.number="dmOpacity" type="range" min="0.2" max="1" step="0.05" />
      </label>
      <label class="row">
        <span>颜色</span>
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
        </span>
      </label>
      <label class="row">
        <span>字号 {{ dmSize }}</span>
        <input v-model.number="dmSize" type="range" min="12" max="34" step="1" />
      </label>
      <label class="row">
        <span>速度 {{ dmSpeed }}s</span>
        <input v-model.number="dmSpeed" type="range" min="4" max="18" step="1" />
      </label>
      <label class="row">
        <span>显示区域</span>
        <select v-model.number="dmArea">
          <option :value="0.25">1/4</option>
          <option :value="0.5">1/2</option>
          <option :value="0.75">3/4</option>
          <option :value="1">全屏</option>
        </select>
      </label>
      <label class="row col">
        <span>屏蔽词</span>
        <input v-model="dmBlock" placeholder="逗号分隔，命中不显示" />
      </label>
    </div>
  </div>
</template>

<style scoped>
.dmset {
  position: relative;
}
.ico {
  background: transparent;
  border: none;
  color: #fff;
  font-size: 14px;
  padding: 4px 6px;
  line-height: 1;
  opacity: 0.9;
  cursor: pointer;
}
.ico:hover {
  opacity: 1;
  background: rgba(255, 255, 255, 0.12);
  border-radius: 5px;
}
.ico.dim {
  opacity: 0.45;
}
.dmset-menu {
  position: absolute;
  right: 0;
  bottom: 32px;
  width: 210px;
  background: rgba(22, 24, 28, 0.96);
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 8px;
  padding: 10px 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
  z-index: 11;
}
/* 按钮在画面顶部时向下展开，否则面板会弹到 .player 的 overflow 外面被裁掉 */
.dmset-menu.down {
  bottom: auto;
  top: 32px;
}
.dmset-menu .row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.85);
  padding: 5px 0;
}
.dmset-menu .row input[type="range"] {
  width: 100px;
}
.dmset-menu .row input[type="checkbox"] {
  width: 16px;
  height: 16px;
}
.dmset-menu .row select {
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(255, 255, 255, 0.18);
  color: #fff;
  border-radius: 4px;
  font-size: 12px;
  padding: 2px 4px;
}
/* 颜色：模式下拉 + 自定义色块 */
.dmset-menu .colwrap {
  display: flex;
  align-items: center;
  gap: 6px;
}
.dmset-menu .pick {
  width: 22px;
  height: 20px;
  padding: 0;
  border: 1px solid rgba(255, 255, 255, 0.25);
  border-radius: 4px;
  background: transparent;
  cursor: pointer;
}
.dmset-menu .pick::-webkit-color-swatch-wrapper {
  padding: 2px;
}
.dmset-menu .pick::-webkit-color-swatch {
  border: none;
  border-radius: 2px;
}
.dmset-menu .row.col {
  display: block;
}
.dmset-menu .row.col span {
  display: block;
  margin-bottom: 4px;
}
.dmset-menu .row.col input {
  width: 100%;
  box-sizing: border-box;
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(255, 255, 255, 0.18);
  color: #fff;
  border-radius: 4px;
  font-size: 12px;
  padding: 4px 6px;
}
</style>
