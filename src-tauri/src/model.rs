//! 统一数据模型：所有平台都归一化成这里的结构，前端只认这一套。

use serde::{Deserialize, Serialize};

/// 平台元信息（前端用来画平台切换条）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub id: String,
    pub name: String,
    /// 平台主色，前端做小圆点/标签
    pub color: String,
    /// 是否支持弹幕
    pub danmaku: bool,
}

/// 分区 / 分类
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Category {
    pub id: String,
    pub name: String,
    /// 子分区
    #[serde(default)]
    pub children: Vec<Category>,
}

/// 直播间摘要（列表页用）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Room {
    pub platform: String,
    pub room_id: String,
    pub title: String,
    /// 主播昵称
    pub streamer: String,
    pub cover: String,
    /// 分区名
    pub area: String,
    /// 人气/在线人数（字符串，各平台单位不同）
    pub online: String,
    /// 是否正在直播
    pub live: bool,
    /// 在放录播（斗鱼：betard 的 `videoLoop == 1`，即视频轮播）。
    ///
    /// 这类房间 `show_status` 仍是 1、也真的在推流，但内容是录像 ——
    /// 只看 live 会误判成「正在直播」。注意**只有房间详情接口（betard）
    /// 有这个字段**，分类列表接口（mixList）完全没有，所以列表里的房间
    /// 一律是 false，只有关注栏和直播间页能拿到真实值。
    #[serde(default)]
    pub replay: bool,
    #[serde(default)]
    pub avatar: String,
}

/// 一场直播回放（当前只有斗鱼提供）。
///
/// 斗鱼把一场直播按约 2 小时切成多段，这里**已经拍平**：
/// 一个 Replay 就是一段可直接播的视频。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Replay {
    /// 播放用的视频 ID（斗鱼是 `v.douyu.com/show/<hash_id>`）
    pub hash_id: String,
    pub title: String,
    pub cover: String,
    /// **整场**时长文案，如 "13:13:52"。
    ///
    /// 注意这不是「一段」的长度：斗鱼把一场直播切成若干 2 小时的段，
    /// 这里给的是整场总长（接口的 `replay_duration`）。
    pub duration: String,
    /// 场次时间，如 "2026-10-08 13点场"
    pub time: String,
    /// 观看数（字符串）
    pub view_num: String,
    /// 该段视频的起始 Unix 秒。
    ///
    /// 弹幕 / AI看点的接口给的都是**绝对时间戳**，要减掉它才是视频内进度。
    #[serde(default)]
    pub start_time: i64,
    /// 整场被切成几段（来自 `getShowReplayList`）
    #[serde(default)]
    pub parts: u32,
}

/// 回放列表的一页。
///
/// 为什么要带 `total`：斗鱼这个接口**每页锁死 20 条**（limit 传 100 也只回 20），
/// 而一个主播的历史回放动辄几千场（实测 2237 场）。前端要知道总数才能
/// 显示「已加载 40 / 共 2237 场」并决定还要不要继续翻页。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayPage {
    pub list: Vec<Replay>,
    /// 总场次（接口的 `data.count`）
    pub total: u32,
}

/// 一条历史弹幕（回放用）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayDanmaku {
    /// 在视频里的秒数
    pub time: f64,
    pub text: String,
    /// 十六进制颜色，空串 = 用默认色
    pub color: String,
    pub user: String,
}

/// 一条 AI 看点（回放用）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayHighlight {
    /// 在视频里的秒数
    pub time: f64,
    pub title: String,
    pub desc: String,
}

/// 播放地址
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayUrl {
    /// 上游真实流地址
    pub url: String,
    /// 走本地代理后的地址（前端播这个，解决 Referer / CORS 防盗链）
    pub proxy: String,
    /// hls | flv
    pub format: String,
    /// 清晰度名，如 原画 / 蓝光 / 1080p
    pub quality: String,
    /// 可选清晰度（目前只有回放有；直播的清晰度在别处）
    #[serde(default)]
    pub qualities: Vec<ReplayQuality>,
}

/// 回放的一档清晰度。
///
/// 斗鱼的 `getStreamUrlWeb` 响应里一次给全所有档位（标清480P / 高清720P /
/// 1080P60 / 原画2K60），所以解析一次就能让用户随便切。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayQuality {
    /// 档位 id，如 normal / high / 1080p60 / 1440p60a
    pub id: String,
    /// 显示名，如「高清1080P60」
    pub name: String,
    /// 上游真实地址
    pub url: String,
    /// 走本地代理后的地址（前端播这个）
    pub proxy: String,
    /// 码率（bps），用来排序 / 显示
    pub bitrate: i64,
    /// 档位高低（斗鱼给的分级，越大越高）
    pub level: i64,
}

/// 直播间详情 = 摘要 + 可用播放地址
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoomDetail {
    #[serde(flatten)]
    pub room: Room,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub plays: Vec<PlayUrl>,
}

/// 分页列表
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoomList {
    pub rooms: Vec<Room>,
    pub page: u32,
    pub has_more: bool,
}
