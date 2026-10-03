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
    #[serde(default)]
    pub avatar: String,
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
