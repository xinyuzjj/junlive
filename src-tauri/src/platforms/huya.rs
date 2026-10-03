//! 虎牙直播

use crate::model::*;
use crate::net;
use crate::proxy;
use rand::Rng;
use serde_json::Value;

fn referer(rid: &str) -> String {
    format!("https://www.huya.com/{rid}")
}

fn hdr(rid: &str) -> Vec<(String, String)> {
    vec![
        ("Referer".into(), referer(rid)),
        ("Origin".into(), "https://www.huya.com".into()),
        ("User-Agent".into(), net::UA.into()),
    ]
}

const LIST_API: &str =
    "https://www.huya.com/cache.php?m=LiveList&do=getLiveListByPage&tagAll=0&pageSize=120";

pub async fn categories() -> Result<Vec<Category>, String> {
    let c = net::direct();
    let v: Value = c
        .get(format!("{LIST_API}&page=1"))
        .header("Referer", "https://www.huya.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // 这个接口给的是 gameHostName（分类 ID）+ gameFullName（分类名）。
    // 但真正的房间过滤要靠 liveHttpUI 的 iGid，而它只认数字，
    // 所以这里把 ID 统一成数字：数字的照抄，字母的只保留能对上号的几个，
    // 其余先不显示——点了也是空列表，不如不给。
    let mut seen: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    if let Some(arr) = v["data"]["datas"].as_array() {
        for r in arr {
            let host = r["gameHostName"].as_str().unwrap_or("");
            let name = r["gameFullName"].as_str().unwrap_or("");
            if host.is_empty() || name.is_empty() {
                continue;
            }
            let gid = match host {
                h if h.chars().all(|ch| ch.is_ascii_digit()) => h.to_string(),
                "lol" => "1".to_string(),
                "wzry" => "100004".to_string(),
                _ => continue,
            };
            seen.entry(gid).or_insert(name.to_string());
        }
    }

    let mut out = vec![Category {
        id: "all".into(),
        name: "全部".into(),
        children: vec![],
    }];
    for (gid, name) in seen {
        out.push(Category {
            id: gid,
            name,
            children: vec![],
        });
    }
    Ok(out)
}

pub async fn rooms(category: &str, page: u32) -> Result<RoomList, String> {
    let c = net::direct();

    // 指定分区：走 liveHttpUI（只有它认 iGid 过滤），字段是 vList / lProfileRoom 那一套。
    // 原来的 cache.php 虽然也能带 gid，但服务端会忽略它，永远返回同样的热门房间。
    if !category.is_empty() && category != "0" && category != "all" {
        let v: Value = c
            .get(format!(
                "https://live.huya.com/liveHttpUI/getLiveList?iGid={category}&iPageNo={page}&iPageSize=120"
            ))
            .header("Referer", "https://www.huya.com/")
            .header("User-Agent", net::UA)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        if let Some(arr) = v["vList"].as_array() {
            for r in arr {
                let rid = r["lProfileRoom"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                if rid.is_empty() {
                    continue;
                }
                list.push(Room {
                    platform: "huya".into(),
                    room_id: rid,
                    title: {
                        let t = r["sIntroduction"].as_str().unwrap_or("");
                        if t.is_empty() {
                            r["sRoomName"].as_str().unwrap_or("").to_string()
                        } else {
                            t.to_string()
                        }
                    },
                    streamer: r["sNick"].as_str().unwrap_or("").to_string(),
                    cover: r["sScreenshot"].as_str().unwrap_or("").to_string(),
                    area: r["sGameFullName"].as_str().unwrap_or("").to_string(),
                    online: fmt_num(
                        r["lUserCount"]
                            .as_i64()
                            .or_else(|| r["lActivityCount"].as_i64())
                            .unwrap_or(0),
                    ),
                    live: true,
                    avatar: r["sAvatar180"].as_str().unwrap_or("").to_string(),
                });
            }
        }
        return Ok(RoomList {
            has_more: !list.is_empty(),
            rooms: list,
            page,
        });
    }

    // 「全部」：沿用原来的 cache.php
    let v: Value = c
        .get(format!("{LIST_API}&page={page}"))
        .header("Referer", "https://www.huya.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    if let Some(arr) = v["data"]["datas"].as_array() {
        for r in arr {
            let rid = r["profileRoom"].as_str().unwrap_or("").to_string();
            if rid.is_empty() {
                continue;
            }
            list.push(Room {
                platform: "huya".into(),
                room_id: rid,
                title: {
                    let t = r["roomName"].as_str().unwrap_or("");
                    if t.is_empty() {
                        r["introduction"].as_str().unwrap_or("").to_string()
                    } else {
                        t.to_string()
                    }
                },
                streamer: r["nick"].as_str().unwrap_or("").to_string(),
                cover: r["screenshot"].as_str().unwrap_or("").to_string(),
                area: r["gameFullName"].as_str().unwrap_or("").to_string(),
                online: fmt_num(
                    r["totalCount"]
                        .as_i64()
                        .or_else(|| r["totalCount"].as_str().and_then(|s| s.parse().ok()))
                        .unwrap_or(0),
                ),
                live: true,
                avatar: r["avatar180"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    let has_more = !list.is_empty();
    Ok(RoomList {
        rooms: list,
        page,
        has_more,
    })
}

pub async fn search(keyword: &str, page: u32) -> Result<RoomList, String> {
    let c = net::direct();
    let kw = urlencoding::encode(keyword);
    let start = (page.saturating_sub(1)) * 20;
    let url = format!(
        "https://search.cdn.huya.com/?m=Search&do=getSearchContent&q={kw}&uid=0&v=4&typ=-5&livestate=0&rows=20&start={start}"
    );
    let v: Value = c
        .get(&url)
        .header("Referer", "https://www.huya.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    if let Some(groups) = v["response"].as_object() {
        for (_k, g) in groups {
            if let Some(docs) = g["docs"].as_array() {
                for r in docs {
                    let rid = r["room_id"]
                        .as_str()
                        .map(|s| s.to_string())
                        .or_else(|| r["room_id"].as_i64().map(|n| n.to_string()))
                        .unwrap_or_default();
                    if rid.is_empty() {
                        continue;
                    }
                    list.push(Room {
                        platform: "huya".into(),
                        room_id: rid,
                        title: r["live_intro"]
                            .as_str()
                            .or_else(|| r["room_name"].as_str())
                            .unwrap_or("")
                            .to_string(),
                        streamer: r["game_nick"]
                            .as_str()
                            .or_else(|| r["nick"].as_str())
                            .unwrap_or("")
                            .to_string(),
                        cover: r["game_avatarUrl180"]
                            .as_str()
                            .or_else(|| r["screenshot"].as_str())
                            .unwrap_or("")
                            .to_string(),
                        area: r["game_name"].as_str().unwrap_or("").to_string(),
                        online: fmt_num(
                            r["game_activityCount"]
                                .as_i64()
                                .or_else(|| {
                                    r["total_count"].as_str().and_then(|s| s.parse::<i64>().ok())
                                })
                                .unwrap_or(0),
                        ),
                        live: r["gameLiveOn"].as_bool().unwrap_or(true),
                        avatar: r["game_avatarUrl180"].as_str().unwrap_or("").to_string(),
                    });
                }
            }
        }
    }
    Ok(RoomList {
        rooms: list,
        page,
        has_more: false,
    })
}

pub async fn room_detail(room_id: &str) -> Result<RoomDetail, String> {
    let c = net::direct();
    let v: Value = c
        .get(format!(
            "https://mp.huya.com/cache.php?m=Live&do=profileRoom&roomid={room_id}"
        ))
        .header("Referer", referer(room_id))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let d = &v["data"];
    if d.is_null() {
        return Err("房间不存在".into());
    }
    let live = d["liveStatus"].as_str().unwrap_or("") == "ON"
        || d["liveStatus"].as_i64() == Some(1)
        || d["liveStatus"].as_bool() == Some(true);

    let room = Room {
        platform: "huya".into(),
        room_id: room_id.to_string(),
        title: d["liveData"]["introduction"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        streamer: d["liveData"]["nick"].as_str().unwrap_or("").to_string(),
        cover: d["liveData"]["screenshot"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        area: d["liveData"]["gameFullName"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        online: fmt_num(d["liveData"]["totalCount"].as_i64().unwrap_or(0)),
        live,
        avatar: d["liveData"]["avatar180"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    };

    let mut plays = Vec::new();
    if live {
        if let Some(mut p) = extract_plays(d, room_id) {
            plays.append(&mut p);
        }
    }

    Ok(RoomDetail {
        room,
        description: String::new(),
        plays,
    })
}


// ---------------------------------------------------------------- 虎牙 anti_code 重算
//
// 虎牙返回的 sFlvAntiCode 里 wsSecret 是「请求那一刻」算出来的，过一会儿就失效——
// 表现就是播几十秒后断开、或者卡在「缓存中」。
// DTV 的做法是每次（重）连接都用当前毫秒重算一遍：
//   seqid    = uid + 当前毫秒
//   secret   = md5(seqid|ctype|t)
//   wsSecret = md5(fm前缀_旋转后的uid_流名_secret_wsTime)
// 重算出来的签名才配得上 codec=264（拿 H.264 而不是 H.265）。

fn md5_hex(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

/// uid 的低 32 位循环左移 8 位，高位原样保留
fn rotl32_by8(v: i64) -> i64 {
    let low = (v as u64 & 0xFFFF_FFFF) as u32;
    let rotated = low.rotate_left(8) as i64;
    (v & !0xFFFF_FFFFi64) | rotated
}

fn build_huya_anti_code(stream_name: &str, uid: i64, anti_code: &str) -> Option<String> {
    let cleaned = anti_code.replace("&amp;", "&");
    let cleaned = cleaned.trim_start_matches(['?', '&']);
    let params: std::collections::HashMap<String, String> = cleaned
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let fm_raw = params.get("fm")?.clone();
    let ctype = params
        .get("ctype")
        .cloned()
        .unwrap_or_else(|| "huya_pc_exe".to_string());
    let platform_id: i64 = params.get("t").and_then(|v| v.parse().ok()).unwrap_or(0);
    let is_wap = platform_id == 103;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let seq_id = uid + now_ms;
    let secret_hash = md5_hex(&format!("{seq_id}|{ctype}|{platform_id}"));
    let convert_uid = rotl32_by8(uid);
    let calc_uid = if is_wap { uid } else { convert_uid };

    let fm_decoded = urlencoding::decode(&fm_raw).ok()?.to_string();
    use base64::Engine;
    let fm_bytes = base64::engine::general_purpose::STANDARD
        .decode(fm_decoded.as_bytes())
        .ok()?;
    let fm_plain = String::from_utf8_lossy(&fm_bytes).to_string();
    let secret_prefix = fm_plain.split('_').next().unwrap_or("").to_string();
    if secret_prefix.is_empty() {
        return None;
    }

    let ws_time = params.get("wsTime")?.clone();
    let ws_secret = md5_hex(&format!(
        "{secret_prefix}_{calc_uid}_{stream_name}_{secret_hash}_{ws_time}"
    ));
    let fs = params.get("fs")?.clone();

    let ws_time_int = i64::from_str_radix(ws_time.trim(), 16).unwrap_or(0);
    let mut rng = rand::thread_rng();
    let ct = (((ws_time_int as f64) + rng.gen::<f64>()) * 1000.0) as i64;
    let uuid = (((((ct % 10_000_000_000i64) as f64) + rng.gen::<f64>()) * 1000.0)
        % (0xFFFF_FFFFu64 as f64)) as u32;

    let mut parts: Vec<(String, String)> = vec![
        ("wsSecret".to_string(), ws_secret),
        ("wsTime".to_string(), ws_time),
        ("seqid".to_string(), seq_id.to_string()),
        ("ctype".to_string(), ctype),
        ("ver".to_string(), "1".to_string()),
        ("fs".to_string(), fs),
        ("fm".to_string(), urlencoding::encode(&fm_raw).to_string()),
        ("t".to_string(), platform_id.to_string()),
    ];
    if is_wap {
        parts.push(("uid".to_string(), uid.to_string()));
        parts.push(("uuid".to_string(), uuid.to_string()));
    } else {
        parts.push(("u".to_string(), convert_uid.to_string()));
    }
    Some(
        parts
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&"),
    )
}

fn extract_plays(d: &Value, room_id: &str) -> Option<Vec<PlayUrl>> {
    let mut out = Vec::new();
    let headers = hdr(room_id);

    // 虎牙的 HLS 地址实测 403（DTV 也完全不用 HLS，只走 FLV），这里直接跳过。

    // 2) 拼接 baseSteamInfoList
    if let Some(list) = d["stream"]["baseSteamInfoList"].as_array() {
        for st in list {
            let name = st["sStreamName"].as_str().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let flv_base = st["sFlvUrl"].as_str().unwrap_or("").trim_end_matches('/');
            let flv_suffix = st["sFlvUrlSuffix"].as_str().unwrap_or("flv");
            let flv_code = st["sFlvAntiCode"].as_str().unwrap_or("").replace("&amp;", "&");
            if !flv_base.is_empty() {
                // 重算 anti_code（不然签名过期会断流），再加 codec=264 拿 H.264
                let uid = d["profileInfo"]["yyid"]
                    .as_i64()
                    .or_else(|| d["liveData"]["uid"].as_i64())
                    .or_else(|| d["profileInfo"]["lUid"].as_i64())
                    .unwrap_or(0);
                let anti = build_huya_anti_code(name, uid, &flv_code)
                    .unwrap_or_else(|| flv_code.clone());
                let url = format!("{flv_base}/{name}.{flv_suffix}?{anti}&codec=264");
                // DTV 的做法：一律转 HTTPS，避免 webview 里的混合内容限制
                let url = match url.strip_prefix("http://") {
                    Some(rest) => format!("https://{rest}"),
                    None => url,
                };
                out.push(PlayUrl {
                    // 虎牙是短连接：走带重连的代理
                    proxy: proxy::wrap_flv(&url, headers.clone(), false),
                    url,
                    format: "flv".into(),
                    quality: "FLV 原画".into(),
                });
            }
            // HLS 跳过：403，且 DTV 只用 FLV
        }
    }

    if out.is_empty() {
        None
    } else {
        // 虎牙的 HLS anti_code 常返回 403，FLV 实测最稳，放前面
        out.sort_by_key(|p| if p.format == "flv" { 0 } else { 1 });
        Some(out)
    }
}

fn fmt_num(n: i64) -> String {
    if n >= 10000 {
        format!("{:.1}万", n as f64 / 10000.0)
    } else {
        n.to_string()
    }
}
