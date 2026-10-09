//! 抖音直播
//!
//! 先访问直播间页面拿 ttwid 等 cookie，再调 webcast 接口。
//! 抖音的关键词搜索需要 a_bogus 签名，这里不做，只支持房间号 / URL 直达。

use crate::model::*;
use crate::net;
use crate::proxy;
use serde_json::Value;
use std::sync::OnceLock;

fn hdr(rid: &str) -> Vec<(String, String)> {
    vec![
        ("Referer".into(), format!("https://live.douyin.com/{rid}")),
        ("User-Agent".into(), net::UA.into()),
    ]
}

/// 从 URL 或纯数字里取房间号
pub fn extract_rid(s: &str) -> String {
    let s = s.trim();
    if s.contains("live.douyin.com/") {
        let rest = s.rsplit("live.douyin.com/").next().unwrap_or("");
        let r = rest.split(['?', '/', '&']).next().unwrap_or("");
        if !r.is_empty() {
            return r.to_string();
        }
    }
    if s.contains("douyin.com/user/") {
        return s.to_string();
    }
    s.to_string()
}

const CATS: &[(&str, &str)] = &[
    ("热门", "720"),
    ("游戏", "1"),
    ("娱乐", "2"),
    ("生活", "3"),
    ("音乐", "5"),
    ("户外", "9"),
];

/// 抖音分类树 —— 从直播首页内嵌的 `categoryData` 里解析。
///
/// 首页 HTML 里带着**完整分类树**（每项有 `id_str` 和 `title`），
/// 游戏下面还有「射击游戏 / MOBA」这类二级、以及「和平精英 / 绝地求生」三级。
/// 这些 `id_str` 直接就能当 `partition` 参数用（实测都能拉到房间）。
///
/// 为什么不再硬编码：以前写死 6 个顶级项、且 `children` 全是空的，
/// 所以抖音看不到任何子分类。
async fn fetch_categories() -> Result<Vec<Category>, String> {
    let c = net::direct();
    let html = c
        .get("https://live.douyin.com/")
        .header("User-Agent", net::UA)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let arr = extract_category_array(&html).ok_or("抖音首页里没找到分类数据")?;
    Ok(parse_categories(&arr))
}

