<script setup lang="ts">
/**
 * 移动端「发现」页 —— 平台总览。
 *
 * 为什么单独开一页，而不是复用顶栏那个平台浮层：
 *   顶栏浮层是「临时切换器」——一屏滚动的纯文字列表，目标是尽快选完走人；
 *   发现页是「先看再选」——大卡片把每个平台的品牌色圆点和当前选中态都摆出来，
 *   本项目支持 7 个平台，多平台用户需要一个能一眼扫完、看清「我现在在哪」的入口。
 *   所以选中态用整卡高亮 + "当前"标签，而不是一个小勾。
 *
 * 切完平台直接回首页：选平台的下一步必然是去看这个平台的内容，
 * 停在发现页会多一次点击。
 */
import { useRouter } from "vue-router";
import { store } from "../store";

const router = useRouter();

function pick(id: string) {
  store.setPlatform(id);
  router.push("/");
}
</script>

<template>
  <div class="md">
    <div class="md-head">平台总览</div>
    <div class="md-hint">点一下即可切换平台并回到首页</div>

    <div class="md-list">
      <button
        v-for="p in store.platforms"
        :key="p.id"
        class="md-card"
        :class="{ on: p.id === store.current }"
        :style="p.id === store.current ? { borderColor: p.color } : {}"
        @click="pick(p.id)"
      >
        <span class="md-dot" :style="{ background: p.color }"></span>
        <span class="md-name">{{ p.name }}</span>
        <span v-if="p.id === store.current" class="md-tag">当前</span>
        <span v-else class="md-go">切换 ›</span>
      </button>
    </div>

    <div v-if="!store.platforms.length" class="md-empty">正在加载平台列表…</div>
  </div>
</template>

<style scoped>
.md {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow-y: auto;
  background: var(--bg);
  padding: 14px 12px calc(16px + env(safe-area-inset-bottom, 0));
}
.md-head {
  font-size: 17px;
  font-weight: 700;
  color: var(--fg);
}
.md-hint {
  margin-top: 4px;
  font-size: 12px;
  color: var(--fg-dim);
}

.md-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 14px;
}
/* 平台大卡片：热区远超 44px，整卡可点，避免手指点不中 */
.md-card {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  min-height: 62px;
  padding: 0 16px;
  border: 1px solid var(--border-2);
  border-radius: 12px;
  background: var(--card);
  color: var(--fg);
  text-align: left;
}
.md-card:active {
  background: var(--chip-hover);
}
/* 当前平台：用品牌色描边 + 浅色底把选中态做足（品牌色是动态的，走内联 style） */
.md-card.on {
  background: var(--brand-soft);
  border-width: 2px;
}
.md-dot {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  flex-shrink: 0;
}
.md-name {
  font-size: 16px;
  font-weight: 600;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.md-tag {
  flex-shrink: 0;
  padding: 2px 10px;
  border-radius: 999px;
  background: var(--brand-soft);
  color: var(--brand);
  font-size: 12px;
  font-weight: 600;
}
.md-go {
  flex-shrink: 0;
  color: var(--fg-dim);
  font-size: 13px;
}
.md-empty {
  margin-top: 20px;
  text-align: center;
  color: var(--fg-dim);
  font-size: 13px;
}
</style>
