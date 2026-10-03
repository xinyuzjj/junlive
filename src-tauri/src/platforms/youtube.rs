//! YouTube 直播（需要系统代理）
//!
//! 走网页端解析（ytInitialData / ytInitialPlayerResponse），
//! 直播流直接用 streamingData.hlsManifestUrl，不需要解密签名。

use crate::model::*;
use crate::net;
use crate::proxy;
use serde_json::Value;

fn hdr() -> Vec<(String, String)> {
    vec![
        ("Referer".into(), "https://www.youtube.com/".into()),
        ("User-Agent".into(), net::UA.into()),
        ("Accept-Language".into(), "zh-CN,zh;q=0.9,en;q=0.8".into()),
        // 登录后 googlevideo 的分片会校验会话，回源必须带上 Cookie，
        // 否则表现是「列表能拉、分片全 403」→ 播放器一直缓冲。
        ("Cookie".into(), net::yt_cookie_header()),
        // 注意：**不要**给媒体分片带 Origin ——
        // rr*.googlevideo.com/videoplayback 收到 Origin 会当成 CORS 请求直接 403。
    ]
}

/// 从 HTML 里抠出 `marker = {...};` 的 JSON
pub(crate) fn extract_json(html: &str, marker: &str) -> Option<Value> {
    let idx = html.find(marker)?;
    let rest = &html[idx + marker.len()..];
    let start = rest.find('{')?;
    let bytes = rest.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut end = 0usize;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        let ch = b as char;
        if in_str {
            if esc {
                esc = false;
            } else if ch == '\\' {
                esc = true;
            } else if ch == '"' {
                in_str = false;
            }
        } else {
            match ch {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    if end == 0 {
        return None;
    }
    serde_json::from_str(&rest[start..end]).ok()
}

const CATS: &[(&str, &str)] = &[
    ("全部直播", "live now"),
    ("游戏", "game live"),
    ("音乐", "music live"),
    ("新闻", "news live"),
    ("体育", "sports live"),
    ("娱乐", "entertainment live"),
    ("科技", "technology live"),
    ("电影", "movie live"),
];

pub async fn categories() -> Result<Vec<Category>, String> {
    Ok(CATS
        .iter()
        .map(|(n, k)| Category {
            id: (*k).to_string(),
            name: (*n).to_string(),
            children: vec![],
        })
        .collect())
}

pub async fn rooms(category: &str, page: u32) -> Result<RoomList, String> {
    search(category, page).await
}

pub async fn search(keyword: &str, page: u32) -> Result<RoomList, String> {
    let c = net::via_proxy();
    let kw = urlencoding::encode(keyword);
    let url = format!(
        "https://www.youtube.com/results?search_query={kw}&sp=EgJAAQ%253D%253D&hl=zh-CN&gl=US"
    );
    let html = c
        .get(&url)
        .header("Referer", "https://www.youtube.com/")
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .header("Cookie", crate::net::yt_cookie_header())
        .send()
        .await
        .map_err(|e| format!("YouTube 请求失败（请检查代理设置）：{e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    let data = extract_json(&html, "ytInitialData").ok_or("解析 YouTube 页面失败")?;
    let mut list = Vec::new();

    let sections = data["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]
        ["sectionListRenderer"]["contents"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for sec in &sections {
        let items = sec["itemSectionRenderer"]["contents"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for it in &items {
            let v = &it["videoRenderer"];
            if v.is_null() {
                continue;
            }
            let vid = v["videoId"].as_str().unwrap_or("");
            if vid.is_empty() {
                continue;
            }
            let live = v["badges"]
                .as_array()
                .map(|b| {
                    b.iter().any(|x| {
                        x["metadataBadgeRenderer"]["style"].as_str() == Some("BADGE_STYLE_TYPE_LIVE_NOW")
                            || x["metadataBadgeRenderer"]["label"].as_str() == Some("LIVE")
                    })
                })
                .unwrap_or(false)
                || v["thumbnailOverlays"]
                    .as_array()
                    .map(|o| {
                        o.iter().any(|x| {
                            x["thumbnailOverlayTimeStatusRenderer"]["style"].as_str() == Some("LIVE")
                        })
                    })
                    .unwrap_or(false);

            list.push(Room {
                platform: "youtube".into(),
                room_id: vid.to_string(),
                title: text_of(&v["title"]),
                streamer: text_of(&v["ownerText"]),
                cover: v["thumbnail"]["thumbnails"]
                    .as_array()
                    .and_then(|t| t.last())
                    .and_then(|t| t["url"].as_str())
                    .unwrap_or("")
                    .to_string(),
                area: String::new(),
                online: text_of(&v["viewCountText"]),
                live,
                avatar: String::new(),
            });
        }
    }

    Ok(RoomList {
        rooms: list,
        page,
        has_more: false,
    })
}

pub async fn room_detail(room_id: &str) -> Result<RoomDetail, String> {
    let vid = extract_video_id(room_id);
    if vid.is_empty() {
        return Err("请填写 YouTube 视频 ID 或链接".into());
    }
    let c = net::via_proxy();
    let url = format!("https://www.youtube.com/watch?v={vid}&hl=zh-CN");
    let html = c
        .get(&url)
        .header("Referer", "https://www.youtube.com/")
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .header("Cookie", crate::net::yt_cookie_header())
        .send()
        .await
        .map_err(|e| format!("YouTube 请求失败（请检查代理设置）：{e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    let pr = extract_json(&html, "ytInitialPlayerResponse").ok_or("解析播放信息失败")?;
    let vd = &pr["videoDetails"];
    if vd.is_null() {
        // YouTube 对代理 IP 会返回 LOGIN_REQUIRED（bot 墙），
        // 这时必须让用户在设置里粘 Cookie，别只丢一句「视频不存在」。
        let st = pr["playabilityStatus"]["status"].as_str().unwrap_or("");
        let reason = pr["playabilityStatus"]["reason"].as_str().unwrap_or("");
        if st == "LOGIN_REQUIRED" {
            return Err(format!(
                "YouTube 要求验证登录（{reason}）。\
                 当前代理 IP 被判定为机器人，请在「设置」里粘贴 YouTube Cookie 后重试。"
            ));
        }
        if !reason.is_empty() {
            return Err(format!("YouTube 无法播放该视频：{reason}"));
        }
        return Err("视频不存在或无法访问".into());
    }
    let live = vd["isLiveContent"].as_bool().unwrap_or(false);

    let room = Room {
        platform: "youtube".into(),
        room_id: vid.clone(),
        title: vd["title"].as_str().unwrap_or("").to_string(),
        streamer: vd["author"].as_str().unwrap_or("").to_string(),
        cover: vd["thumbnail"]["thumbnails"]
            .as_array()
            .and_then(|t| t.last())
            .and_then(|t| t["url"].as_str())
            .unwrap_or("")
            .to_string(),
        area: String::new(),
        online: vd["viewCount"]
            .as_str()
            .map(|s| {
                s.parse::<i64>()
                    .map(|n| {
                        if n >= 10000 {
                            format!("{:.1}万", n as f64 / 10000.0)
                        } else {
                            n.to_string()
                        }
                    })
                    .unwrap_or_else(|_| s.to_string())
            })
            .unwrap_or_default(),
        live,
        avatar: String::new(),
    };

    let mut plays = Vec::new();
    if let Some(hls) = pr["streamingData"]["hlsManifestUrl"].as_str() {
        plays.push(PlayUrl {
            proxy: proxy::wrap(hls, hdr(), true),
            url: hls.to_string(),
            format: "hls".into(),
            quality: "自动".into(),
        });
    }

    Ok(RoomDetail {
        room,
        description: vd["shortDescription"].as_str().unwrap_or("").to_string(),
        plays,
    })
}

fn text_of(v: &Value) -> String {
    if let Some(s) = v["simpleText"].as_str() {
        return s.to_string();
    }
    if let Some(runs) = v["runs"].as_array() {
        return runs
            .iter()
            .filter_map(|r| r["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
    }
    String::new()
}

pub(crate) fn extract_video_id(s: &str) -> String {
    let s = s.trim();
    if let Some(i) = s.find("v=") {
        let rest = &s[i + 2..];
        return rest.split('&').next().unwrap_or("").to_string();
    }
    if s.contains("youtu.be/") {
        return s
            .rsplit("youtu.be/")
            .next()
            .unwrap_or("")
            .split(['?', '&', '/'])
            .next()
            .unwrap_or("")
            .to_string();
    }
    s.split(['?', '&', '/']).next().unwrap_or("").to_string()
}
