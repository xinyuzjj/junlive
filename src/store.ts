import { reactive, ref } from "vue";
import type { PlatformInfo, Room } from "./api";

export interface FollowItem {
  platform: string;
  room_id: string;
  streamer: string;
  avatar: string;
  added: number;
  /**
   * 直播状态，**持久化**在关注记录里。
   *
   * 参考 DTV：不要在进页面时临时拉一次就丢 —— 那样刷新出来之前顺序是乱的。
   * 存下来后一进页面顺序就是对的，后台再刷新覆盖。
   *   LIVE    在播
   *   OFFLINE 未开播
   *   UNKNOWN 还没查到（拉失败也算，按 DTV 的规则不能当成「在播」）
   */
  live?: "LIVE" | "OFFLINE" | "UNKNOWN";
}

/** 排序权重：在播最前，未知中间，未开播最后（和 DTV 一致） */
export function liveRank(s?: FollowItem["live"]): number {
  if (s === "LIVE") return 0;
  if (s === "UNKNOWN" || s === undefined) return 1;
  return 2;
}

/** 三档主题：跟随系统 / 浅色 / 深色 —— 对齐 Simple Live 的三选做法（非两态开关） */
export type ThemeMode = "system" | "light" | "dark";

/** 主题新键与旧键（旧键待迁移） */
const THEME_KEY = "junlive.theme";
const LEGACY_DARK_KEY = "junlive.dark";

/**
 * 系统是否偏好深色。由 main.ts 的 matchMedia 监听器写入。
 * 为什么用 ref 而不是每次现场读 window：theme="system" 时，系统切换要能
 * 触发依赖 store.dark 的组件（顶栏按钮图标）重渲染，读 window 不会建立响应式依赖。
 */
export const systemPrefersDark = ref(false);

/**
 * 读取主题并完成旧键迁移（一次性）：
 *   - junlive.theme 已是合法值 → 直接用
 *   - 否则回退读旧键 junlive.dark："1" → dark，"0" → light
 *   - 新旧都没有 → system（跟随系统，最不打扰）
 * 结果立即写回新键、删掉旧键，之后不再依赖旧键。
 */
function loadTheme(): ThemeMode {
  const v = localStorage.getItem(THEME_KEY);
  let t: ThemeMode;
  if (v === "system" || v === "light" || v === "dark") {
    t = v;
  } else {
    const old = localStorage.getItem(LEGACY_DARK_KEY);
    t = old === "1" ? "dark" : old === "0" ? "light" : "system";
  }
  localStorage.setItem(THEME_KEY, t);
  localStorage.removeItem(LEGACY_DARK_KEY);
  return t;
}

const FOLLOW_KEY = "junlive.follows";

function loadFollows(): FollowItem[] {
  try {
    const raw = localStorage.getItem(FOLLOW_KEY);
    return raw ? (JSON.parse(raw) as FollowItem[]) : [];
  } catch {
    return [];
  }
}

