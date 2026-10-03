//! 弹幕接入。**每个平台一个文件、一套自己的协议，互不混用**：
//!
//! - `bilibili.rs` 哔哩哔哩：wss + 认证包 + brotli，拿 token 走 WBI 签名
//! - `douyu.rs`    斗鱼：wss + `key@=value/` 文本协议，免登录
//! - `huya.rs`     虎牙：wss + TARS，uid 取房间页 `lp`，聊天弹幕 kind==1400
//! - `twitch.rs`   Twitch：wss IRC，匿名 justinfan
//! - `youtube.rs`  YouTube：HTTP 长轮询 live_chat（播放走官方 embed，弹幕自己拉）
//!
//! 本文件只放公共部分：消息结构、连接代号、统一 emit、以及 `start` 的平台分发。

pub(crate) use crate::net;
pub(crate) use futures_util::{SinkExt, StreamExt};
pub(crate) use serde::Serialize;
pub(crate) use serde_json::Value;
pub(crate) use std::collections::HashMap;
pub(crate) use std::io::Read;
pub(crate) use std::sync::atomic::{AtomicU64, Ordering};
pub(crate) use std::time::Duration;
pub(crate) use tauri::{AppHandle, Emitter};
pub(crate) use tokio::time::interval;
pub(crate) use tokio_tungstenite::tungstenite::client::IntoClientRequest;
pub(crate) use tokio_tungstenite::tungstenite::Message;

mod bilibili;
mod douyin;
mod douyu;
mod huya;
mod sign;
mod twitch;
mod youtube;

#[derive(Clone, Serialize)]
pub struct DanmakuMsg {
    pub platform: String,
    pub user: String,
    pub text: String,
    /// 形如 "#ffffff"，前端直接用
    pub color: String,
    pub kind: String,
    pub ts: u64,
}

/// 连接代号：新连接 +1，旧循环发现对不上就自行退出
static SESSION: AtomicU64 = AtomicU64::new(0);

pub fn stop() {
    SESSION.fetch_add(1, Ordering::SeqCst);
}

fn session_id() -> u64 {
    SESSION.load(Ordering::SeqCst)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn emit(app: &Option<AppHandle>, platform: &str, user: &str, text: &str, color: &str) {
    if text.trim().is_empty() {
        return;
    }
    let msg = DanmakuMsg {
        platform: platform.into(),
        user: if user.trim().is_empty() {
            "匿名".into()
        } else {
            user.trim().to_string()
        },
        text: text.trim().to_string(),
        color: color.into(),
        kind: "chat".into(),
        ts: now_ms(),
    };
    match app {
        Some(a) => {
            let _ = a.emit("danmaku", msg);
        }
        // CLI 诊断模式（--test danmaku）：直接打到终端
        None => println!("[{}] {}: {}", msg.platform, msg.user, msg.text),
    }
}

pub async fn start(app: Option<AppHandle>, platform: &str, room_id: &str) -> Result<(), String> {
    stop();
    let my = session_id();
    let (p, rid) = (platform.to_string(), room_id.to_string());
    tokio::spawn(async move {
        let r = match p.as_str() {
            "bilibili" => {
                let n: u64 = match rid.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!("[danmaku] bilibili 房间号不是数字: {rid}");
                        return;
                    }
                };
                bilibili::run_bilibili(app.clone(), n, my).await
            }
            "douyin" => douyin::run_douyin(app.clone(), rid.clone(), my).await,
            "douyu" => douyu::run_douyu(app.clone(), rid.clone(), my).await,
            "huya" => huya::run_huya(app.clone(), rid.clone(), my).await,
            "twitch" => twitch::run_twitch(app.clone(), rid.clone(), my).await,
            "youtube" => youtube::run_youtube(app.clone(), rid.clone(), my).await,
            other => Err(format!("{other} 的弹幕暂未接入")),
        };
        if let Err(e) = r {
            eprintln!("[danmaku] {p} {rid}: {e}");
        }
    });
    Ok(())
}