/// 从首页 HTML 里抠出 `categoryData` 对应的数组。
///
/// 两个坑：
/// 1. 它是**嵌在转义 JSON 字符串里**的（`categoryData\":[{\"partition\":...`），
///    必须先把 `\"` 还原成 `"` 才能当 JSON 解析。
/// 2. 必须**括号计数**取数组边界，不能用正则 —— 字符串值里同样有 `]`，
///    非贪婪正则会在那里截断，得到一个残缺数组。
fn extract_category_array(html: &str) -> Option<Value> {
    let i = html.find("categoryData")?;
    let end = (i + 300_000).min(html.len());
    let seg = html[i..end].replace("\\\"", "\"").replace("\\\\", "\\");

    let start = seg.find('[')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (k, ch) in seg[start..].char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if ch == '\\' {
                esc = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&seg[start..start + k + 1]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// 把 `categoryData` 的节点递归转成 `Category`。
fn parse_categories(v: &Value) -> Vec<Category> {
    let mut out = Vec::new();
    let Some(arr) = v.as_array() else {
        return out;
    };
    for item in arr {
        let p = &item["partition"];
        let id = p["id_str"].as_str().unwrap_or("");
        let name = p["title"].as_str().unwrap_or("");
        if id.is_empty() || name.is_empty() {
            continue;
        }
        // ID 里带上节点的 `type`：请求房间列表时必须原样回传 partition_type，
        // 否则类型对不上，接口会返回完全不相干的内容
        // （顶级节点 type=4，传成 1 时「游戏」返回的是卖黄金的直播间）。
        let ty = p["type"].as_i64().unwrap_or(1);
        out.push(Category {
            id: format!("{ty}_{id}"),
            name: name.to_string(),
            children: parse_categories(&item["sub_partition"]),
        });
    }
    out
}

pub async fn categories() -> Result<Vec<Category>, String> {
    match fetch_categories().await {
        Ok(v) if !v.is_empty() => Ok(v),
        // 首页结构变了就退回硬编码的六个顶级项，至少别让分类页空着
        _ => Ok(CATS
            .iter()
            .map(|(n, id)| Category {
                id: (*id).to_string(),
                name: (*n).to_string(),
                children: vec![],
            })
            .collect()),
    }
}

/// 百分比编码（a_bogus 里可能带 + / = 等字符）
fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// msToken（107 位，和浏览器里的一致）。
///
/// **整个进程只生成一次，之后所有请求复用同一个值。**
///
/// 踩过的坑：以前是每次请求都 `gen_ms_token(107)` 随机一个新的，
/// 结果抖音把每次请求都当成**新会话**，于是 `offset` 被无视 ——
/// page=1 和 page=2 返回**完全一样的 15 条**（实测 dupCount=15/15）。
/// 用户看到的就是「只有 15 个、点加载更多全是重复」。
///
/// 对照实验：不带 msToken 的请求 offset 完全正常（6 页 90 个不重复房间），
/// 带随机 msToken 的请求分页失效。所以问题就出在「每页换 token」上。
fn gen_ms_token(n: usize) -> String {
    use rand::Rng;
    const CH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..n).map(|_| CH[rng.gen_range(0..CH.len())] as char).collect()
}

/// msToken —— **整个进程只生成一次，之后所有请求复用同一个值。**
///
/// 踩过的坑：以前是每次请求都随机一个新 msToken，结果抖音把每次请求
/// 都当成**新会话**，于是 `offset` 被无视 —— page=1 和 page=2 返回
/// **完全一样的 15 条**（实测 dupCount=15/15）。用户看到的就是
/// 「只有 15 个、点加载更多全是重复」。
///
/// 对照实验：不带 msToken 的请求 offset 完全正常（6 页 90 个不重复房间），
/// 带随机 msToken 的请求分页失效。所以问题就出在「每页换 token」上。
static MS_TOKEN: OnceLock<String> = OnceLock::new();

fn ms_token() -> String {
    MS_TOKEN.get_or_init(|| gen_ms_token(107)).clone()
}

pub async fn rooms(category: &str, page: u32) -> Result<RoomList, String> {
    // **整个进程共用一个 client**（cookie / 会话必须连续）。
    //
    // 以前每次都 `net::direct()` 新建一个，等于每次请求都是全新会话 ——
    // 这和 msToken 每页换新是同一类问题：抖音会因此放弃 offset 游标，
    // 翻页返回同一批房间（实测 page=1 和 page=2 完全一样的 15 条）。
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    let c = CLIENT.get_or_init(net::direct);
    let c = c.clone();

    // 预热拿 cookie —— 每个进程只做一次，之后靠 client 自己保持。
    static WARMED: OnceLock<()> = OnceLock::new();
    if WARMED.get().is_none() {
        let _ = c
            .get("https://live.douyin.com/")
            .header("User-Agent", net::UA)
            .send()
            .await;
        let _ = WARMED.set(());
    }

    // category 形如 "4_103"（type_id，由 categories() 产出）；
    // 兼容不带 type 的旧写法，此时按 type=1 处理。
    let (ptype, pid) = match category.split_once('_') {
        Some((t, rest)) if t.chars().all(|c| c.is_ascii_digit()) => (t.to_string(), rest.to_string()),
        _ => ("1".to_string(), category.to_string()),
    };

    let offset = (page.saturating_sub(1)) * 15;
    let ms = ms_token();
    let q = format!(
        "aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&enter_from=web_homepage_hot&cookie_enabled=true&screen_width=1920&screen_height=1080&browser_language=zh-CN&browser_platform=Win32&browser_name=Chrome&browser_version=131.0.0.0&count=15&offset={offset}&partition={pid}&partition_type={ptype}&req_from=2&msToken={ms}"
    );
    // 房间列表接口必须带 a_bogus 签名，缺了直接 Access Denied
    let sign = super::douyin_a_bogus::generate_a_bogus(&q, net::UA);
    let url = format!(
        "https://live.douyin.com/webcast/web/partition/detail/room/v2/?{q}&a_bogus={}",
        pct(&sign)
    );
    let v: Value = c
        .get(&url)
        .header("Referer", "https://live.douyin.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    if let Some(arr) = v["data"]["data"].as_array() {
        for r in arr {
            list.push(parse_room(r));
        }
    }
    // 「还有没有下一页」按**返回条数**判，别只看列表空不空。
    //
    // 以前写的是 `!list.is_empty()`，而抖音每次都凑满 15 条，
    // 导致 has_more 永远为 true —— 翻到最后一页前端还会继续请求，
    // 拿回来一批重复的，再叠加 Home.vue 里那套「有一次重复就锁死」的逻辑，
    // 表现就是「只有 15 个、滚到底没反应」。
    // 实测：热门 6 页 × 15 = 90 个不重复房间，offset 是生效的。
    let has_more = list.len() >= 15;
    Ok(RoomList {
        rooms: list,
        page,
        has_more,
    })
}

/// 抖音搜索。
///
/// **抖音没有可用的匿名搜索接口** —— `/aweme/v1/web/live/search/`、
/// `/aweme/v1/web/search/item/` 无论怎么签名都返回
/// `{"status_code":2483,"status_msg":"请先登录，再继续搜索吧"}`，
/// 这是平台的硬限制（搜索必须带登录态 Cookie）。
///
/// 所以这里照 DTV 的做法：**把输入当成直播间号**。
/// DTV 的抖音搜索框占位符就写着「搜索直播间号」，
/// 它的 `searchPlatform` 对 douyin 直接 `return null`（源码注释：
/// 「custom / douyin：暂不支持统一搜索」）。
///
/// 输入纯数字（或直播间链接）→ 查这个房间并返回一条结果，
/// 点进去就是那个直播间。输入中文关键词 → 明确提示要输房间号，
/// 不要静默返回 0 条让人以为「搜不到」。
pub async fn search(keyword: &str, page: u32) -> Result<RoomList, String> {
    let kw = keyword.trim();
    if kw.is_empty() {
        return Ok(RoomList {
            rooms: vec![],
            page,
            has_more: false,
        });
    }

    let rid = extract_rid(kw);
    let is_room_id = !rid.is_empty() && rid.chars().all(|c| c.is_ascii_digit());
    if !is_room_id {
        return Err(
            "抖音不支持按关键词搜索（平台限制：搜索接口必须登录）。请输入抖音直播间号（纯数字，如 444120643759）或直播间链接。"
                .into(),
        );
    }

    let d = room_detail(&rid).await?;
    Ok(RoomList {
        rooms: vec![d.room],
        page,
        has_more: false,
    })
}

pub async fn room_detail(room_id: &str) -> Result<RoomDetail, String> {
    let rid = extract_rid(room_id);
    let c = net::direct();
    let _ = c
        .get(format!("https://live.douyin.com/{rid}"))
        .header("User-Agent", net::UA)
        .send()
        .await;

    let url = format!(
        "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&enter_from=web_live&cookie_enabled=true&screen_width=1920&screen_height=1080&browser_language=zh-CN&browser_platform=Win32&browser_name=Chrome&browser_version=131.0.0.0&web_rid={rid}"
    );
    let v: Value = c
        .get(&url)
        .header("Referer", format!("https://live.douyin.com/{rid}"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let data = &v["data"];
    if data.is_null() {
        return Err(format!("抖音房间 {rid} 解析失败（可能需要更新 cookie 或该房间不存在）"));
    }
    let rooms = data["data"].as_array().cloned().unwrap_or_default();
    let r0 = rooms.first().cloned().unwrap_or(Value::Null);
    let user = &data["user"];

    let status = r0["status"].as_i64().unwrap_or(0);
    let live = status == 2;

    let room = Room {
        platform: "douyin".into(),
        room_id: rid.clone(),
        title: r0["title"].as_str().unwrap_or("").to_string(),
        streamer: user["nickname"].as_str().unwrap_or("").to_string(),
        cover: r0["cover"]["url_list"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        area: String::new(),
        online: fmt_num(
            r0["stats"]["total_user"]
                .as_i64()
                .or_else(|| r0["user_count"].as_i64())
                .unwrap_or(0),
        ),
        live,
        avatar: user["avatar_thumb"]["url_list"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
            replay: false,
        };

    let mut plays = Vec::new();
    if live {
        let su = &r0["stream_url"];
        let mut candidates: Vec<(&str, String)> = Vec::new();
        // FLV 先塞：sort_by_key 是稳定排序，同画质下 FLV 会排在 HLS 前面，
        // 于是 plays[0] 就是 FLV（抖音的 FLV 比 HLS 稳）。
        if let Some(map) = su["flv_pull_url"].as_object() {
            for (k, v) in map {
                if let Some(u) = v.as_str() {
                    candidates.push((k, u.to_string()));
                }
            }
        }
        if let Some(map) = su["hls_pull_url_map"].as_object() {
            for (k, v) in map {
                if let Some(u) = v.as_str() {
                    candidates.push((k, u.to_string()));
                }
            }
        }
        if let Some(u) = su["hls_pull_url"].as_str() {
            candidates.push(("HLS", u.to_string()));
        }

        // 原画优先
        candidates.sort_by_key(|(k, _)| {
            let k = k.to_uppercase();
            if k.contains("FULL_HD") || k.contains("ORIGIN") {
                0
            } else if k.contains("HD") {
                1
            } else {
                2
            }
        });

        for (k, u) in candidates {
            let format = if u.contains(".flv") { "flv" } else { "hls" };
            plays.push(PlayUrl {
                proxy: proxy::wrap(&u, hdr(&rid), false),
                url: u,
                format: format.into(),
                quality: k.to_string(),
                qualities: vec![],
            });
            if plays.len() >= 3 {
                break;
            }
        }
    }

    Ok(RoomDetail {
        room,
        description: String::new(),
        plays,
    })
}

fn parse_room(r: &Value) -> Room {
    // 分区列表 v2 接口把房间信息放在 room 里，web_rid 在外层
    let inner = if r.get("room").map(|v| !v.is_null()).unwrap_or(false) {
        &r["room"]
    } else {
        r
    };
    let rid = r["web_rid"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| inner["web_rid"].as_str().map(|s| s.to_string()))
        .or_else(|| inner["id_str"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    Room {
        platform: "douyin".into(),
        room_id: rid,
        title: inner["title"].as_str().unwrap_or("").to_string(),
        streamer: inner["owner"]["nickname"]
            .as_str()
            .or_else(|| inner["nickname"].as_str())
            .unwrap_or("")
            .to_string(),
        cover: inner["cover"]["url_list"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        area: inner["partition_name"]
            .as_str()
            .or_else(|| inner["cate_name"].as_str())
            .unwrap_or("")
            .to_string(),
        online: fmt_num(
            inner["stats"]["total_user"]
                .as_i64()
                .or_else(|| inner["user_count"].as_i64())
                .unwrap_or(0),
        ),
        live: true,
        avatar: inner["owner"]["avatar_thumb"]["url_list"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        replay: false,
    }
}

fn fmt_num(n: i64) -> String {
    if n >= 10000 {
        format!("{:.1}万", n as f64 / 10000.0)
    } else {
        n.to_string()
    }
}
