import { createRouter, createWebHashHistory } from "vue-router";

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
  ],
});

export default router;