export const store = reactive({
  platforms: [] as PlatformInfo[],
  current: localStorage.getItem("junlive.platform") || "bilibili",
  follows: loadFollows(),

  /**
   * 每个平台上次浏览的版块（父分类 + 子分类）。
   *
   * 为什么放在 store：路由切换时 App.vue 的 `<router-view :key="$route.fullPath">`
   * 会把 Home 整个销毁重建，组件里的 activeParent 会被重置成第一个分类 ——
   * 表现就是「看完直播点返回，跳回平台首页，而不是刚才那个版块」。
   * 状态放这里，重建后就能恢复。
   */
  catSel: {} as Record<string, { parent: string; sub: string }>,

  /**
   * 主题模式：跟随系统 / 浅色 / 深色。
   *
   * 为什么把状态放在 store 而不是组件里：顶栏按钮、启动时的主题应用、
   * 刷新后的恢复，三处都要读同一个值，放这里只有一份真相。
   * 持久化键 junlive.theme，启动时由 loadTheme() 完成旧键 junlive.dark 的迁移。
   * 注意：这里**只负责状态与持久化**，真正往 <html> 上挂 class 的动作交给
   * main.ts —— 保持 store 不直接碰 DOM，方便以后在别处复用。
   */
  theme: loadTheme(),

  /**
   * 兼容旧调用：当前是否"实际"处于深色。
   * Shell.vue 仍读 store.dark 决定按钮图标，此 getter 保证它无需改动即可工作：
   * theme=system 时跟随系统偏好（systemPrefersDark 由 main.ts 维护，可响应式更新）。
   */
  get dark(): boolean {
    if (this.theme === "dark") return true;
    if (this.theme === "light") return false;
    return systemPrefersDark.value;
  },

  /** 设置主题并持久化（切换立即由 main.ts 的 watch 应用到 <html>） */
  setTheme(t: ThemeMode) {
    this.theme = t;
    localStorage.setItem(THEME_KEY, t);
  },

  /**
   * 保留给 Shell.vue 顶栏按钮的旧入口，按 跟随系统 → 深色 → 浅色 → 循环。
   * 绝不能删除：对方 agent 的 Shell.vue 仍调用 store.toggleDark()，删了构建会挂。
   */
  toggleDark() {
    const next: ThemeMode =
      this.theme === "system"
        ? "dark"
        : this.theme === "dark"
          ? "light"
          : "system";
    this.setTheme(next);
  },

  setPlatform(id: string) {
    this.current = id;
    localStorage.setItem("junlive.platform", id);
  },

  /** 记住这个平台当前在看的版块，返回首页时用来恢复。 */
  setCatSel(platform: string, parent: string, sub: string) {
    this.catSel[platform] = { parent, sub };
  },

  isFollowed(platform: string, roomId: string) {
    return this.follows.some(
      (f) => f.platform === platform && f.room_id === roomId,
    );
  },

  toggleFollow(room: Room) {
    const i = this.follows.findIndex(
      (f) => f.platform === room.platform && f.room_id === room.room_id,
    );
    if (i >= 0) {
      this.follows.splice(i, 1);
    } else {
      this.follows.unshift({
        platform: room.platform,
        room_id: room.room_id,
        streamer: room.streamer,
        avatar: room.avatar,
        added: Date.now(),
        // 还没查过，先记 UNKNOWN（排序时排在「在播」之后）
        live: "UNKNOWN",
      });
    }
    localStorage.setItem(FOLLOW_KEY, JSON.stringify(this.follows));
  },

  /**
   * 写入某个关注主播的直播状态（同时落盘）。
   * 状态持久化后，下次进页面不用等异步就能排对顺序。
   */
  setFollowLive(platform: string, roomId: string, live: FollowItem["live"]) {
    const f = this.follows.find(
      (x) => x.platform === platform && x.room_id === roomId,
    );
    if (!f || f.live === live) return;
    f.live = live;
    localStorage.setItem(FOLLOW_KEY, JSON.stringify(this.follows));
  },

  /** 清空关注列表（设置页的「数据管理」用） */
  clearFollows() {
    this.follows.splice(0, this.follows.length);
    localStorage.setItem(FOLLOW_KEY, "[]");
  },

  /** 顺带更新头像/昵称（关注时可能是空的，或平台换了头像） */
  setFollowInfo(platform: string, roomId: string, streamer: string, avatar: string) {
    const f = this.follows.find(
      (x) => x.platform === platform && x.room_id === roomId,
    );
    if (!f) return;
    let changed = false;
    if (avatar && f.avatar !== avatar) {
      f.avatar = avatar;
      changed = true;
    }
    if (streamer && f.streamer !== streamer) {
      f.streamer = streamer;
      changed = true;
    }
    if (changed) localStorage.setItem(FOLLOW_KEY, JSON.stringify(this.follows));
  },

  platformName(id: string) {
    return this.platforms.find((p) => p.id === id)?.name || id;
  },

  platformColor(id: string) {
    return this.platforms.find((p) => p.id === id)?.color || "#888";
  },
});
