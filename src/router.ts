import { createRouter, createWebHashHistory } from "vue-router";

/**
 * 移动端路由占位组件。
 *
 * 移动端那几页（首页 / 关注 / 分类 / 我的）的实际渲染由 src/mobile/Shell.vue
 * 按 route.path 自己分发，不走 <router-view>；但这些路径仍必须出现在路由表里，
 * 否则 vue-router 会打印 "No match found" 告警、且 route.matched 为空
 * （一旦以后加了 catch-all 404 就会白屏，靠容错运行不等于对）。
 *
 * 用 render 函数而不是 `template` 字符串：Vite 默认走 Vue 的 runtime-only 构建，
 * 没有模板编译器，写 template 会运行时报错。
 */
const Blank = { render: () => null };

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "home", component: () => import("./views/Home.vue") },
    {
      path: "/room/:platform/:id",
      name: "room",
      component: () => import("./views/Room.vue"),
      props: true,
    },
    {
      path: "/follow",
      name: "follow",
      component: () => import("./views/Follow.vue"),
    },
    {
      path: "/settings",
      name: "settings",
      component: () => import("./views/Settings.vue"),
    },
    {
      path: "/about",
      name: "about",
      component: () => import("./views/About.vue"),
    },

    /* ---- 移动端专属路径：只用于消除 No match 告警，渲染由 Shell.vue 接管 ---- */
    { path: "/classify", name: "classify", component: Blank },
    { path: "/me", name: "me", component: Blank },
    // 旧版「发现」tab 用过的路径，保留注册以兼容历史 URL / 深链
    { path: "/discover", name: "discover", component: Blank },
  ],
});

export default router;
