//! 哔哩哔哩直播
//! 参考：Simple Live / pure_live 的站点接口抓取思路

use crate::model::*;
use crate::net;
use crate::proxy;
use serde_json::Value;

const REFERER: &str = "https://live.bilibili.com/";

fn hdr() -> Vec<(String, String)> {
    vec![
        ("Referer".into(), REFERER.into()),
        ("Origin".into(), "https://live.bilibili.com".into()),
        ("User-Agent".into(), net::UA.into()),
    ]
}

pub async fn categories() -> Result<Vec<Category>, String> {
    let c = net::direct();
    let v: Value = c
        .get("https://api.live.bilibili.com/xlive/web-interface/v1/index/getWebAreaList?source_id=2")
        .header("Referer", REFERER)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    if let Some(arr) = v["data"]["data"].as_array() {
        for p in arr {
            let name = p["name"].as_str().unwrap_or("").to_string();
            if name.is_empty() || name == "全部" {
                continue;
            }
            let mut children = Vec::new();
            if let Some(list) = p["list"].as_array() {
                for c2 in list {
                    let cname = c2["name"].as_str().unwrap_or("").to_string();
                    if cname.is_empty() {
                        continue;
                    }
                    children.push(Category {
                        id: cname.clone(),
                        name: cname,
                        children: vec![],
                    });
                }
            }
            out.push(Category {
                id: name.clone(),
                name,
                children,
            });
        }
    }
    Ok(out)
}

/// 分区浏览：走搜索（分区名当关键词），绕开 -352 风控
pub async fn rooms(category: &str, page: u32) -> Result<RoomList, String> {
    search(category, page).await
}

pub async fn search(keyword: &str, page: u32) -> Result<RoomList, String> {
    let c = net::direct();
    // B站搜索接口现在强制要 wbi 签名，且路径要带 /wbi/，否则返回 -352
    let (ik, sk) = net::wbi_keys(&c).await?;
    let q = net::wbi_sign(
        vec![
            ("search_type", "live_room".to_string()),
            ("keyword", keyword.to_string()),
            ("page", page.to_string()),
        ],
        &ik,
        &sk,
    );
    let url = format!("https://api.bilibili.com/x/web-interface/wbi/search/type?{q}");
    let v: Value = c
        .get(&url)
        .header("User-Agent", net::BILI_UA)
        .header("Referer", "https://live.bilibili.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if v["code"].as_i64().unwrap_or(0) != 0 {
        eprintln!(
            "[bilibili] search code={} msg={}",
            v["code"],
            v["message"].as_str().unwrap_or("")
        );
    }
    let mut list = Vec::new();
    if let Some(arr) = v["data"]["result"].as_array() {
        for r in arr {
            let rid = r["roomid"].as_i64().unwrap_or(0);
            if rid == 0 {
                continue;
            }
            list.push(Room {
                platform: "bilibili".into(),
                room_id: rid.to_string(),
                title: strip_tags(r["title"].as_str().unwrap_or("")),
                streamer: r["uname"].as_str().unwrap_or("").to_string(),
                cover: fix_url(
                    r["cover"]
                        .as_str()
                        .or_else(|| r["user_cover"].as_str())
                        .or_else(|| r["pic"].as_str())
                        .unwrap_or(""),
                ),
                area: strip_tags(
                    r["area_name"]
                        .as_str()
                        .or_else(|| r["cate_name"].as_str())
                        .unwrap_or(""),
                ),
                online: fmt_num(r["online"].as_i64().unwrap_or(0)),
                live: r["live_status"].as_i64().unwrap_or(1) == 1,
                avatar: fix_url(r["uface"].as_str().unwrap_or("")),
                replay: false,
            });
        }
    }
    Ok(RoomList {
        rooms: list,
        page,
        has_more: true,
    })
}

/// 去掉搜索接口标题里的 <em class="keyword"> 高亮标签
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

