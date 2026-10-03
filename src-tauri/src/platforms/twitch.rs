//! Twitch（需要系统代理）

use crate::model::*;
use crate::net;
use crate::proxy;
use serde_json::{json, Value};

const GQL: &str = "https://gql.twitch.tv/gql";
const CID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";

async fn gql(query: &str, vars: Option<Value>) -> Result<Value, String> {
    let c = net::via_proxy();
    let mut body = json!({ "query": query });
    if let Some(v) = vars {
        body["variables"] = v;
    }
    let v: Value = c
        .post(GQL)
        .header("Client-ID", CID)
        .header("Origin", "https://www.twitch.tv")
        .header("Referer", "https://www.twitch.tv/")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Twitch 请求失败（请检查代理设置）：{e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if let Some(errs) = v["errors"].as_array() {
        if !errs.is_empty() {
            let msg = errs[0]["message"].as_str().unwrap_or("Twitch 接口报错");
            return Err(msg.to_string());
        }
    }
    Ok(v)
}

pub async fn categories() -> Result<Vec<Category>, String> {
    let q = r#"query { games(first: 60) { edges { node { id name displayName } } } }"#;
    let v = gql(q, None).await?;
    let mut out = Vec::new();
    if let Some(edges) = v["data"]["games"]["edges"].as_array() {
        for e in edges {
            let name = e["node"]["name"].as_str().unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            out.push(Category {
                id: name,
                name: e["node"]["displayName"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                children: vec![],
            });
        }
    }
    Ok(out)
}

pub async fn rooms(category: &str, page: u32) -> Result<RoomList, String> {
    let q = r#"query($name: String!) { game(name: $name) { streams(first: 30) { edges { node { id title viewersCount previewImageURL(width: 640) broadcaster { login displayName profileImageURL(width: 150) } } } } } }"#;
    let v = gql(q, Some(json!({ "name": category }))).await?;
    let mut list = Vec::new();
    if let Some(edges) = v["data"]["game"]["streams"]["edges"].as_array() {
        for e in edges {
            let n = &e["node"];
            let login = n["broadcaster"]["login"].as_str().unwrap_or("");
            if login.is_empty() {
                continue;
            }
            list.push(Room {
                platform: "twitch".into(),
                room_id: login.to_string(),
                title: n["title"].as_str().unwrap_or("").to_string(),
                streamer: n["broadcaster"]["displayName"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                cover: fix_cover(n["previewImageURL"].as_str().unwrap_or("")),
                area: category.to_string(),
                online: fmt_num(n["viewersCount"].as_i64().unwrap_or(0)),
                live: true,
                avatar: n["broadcaster"]["profileImageURL"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    Ok(RoomList {
        rooms: list,
        page,
        has_more: false,
    })
}

pub async fn search(keyword: &str, page: u32) -> Result<RoomList, String> {
    let q = r#"query($q: String!) { searchFor(userQuery: $q, platform: "web") { channels { items { ... on User { login displayName stream { id title viewersCount previewImageURL(width: 640) game { displayName } } } } } } }"#;
    let v = gql(q, Some(json!({ "q": keyword }))).await?;
    let mut list = Vec::new();
    if let Some(items) = v["data"]["searchFor"]["channels"]["items"].as_array() {
        for u in items {
            let login = u["login"].as_str().unwrap_or("");
            if login.is_empty() {
                continue;
            }
            let live = !u["stream"].is_null();
            list.push(Room {
                platform: "twitch".into(),
                room_id: login.to_string(),
                title: u["stream"]["title"].as_str().unwrap_or("").to_string(),
                streamer: u["displayName"].as_str().unwrap_or("").to_string(),
                cover: fix_cover(
                    u["stream"]["previewImageURL"].as_str().unwrap_or(""),
                ),
                area: u["stream"]["game"]["displayName"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                online: fmt_num(u["stream"]["viewersCount"].as_i64().unwrap_or(0)),
                live,
                avatar: u["profileImageURL"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Ok(RoomList {
        rooms: list,
        page,
        has_more: false,
    })
}

pub async fn room_detail(login: &str) -> Result<RoomDetail, String> {
    let login = login
        .trim()
        .trim_start_matches('@')
        .rsplit('/')
        .next()
        .unwrap_or(login)
        .to_lowercase();

    let q = r#"query($login: String!) { user(login: $login) { id login displayName profileImageURL(width: 150) stream { id title viewersCount previewImageURL(width: 640) game { displayName } } } }"#;
    let v = gql(q, Some(json!({ "login": login }))).await?;
    let u = &v["data"]["user"];
    if u.is_null() {
        return Err(format!("频道 {login} 不存在"));
    }
    let live = !u["stream"].is_null();

    let room = Room {
        platform: "twitch".into(),
        room_id: u["login"].as_str().unwrap_or(&login).to_string(),
        title: u["stream"]["title"].as_str().unwrap_or("").to_string(),
        streamer: u["displayName"].as_str().unwrap_or("").to_string(),
        cover: fix_cover(u["stream"]["previewImageURL"].as_str().unwrap_or("")),
        area: u["stream"]["game"]["displayName"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        online: fmt_num(u["stream"]["viewersCount"].as_i64().unwrap_or(0)),
        live,
        avatar: u["profileImageURL"].as_str().unwrap_or("").to_string(),
    };

    let mut plays = Vec::new();
    if live {
        match play_urls(&login).await {
            Ok(mut p) => plays.append(&mut p),
            Err(e) => eprintln!("[twitch] play url error: {e}"),
        }
    }

    Ok(RoomDetail {
        room,
        description: u["description"].as_str().unwrap_or("").to_string(),
        plays,
    })
}

async fn play_urls(login: &str) -> Result<Vec<PlayUrl>, String> {
    let q = r#"query($login: String!) { streamPlaybackAccessToken(channelName: $login, params: {platform: "web", playerBackend: "mediaplayer", playerType: "site"}) { value signature } }"#;
    let v = gql(q, Some(json!({ "login": login }))).await?;
    let token = v["data"]["streamPlaybackAccessToken"]["value"]
        .as_str()
        .unwrap_or("");
    let sig = v["data"]["streamPlaybackAccessToken"]["signature"]
        .as_str()
        .unwrap_or("");
    if token.is_empty() {
        return Err("拿不到播放令牌（可能需要代理，或该频道受地区限制）".into());
    }

    let url = format!(
        "https://usher.ttvnw.net/api/channel/hls/{login}.m3u8?allow_source=true&allow_audio_only=true&client_id={CID}&fast_bread=true&player_backend=mediaplayer&playlist_include_framerate=true&reassignments_supported=true&sig={}&supported_codecs=av1,h264&token={}&transcode_mode=cross_version_2",
        urlencoding::encode(sig),
        urlencoding::encode(token)
    );

    let headers = vec![
        ("Referer".into(), "https://www.twitch.tv/".into()),
        ("Origin".into(), "https://www.twitch.tv".into()),
        ("User-Agent".into(), net::UA.into()),
    ];

    Ok(vec![PlayUrl {
        proxy: proxy::wrap(&url, headers, true),
        url,
        format: "hls".into(),
        quality: "自动（含所有清晰度）".into(),
    }])
}

fn fmt_num(n: i64) -> String {
    if n >= 10000 {
        format!("{:.1}万", n as f64 / 10000.0)
    } else {
        n.to_string()
    }
}

/// Twitch GQL 返回的缩略图是 `...-640x{height}.jpg`，占位符**不会自动替换**，
/// 直接当 URL 用会 404 → 列表里全是字母占位图。这里补成 640x360。
fn fix_cover(u: &str) -> String {
    u.replace("{width}", "640").replace("{height}", "360")
}
