import { reactive } from "vue";
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
   * 深色模式开关。
   *
   * 为什么把状态放在 store 而不是组件里：顶栏的切换按钮、启动时的主题应用、
   * 刷新后的恢复，三处都要读同一个值，放这里只有一份真相。
   * 持久化键名固定为 junlive.dark，存 "1"/"0"（用 === "1" 判断，
   * 避免老数据里出现字符串 "true"/"false" 时误判）。
   * 注意：这里**只负责状态与持久化**，真正往 <html> 上挂 class 的动作交给
   * main.ts —— 保持 store 不直接碰 DOM，方便以后在别处复用。
   */
  dark: localStorage.getItem("junlive.dark") === "1",

  toggleDark() {
    this.dark = !this.dark;
    localStorage.setItem("junlive.dark", this.dark ? "1" : "0");
  },

  setPlatform(id: string) {
    this.current = id;
    localStorage.setItem("junlive.platform", id);
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