pub async fn room_detail(room_id: &str) -> Result<RoomDetail, String> {
    let c = net::direct();

    let info: Value = c
        .get(format!(
            "https://api.live.bilibili.com/room/v1/Room/get_info?room_id={room_id}"
        ))
        .header("Referer", REFERER)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let real_id = info["data"]["room_id"].as_i64().unwrap_or(0);
    if real_id == 0 {
        return Err("房间不存在".into());
    }
    let live = info["data"]["live_status"].as_i64().unwrap_or(0) == 1;

    let anchor: Value = c
        .get(format!(
            "https://api.live.bilibili.com/live_user/v1/UserInfo/get_anchor_in_room?roomid={real_id}"
        ))
        .header("Referer", REFERER)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .unwrap_or(Value::Null);

    let room = Room {
        platform: "bilibili".into(),
        room_id: real_id.to_string(),
        title: info["data"]["title"].as_str().unwrap_or("").to_string(),
        streamer: anchor["data"]["info"]["uname"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        cover: fix_url(info["data"]["user_cover"].as_str().unwrap_or("")),
        area: info["data"]["area_name"].as_str().unwrap_or("").to_string(),
        online: fmt_num(info["data"]["online"].as_i64().unwrap_or(0)),
        live,
        avatar: fix_url(anchor["data"]["info"]["face"].as_str().unwrap_or("")),
        replay: false,
    };

    let mut plays = Vec::new();
    if live {
        match play_urls(real_id).await {
            Ok(mut p) => plays.append(&mut p),
            Err(e) => eprintln!("[bilibili] play url error: {e}"),
        }
    }

    Ok(RoomDetail {
        room,
        description: info["data"]["description"].as_str().unwrap_or("").to_string(),
        plays,
    })
}

pub async fn play_urls(rid: i64) -> Result<Vec<PlayUrl>, String> {
    let c = net::direct();
    let url = format!(
        "https://api.live.bilibili.com/xlive/web-room/v2/index/getRoomPlayInfo?room_id={rid}&protocol=0,1&format=0,1,2&codec=0,1&qn=10000&platform=web&ptype=8&dolby=5&panorama=1"
    );
    let v: Value = c
        .get(&url)
        .header("Referer", REFERER)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // (是否 HLS, 编码优先级, 播放地址)
    let mut raw: Vec<(u8, u8, PlayUrl)> = Vec::new();
    let streams = match v["data"]["playurl_info"]["playurl"]["stream"].as_array() {
        Some(s) => s.clone(),
        None => return Err("没有可用的播放地址".into()),
    };

    for st in &streams {
        let proto = st["protocol_name"].as_str().unwrap_or("");
        let formats = match st["format"].as_array() {
            Some(f) => f,
            None => continue,
        };
        for fmt in formats {
            let fname = fmt["format_name"].as_str().unwrap_or("");
            let codecs = match fmt["codec"].as_array() {
                Some(c) => c,
                None => continue,
            };
            for cd in codecs {
                let codec_name = cd["codec_name"].as_str().unwrap_or("");
                let base_url = cd["base_url"].as_str().unwrap_or("");
                let infos = match cd["url_info"].as_array() {
                    Some(i) => i,
                    None => continue,
                };
                if let Some(ui) = infos.first() {
                    let host = ui["host"].as_str().unwrap_or("");
                    let extra = ui["extra"].as_str().unwrap_or("");
                    let full = format!("{host}{base_url}{extra}");
                    if full.is_empty() {
                        continue;
                    }
                    let is_hls = proto.contains("hls")
                        || fname.contains("ts")
                        || fname.contains("fmp4");
                    // 浏览器 MSE 只稳定支持 H.264，HEVC / AV1 降权排到后面
                    let codec_rank: u8 = match codec_name {
                        "avc" => 0,
                        "hevc" => 2,
                        "av1" => 3,
                        _ => 1,
                    };
                    let q = format!(
                        "{} · {}{}",
                        if is_hls { "HLS" } else { "FLV" },
                        fname,
                        if codec_name.is_empty() || codec_name == "avc" {
                            String::new()
                        } else {
                            format!(" · {}", codec_name.to_uppercase())
                        }
                    );
                    raw.push((
                        // FLV 优先：浏览器对 FLV（mpegts.js）比 HLS 稳，且不会撞 HEVC 的 MSE 限制
                        if is_hls { 1u8 } else { 0u8 },
                        codec_rank,
                        PlayUrl {
                            proxy: proxy::wrap(&full, hdr(), false),
                            url: full,
                            format: if is_hls { "hls".into() } else { "flv".into() },
                            quality: q,
                            qualities: vec![],
                        },
                    ));
                    break; // 每个 codec 只取第一个可用 host
                }
            }
        }
    }

    raw.sort_by_key(|(h, c, _)| (*h, *c));
    let out: Vec<PlayUrl> = raw.into_iter().map(|(_, _, p)| p).collect();
    if out.is_empty() {
        return Err("没有可用的播放地址".into());
    }
    Ok(out)
}

fn fix_url(u: &str) -> String {
    if u.starts_with("//") {
        format!("https:{u}")
    } else if u.starts_with("http://") {
        u.replacen("http://", "https://", 1)
    } else {
        u.to_string()
    }
}

fn fmt_num(n: i64) -> String {
    if n >= 10000 {
        format!("{:.1}万", n as f64 / 10000.0)
    } else {
        n.to_string()
    }
}
