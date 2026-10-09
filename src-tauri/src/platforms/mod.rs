//! 平台注册表：把 7 个平台的实现统一到一组函数上。

pub mod bilibili;
pub mod douyin;
pub mod douyin_a_bogus;
pub mod douyu;
pub mod huya;
pub mod soop;
pub mod twitch;
pub mod youtube;

use crate::model::*;

pub fn all() -> Vec<PlatformInfo> {
    vec![
        PlatformInfo {
            id: "bilibili".into(),
            name: "哔哩哔哩".into(),
            color: "#FB7299".into(),
            danmaku: true,
        },
        PlatformInfo {
            id: "douyu".into(),
            name: "斗鱼".into(),
            color: "#FF5D23".into(),
            danmaku: true,
        },
        PlatformInfo {
            id: "huya".into(),
            name: "虎牙".into(),
            color: "#FFA800".into(),
            danmaku: true,
        },
        PlatformInfo {
            id: "douyin".into(),
            name: "抖音".into(),
            color: "#FE2C55".into(),
            danmaku: true,
        },
        PlatformInfo {
            id: "youtube".into(),
            name: "YouTube".into(),
            color: "#FF0000".into(),
            danmaku: true,
        },
        PlatformInfo {
            id: "soop".into(),
            name: "SOOP".into(),
            color: "#0B7DFF".into(),
            danmaku: false,
        },
        PlatformInfo {
            id: "twitch".into(),
            name: "Twitch".into(),
            color: "#9146FF".into(),
            danmaku: true,
        },
    ]
}

pub async fn categories(platform: &str) -> Result<Vec<Category>, String> {
    match platform {
        "bilibili" => bilibili::categories().await,
        "douyu" => douyu::categories().await,
        "huya" => huya::categories().await,
        "douyin" => douyin::categories().await,
        "youtube" => youtube::categories().await,
        "soop" => soop::categories().await,
        "twitch" => twitch::categories().await,
        other => Err(format!("未知平台: {other}")),
    }
}

pub async fn rooms(platform: &str, category: &str, page: u32) -> Result<RoomList, String> {
    match platform {
        "bilibili" => bilibili::rooms(category, page).await,
        "douyu" => douyu::rooms(category, page).await,
        "huya" => huya::rooms(category, page).await,
        "douyin" => douyin::rooms(category, page).await,
        "youtube" => youtube::rooms(category, page).await,
        "soop" => soop::rooms(category, page).await,
        "twitch" => twitch::rooms(category, page).await,
        other => Err(format!("未知平台: {other}")),
    }
}

pub async fn search(platform: &str, keyword: &str, page: u32) -> Result<RoomList, String> {
    match platform {
        "bilibili" => bilibili::search(keyword, page).await,
        "douyu" => douyu::search(keyword, page).await,
        "huya" => huya::search(keyword, page).await,
        "douyin" => douyin::search(keyword, page).await,
        "youtube" => youtube::search(keyword, page).await,
        "soop" => soop::search(keyword, page).await,
        "twitch" => twitch::search(keyword, page).await,
        other => Err(format!("未知平台: {other}")),
    }
}

pub async fn room_detail(platform: &str, room_id: &str) -> Result<RoomDetail, String> {
    match platform {
        "bilibili" => bilibili::room_detail(room_id).await,
        "douyu" => douyu::room_detail(room_id).await,
        "huya" => huya::room_detail(room_id).await,
        "douyin" => douyin::room_detail(room_id).await,
        "youtube" => youtube::room_detail(room_id).await,
        "soop" => soop::room_detail(room_id).await,
        "twitch" => twitch::room_detail(room_id).await,
        other => Err(format!("未知平台: {other}")),
    }
}

/// 主播的直播回放。目前只有斗鱼提供 —— 其他平台明确报「不支持」，
/// 不要静默返回空列表，否则前端分不清「没有回放」和「平台不支持」。
pub async fn replays(platform: &str, room_id: &str, page: u32) -> Result<ReplayPage, String> {
    match platform {
        "douyu" => douyu::replays(room_id, page).await,
        other => Err(format!("{} 暂不支持查看回放", display_name(other))),
    }
}

/// 一场回放的完整分段（当前只有斗鱼）。
pub async fn replay_parts(
    platform: &str,
    room_id: &str,
    hash_id: &str,
    show_start: i64,
) -> Result<Vec<Replay>, String> {
    match platform {
        "douyu" => douyu::replay_parts(room_id, hash_id, show_start).await,
        other => Err(format!("{} 暂不支持查看回放", display_name(other))),
    }
}

/// 一场回放的历史弹幕。
pub async fn replay_danmaku(platform: &str, hash_id: &str, start_time: i64) -> Result<Vec<ReplayDanmaku>, String> {
    match platform {
        "douyu" => douyu::replay_danmaku(hash_id, start_time).await,
        other => Err(format!("{} 暂不支持查看回放", display_name(other))),
    }
}

/// 一场回放的 AI 看点。
pub async fn replay_highlights(
    platform: &str,
    hash_id: &str,
    start_time: i64,
    duration: i64,
) -> Result<Vec<ReplayHighlight>, String> {
    match platform {
        "douyu" => douyu::replay_highlights(hash_id, start_time, duration).await,
        other => Err(format!("{} 暂不支持查看回放", display_name(other))),
    }
}

/// 平台 id → 显示名
pub fn display_name(platform: &str) -> String {
    all()
        .into_iter()
        .find(|p| p.id == platform)
        .map(|p| p.name)
        .unwrap_or_else(|| platform.to_string())
}
