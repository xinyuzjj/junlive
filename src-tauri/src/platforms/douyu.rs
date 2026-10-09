//! 斗鱼直播

use crate::model::*;
use crate::net;
use crate::proxy;
use rand::Rng;
use serde_json::Value;

fn referer(rid: &str) -> String {
    format!("https://www.douyu.com/{rid}")
}

fn hdr(rid: &str) -> Vec<(String, String)> {
    vec![
        ("Referer".into(), referer(rid)),
        ("Origin".into(), "https://www.douyu.com".into()),
        ("User-Agent".into(), net::UA.into()),
    ]
}

/// 斗鱼 web 端固定设备 id（streamlink 的 douyu 插件用同一常量）
const DID: &str = "10000000000000000000000000001501";





#[allow(dead_code)]
fn random_did() -> String {
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| char::from_digit(rng.gen_range(0..10), 10).unwrap())
        .collect()
}

/// 主播的直播回放列表。
///
/// 两步：
///   ① `betard/{room_id}` 拿 `room.up_id`（主播加密 ID）
///   ② `v.douyu.com/wgapi/vod/center/authorShowVideoList?up_id=...` 拿回放
///
/// 注意这个接口**只认 up_id**，跟直播间是两套域名（www vs v）。
/// 返回的是一「场」直播（`video_list` 里按约 2 小时切成多段），
/// 这里拍平成一段一条，前端不用关心分段。
pub async fn replays(room_id: &str, page: u32) -> Result<ReplayPage, String> {
    let c = net::direct();

    let v: Value = c
        .get(format!("https://www.douyu.com/betard/{room_id}"))
        .header("Referer", referer(room_id))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let up_id = v["room"]["up_id"].as_str().unwrap_or("").to_string();
    if up_id.is_empty() {
        return Err("该主播没有开放回放".into());
    }

    let url = format!(
        "https://v.douyu.com/wgapi/vod/center/authorShowVideoList?up_id={up_id}&page={page}&limit=20"
    );
    let v: Value = c
        .get(&url)
        .header("Referer", "https://v.douyu.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if v["error"].as_i64().unwrap_or(-1) != 0 {
        return Err(format!(
            "斗鱼回放接口返回异常：{}",
            v["msg"].as_str().unwrap_or("未知错误")
        ));
    }

    // **一场直播 = 一条记录**，不是一段一条。
    //
    // 斗鱼把一场直播切成若干 2 小时的段（放在 `video_list`，但只给第一段），
    // 完整分段要另外调 `getShowReplayList`。以前按段拍平，结果是
    // 「同一场标题重复出现 7 次、每条都显示 2 小时」，看着就像总时长错了。
    // 现在取 `replay_duration`（整场总长）作为时长，段数等播放时再查。
    let total = v["data"]["count"].as_u64().unwrap_or(0) as u32;
    let mut out = Vec::new();
    if let Some(shows) = v["data"]["list"].as_array() {
        for show in shows {
            let time = show["time"].as_str().unwrap_or("").to_string();
            let Some(vids) = show["video_list"].as_array() else {
                continue;
            };
            // 用第一段当「入场券」：它的 hash 既能解析播放地址，
            // 也能拿它去换整场的分段列表。
            let Some(x) = vids.first() else { continue };
            let hash = x["hash_id"].as_str().unwrap_or("");
            if hash.is_empty() {
                continue;
            }
            let dur = show["replay_duration"].as_str().unwrap_or("");
            out.push(Replay {
                hash_id: hash.to_string(),
                title: x["title"].as_str().unwrap_or("").to_string(),
                cover: x["video_pic"].as_str().unwrap_or("").to_string(),
                duration: if dur.is_empty() {
                    x["video_str_duration"].as_str().unwrap_or("").to_string()
                } else {
                    dur.to_string()
                },
                time,
                view_num: x["view_num"].as_str().unwrap_or("").to_string(),
                start_time: x["start_time"].as_i64().unwrap_or(0),
                parts: 0,
            });
        }
    }
    Ok(ReplayPage { list: out, total })
}

/// 取主播的加密 ID（`up_id`）—— 回放的几个接口都要它。
async fn up_id_of(room_id: &str) -> Result<String, String> {
    let c = net::direct();
    let v: Value = c
        .get(format!("https://www.douyu.com/betard/{room_id}"))
        .header("Referer", referer(room_id))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let up = v["room"]["up_id"].as_str().unwrap_or("").to_string();
    if up.is_empty() {
        return Err("该主播没有开放回放".into());
    }
    Ok(up)
}

/// 一场直播的**完整分段列表**。
///
/// 斗鱼把一场直播切成若干约 2 小时的段：`authorShowVideoList` 只给第一段，
/// 完整列表要拿任意一段的 hash + 主播 up_id 去 `getShowReplayList` 换。
/// 返回的第一条就是这场的第一段（按时间顺序）。
/// `show_start`：这一场的起始 Unix 秒（列表里那一条的 `start_time`）。
///
/// 分段接口**不给每段的起始时间**，但各段时长是首尾相接的，
/// 所以按「前几段时长累加」就能推出每段起点 —— 实测 7 段加起来
/// 47 分 29 秒 ≈ 场次总长 13:13:52，对得上。
pub async fn replay_parts(
    room_id: &str,
    hash_id: &str,
    show_start: i64,
) -> Result<Vec<Replay>, String> {
    let up = up_id_of(room_id).await?;
    let c = net::direct();
    let url = format!(
        "https://v.douyu.com/wgapi/vod/center/getShowReplayList?vid={hash_id}&up_id={up}"
    );
    let v: Value = c
        .get(&url)
        .header("Referer", "https://v.douyu.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if v["error"].as_i64().unwrap_or(-1) != 0 {
        return Err("斗鱼回放分段接口返回异常".into());
    }
    let mut out: Vec<Replay> = Vec::new();
    let mut acc: i64 = 0; // 前面各段时长之和
    if let Some(list) = v["data"]["list"].as_array() {
        for x in list {
            let hash = x["hash_id"].as_str().unwrap_or("");
            if hash.is_empty() {
                continue;
            }
            let dur = x["video_duration"].as_str().unwrap_or("");
            out.push(Replay {
                hash_id: hash.to_string(),
                title: x["title"].as_str().unwrap_or("").to_string(),
                cover: x["cover"].as_str().unwrap_or("").to_string(),
                duration: dur.to_string(),
                time: x["show_remark"].as_str().unwrap_or("").to_string(),
                view_num: x["view_num"].as_i64().unwrap_or(0).to_string(),
                start_time: if show_start > 0 { show_start + acc } else { 0 },
                parts: 0,
            });
            acc += dur_secs(dur);
        }
    }
    let total = out.len() as u32;
    for r in out.iter_mut() {
        r.parts = total;
    }
    Ok(out)
}

/// "02:00:05" / "120:05" → 秒
fn dur_secs(s: &str) -> i64 {
    let p: Vec<i64> = s.split(':').filter_map(|x| x.trim().parse().ok()).collect();
    match p.len() {
        3 => p[0] * 3600 + p[1] * 60 + p[2],
        2 => p[0] * 60 + p[1],
        _ => 0,
    }
}

/// 斗鱼的弹幕颜色是**枚举值**不是 RGB（0 默认 / 1 红 / 2 橙 …）。
fn dm_color(v: i64) -> String {
    match v {
        1 => "#ff4d4f",
        2 => "#ff8c00",
        3 => "#ffd700",
        4 => "#00c853",
        5 => "#40a9ff",
        6 => "#b37feb",
        7 => "#ff85c0",
        _ => "",
    }
    .to_string()
}

/// 一场回放的**历史弹幕**。
///
/// 接口按**毫秒区间**取，每次最多 500 条，所以要一页页往后翻
/// （用返回的 `end_time` 当下一次的 `start_time`）。
///
/// 每条给的是绝对时间戳 `sts`，减掉视频起始时间 `start_time` 才是视频内进度；
/// 拿不到 `start_time` 时退而用第一页的 `sts - start_time/1000` 反推。
pub async fn replay_danmaku(hash_id: &str, start_time: i64) -> Result<Vec<ReplayDanmaku>, String> {
    let c = net::direct();
    let mut out: Vec<ReplayDanmaku> = Vec::new();
    let mut start: i64 = 0;
    let mut base: Option<i64> = None;

    // 最多 8 页 × 500 条 = 4000 条，够一场看；再多也没意义（弹幕太密会刷屏）
    for _ in 0..8 {
        let url = format!(
            "https://v.douyu.com/wgapi/vod/center/getBarrageList?vid={hash_id}&start_time={start}&end_time=-1"
        );
        let v: Value = c
            .get(&url)
            .header("Referer", "https://v.douyu.com/")
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if v["error"].as_i64().unwrap_or(-1) != 0 {
            break;
        }
        let d = &v["data"];
        let Some(list) = d["list"].as_array() else { break };
        if list.is_empty() {
            break;
        }
        if base.is_none() {
            let first_sts = list[0]["sts"].as_i64().unwrap_or(0);
            let st_ms = d["start_time"].as_i64().unwrap_or(0);
            base = Some(if start_time > 0 {
                start_time
            } else {
                first_sts - st_ms / 1000
            });
        }
        let b = base.unwrap_or(0);
        for x in list {
            let text = x["ctt"].as_str().unwrap_or("");
            if text.is_empty() {
                continue;
            }
            let sts = x["sts"].as_i64().unwrap_or(b);
            out.push(ReplayDanmaku {
                time: ((sts - b) as f64).max(0.0),
                text: text.to_string(),
                color: dm_color(x["col"].as_i64().unwrap_or(0)),
                user: x["nn"].as_str().unwrap_or("").to_string(),
            });
        }
        let end = d["end_time"].as_i64().unwrap_or(0);
        if end <= start {
            break;
        }
        start = end;
    }
    out.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}

/// 一场回放的 **AI 看点**（分段摘要 + 起止时间）。
///
/// 走 `aivideo/mainV1`：返回若干「段落」，每段有标题、摘要和起止**绝对时间戳**，
/// 减掉视频起始时间就是进度。拿不到就返回空列表（前端不显示这一块）。
pub async fn replay_highlights(
    hash_id: &str,
    start_time: i64,
    duration: i64,
) -> Result<Vec<ReplayHighlight>, String> {
    let c = net::direct();
    let url = format!("https://v.douyu.com/wgapi/vod/center/aivideo/mainV1?vid={hash_id}");
    let v: Value = c
        .get(&url)
        .header("Referer", "https://v.douyu.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let Some(arr) = v["data"].as_array() else {
        return Ok(out);
    };
    for group in arr {
        let Some(list) = group["list"].as_array() else { continue };
        for x in list {
            let title = x["firstTitle"].as_str().unwrap_or("");
            if title.is_empty() {
                continue;
            }
            let st = x["startTime"].as_i64().unwrap_or(0);
            // **AI 看点是整场的**（实测跨 11 小时），而一段只有 2 小时。
            // 必须减掉「这一段」的起点，并且把不属于这一段的丢掉 ——
            // 否则超出的那些在进度条上全挤到右边外面去。
            let off = (st - start_time) as f64;
            if duration > 0 && (off < 0.0 || off >= duration as f64) {
                continue;
            }
            out.push(ReplayHighlight {
                time: if start_time > 0 && st > 0 { off.max(0.0) } else { 0.0 },
                title: title.to_string(),
                desc: x["describe"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}

pub async fn categories() -> Result<Vec<Category>, String> {
    let c = net::direct();
    let v: Value = c
        .get("https://m.douyu.com/api/cate/list")
        .header("Referer", "https://www.douyu.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    if let Some(arr) = v["data"]["cate2Info"].as_array() {
        for x in arr {
            let c1 = x["cate1Id"].as_i64().unwrap_or(1);
            let c2 = x["cate2Id"].as_i64().unwrap_or(0);
            let name = x["cate2Name"].as_str().unwrap_or("").to_string();
            if c2 == 0 || name.is_empty() {
                continue;
            }
            out.push(Category {
                id: format!("{c1}_{c2}"),
                name,
                children: vec![],
            });
        }
    }
    if out.is_empty() {
        return Err("斗鱼分类接口没有返回数据".into());
    }
    Ok(out)
}

pub async fn rooms(category: &str, page: u32) -> Result<RoomList, String> {
    let c = net::direct();
    // category 形如 "1_1"（cate1_cate2）；兼容只传 cate2 的情况
    let cate2 = category
        .split('_')
        .next_back()
        .unwrap_or(category)
        .to_string();
    let url = format!("https://www.douyu.com/gapi/rkc/directory/mixList/2_{cate2}/{page}");
    let v: Value = c
        .get(&url)
        .header("Referer", "https://www.douyu.com/directory")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    if let Some(arr) = v["data"]["rl"].as_array() {
        for r in arr {
            let rid = r["rid"].as_i64().unwrap_or(0);
            if rid == 0 {
                continue;
            }
            list.push(Room {
                platform: "douyu".into(),
                room_id: rid.to_string(),
                title: r["rn"].as_str().unwrap_or("").to_string(),
                streamer: r["nn"].as_str().unwrap_or("").to_string(),
                cover: r["rs16"].as_str().unwrap_or("").to_string(),
                area: r["c2name"].as_str().unwrap_or("").to_string(),
                online: fmt_num(r["ol"].as_i64().unwrap_or(0)),
                live: true,
                avatar: av_url(r["av"].as_str().unwrap_or("")),
                replay: false,
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
    // 斗鱼的老搜索接口（japi/search/api/searchLive）早就 404 了，移动端那几个也只剩
    // HTML 壳。现在能用的只有两个，合起来用：
    //   1. getSearchRecV2 —— 精确匹配，带 rid/nickName/avatar/cateName/isLive（约 6~10 条）
    //   2. searchWordRec  —— 相关直播间（约 30 条），但只有 bizId + kw
    // 两个都按房间号去重。
    let c = net::direct();
    let kw = urlencoding::encode(keyword);
    let mut list: Vec<Room> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // ---- 1) 精确结果 ----
    if let Ok(resp) = c
        .get(format!(
            "https://www.douyu.com/japi/search/api/getSearchRecV2?kw={kw}"
        ))
        .header("Referer", "https://www.douyu.com/")
        .send()
        .await
    {
        if let Ok(v) = resp.json::<Value>().await {
            for it in v["data"].as_array().cloned().unwrap_or_default() {
                let r = &it["roomResult"];
                let rid = r["rid"].as_i64().unwrap_or(0);
                if rid == 0 {
                    continue;
                }
                let id = rid.to_string();
                if !seen.insert(id.clone()) {
                    continue;
                }
                list.push(Room {
                    platform: "douyu".into(),
                    room_id: id,
                    title: it["kw"].as_str().unwrap_or("").to_string(),
                    streamer: r["nickName"].as_str().unwrap_or("").to_string(),
                    cover: String::new(),
                    area: r["cateName"].as_str().unwrap_or("").to_string(),
                    online: String::new(),
                    // isLive: 1 在播 / 2 未开播
                    live: r["isLive"].as_i64().unwrap_or(0) == 1,
                    avatar: av_url(r["avatar"].as_str().unwrap_or("")),
                    replay: false,
                });
            }
        }
    }

    // ---- 2) 相关直播间（补量） ----
    if let Ok(resp) = c
        .get(format!(
            "https://www.douyu.com/wgapi/livenc/search/searchWordRec?kw={kw}"
        ))
        .header("Referer", "https://www.douyu.com/search/")
        .send()
        .await
    {
        if let Ok(v) = resp.json::<Value>().await {
            for it in v["data"]["list"].as_array().cloned().unwrap_or_default() {
                // bizType=2 才是直播间
                if it["bizType"].as_i64() != Some(2) {
                    continue;
                }
                let rid = it["bizId"].as_i64().unwrap_or(0);
                if rid == 0 {
                    continue;
                }
                let id = rid.to_string();
                if !seen.insert(id.clone()) {
                    continue;
                }
                // schemaUrl 里带了 roomSrc（封面），抠出来用
                let schema = it["schemaUrl"].as_str().unwrap_or("");
                let cover = urlencoding::decode(
                    schema
                        .split("roomSrc=")
                        .nth(1)
                        .unwrap_or("")
                        .split('&')
                        .next()
                        .unwrap_or(""),
                )
                .map(|s| s.to_string())
                .unwrap_or_default();
                list.push(Room {
                    platform: "douyu".into(),
                    room_id: id,
                    title: it["kw"].as_str().unwrap_or("").to_string(),
                    streamer: it["kw"].as_str().unwrap_or("").to_string(),
                    cover,
                    area: String::new(),
                    online: String::new(),
                    live: it["showStatus"].as_i64().unwrap_or(0) == 1,
                    avatar: String::new(),
                    replay: false,
                });
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
    let info: Value = c
        .get(format!("https://www.douyu.com/betard/{room_id}"))
        .header("Referer", referer(room_id))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let r = &info["room"];
    if r.is_null() {
        return Err("房间不存在".into());
    }
    let live = r["show_status"].as_i64().unwrap_or(0) == 1;
    let room = Room {
        platform: "douyu".into(),
        room_id: r["room_id"].as_i64().unwrap_or(0).to_string(),
        title: r["room_name"].as_str().unwrap_or("").to_string(),
        streamer: r["owner_name"]
            .as_str()
            .or_else(|| r["nickname"].as_str())
            .unwrap_or("")
            .to_string(),
        cover: r["room_pic"].as_str().unwrap_or("").to_string(),
        area: r["cate_name"].as_str().unwrap_or("").to_string(),
        online: fmt_num(r["hn"].as_i64().unwrap_or(0)),
        live,
        // betard 的 room.avatar 是**对象** {big,mid,small} 不是字符串，
        // 直接 as_str() 取不到 → 头像空。按 mid → owner_avatar → 对象内的键兜底。
        avatar: av_url(
            r["avatar_mid"]
                .as_str()
                .or_else(|| r["owner_avatar"].as_str())
                .or_else(|| r["avatar"]["mid"].as_str())
                .or_else(|| r["avatar"]["big"].as_str())
                .unwrap_or(""),
        ),
        // 录播（视频轮播）：show_status 仍是 1、也确实在推流，但内容是录像。
        // 只有 betard 有这个字段，分类列表接口（mixList）没有。
        replay: r["videoLoop"].as_i64().unwrap_or(0) == 1,
    };

    let mut plays = Vec::new();
    if live {
        match play_urls(room_id).await {
            Ok(mut p) => plays.append(&mut p),
            Err(e) => eprintln!("[douyu] play url error: {e}"),
        }
    }

    Ok(RoomDetail {
        room,
        description: String::new(),
        plays,
    })
}

/// 斗鱼 web 播放地址的签名流程（参考 streamlink 的 douyu 插件实现）
pub async fn play_urls(room_id: &str) -> Result<Vec<PlayUrl>, String> {
    let c = net::direct();
    let did = DID;

    // 1) 取加密参数
    let resp = c
        .get("https://www.douyu.com/wgapi/livenc/liveweb/websec/getEncryption")
        .query(&[("did", did)])
        .header("Referer", referer(room_id))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    // 时间戳用响应头 Date（服务端时间，避免本机时钟偏差）
    let ts = resp
        .headers()
        .get("date")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_http_date)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let enc = &v["data"];
    if v["error"].as_i64().unwrap_or(-1) != 0 || enc.is_null() {
        return Err("斗鱼加密参数获取失败".into());
    }
    let key = enc["key"].as_str().unwrap_or("");
    let rand_str = enc["rand_str"].as_str().unwrap_or("");
    let enc_time = enc["enc_time"].as_i64().unwrap_or(0);
    let enc_data = enc["enc_data"].as_str().unwrap_or("");
    let is_special = enc["is_special"].as_i64().unwrap_or(0);

    // 2) 计算 auth
    let suffix = if is_special == 1 {
        String::new()
    } else {
        format!("{room_id}{ts}")
    };
    let mut f = rand_str.to_string();
    for _ in 0..enc_time.max(0) {
        f = md5_hex(&format!("{f}{key}"));
    }
    let auth = md5_hex(&format!("{f}{key}{suffix}"));

    // 3) 逐档请求，按**接口实际返回的档位**命名并去重。
    //
    // 关键坑：斗鱼会按房间降级。实测同一批房间里，有的请求 rate=0（原画）
    // 真的给原画（rtmp_live 无码率后缀、data.rate=0），有的却回
    // data.rate=4、rtmp_live 带 _4000 后缀 —— 也就是**只给蓝光4M**。
    //
    // 早期代码无条件把第一条标成「原画」，于是 UI 上写着原画、实际播的是
    // 4M；而且原画/蓝光8M/蓝光4M 三条都指向同一个流（三个重复条目）。
    // 现在一律以 data.rate 为准 —— 拿不到原画就不列原画，不骗用户。
    let ts_s = ts.to_string();
    let first = fetch_play(&c, room_id, enc_data, &ts_s, &auth, "0").await?;

    let err = first["error"].as_i64().unwrap_or(-1);
    if err != 0 || first["data"].is_null() {
        return Err(format!(
            "斗鱼接口返回 {err} {}",
            first["msg"].as_str().unwrap_or("")
        ));
    }

    // rate → 名称（含 rate=0 的原画，名字形如「原画2K60」/「原画1080P60」）。
    // 用斗鱼给的名字比自己写死「原画」更有信息量。
    let name_of: Vec<(i64, String)> = first["data"]["multirates"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| Some((r["rate"].as_i64()?, r["name"].as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let label_of = |rate: i64| -> String {
        name_of
            .iter()
            .find(|(r, _)| *r == rate)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| format!("{rate}M"))
    };

    // 首请求（rate=0）已拿到；再按 multirates 顺序请求其余档位。
    let mut responses: Vec<(i64, Value)> = vec![(0, first)];
    for (rate, _) in name_of.iter().filter(|(r, _)| *r != 0).take(5) {
        let rs = rate.to_string();
        if let Ok(v) = fetch_play(&c, room_id, enc_data, &ts_s, &auth, &rs).await {
            if v["error"].as_i64().unwrap_or(-1) == 0 && !v["data"].is_null() {
                responses.push((*rate, v));
            }
        }
    }

    // 按**实际**档位取名 + 去重：被降级时多个请求会落到同一档，
    // 不去重就会出现三个内容相同的条目。
    let mut out: Vec<PlayUrl> = Vec::new();
    let mut got: Vec<i64> = Vec::new();
    for (asked, v) in &responses {
        let d = &v["data"];
        // data.rate 是斗鱼**实际**给我们的档位，缺失时才退回请求值
        let actual = d["rate"].as_i64().unwrap_or(*asked);
        if got.contains(&actual) {
            continue;
        }
        if let Some(p) = build_play(d, &label_of(actual), room_id) {
            got.push(actual);
            out.push(p);
        }
    }

    if out.is_empty() {
        return Err("没有可用播放地址".into());
    }
    Ok(out)
}

/// 按指定 rate 请求一次播放地址
async fn fetch_play(
    c: &reqwest::Client,
    room_id: &str,
    enc_data: &str,
    ts_s: &str,
    auth: &str,
    rate: &str,
) -> Result<Value, String> {
    let v: Value = c
        .post(format!(
            "https://www.douyu.com/lapi/live/getH5PlayV1/{room_id}"
        ))
        .header("Referer", referer(room_id))
        .header("Origin", "https://www.douyu.com")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("enc_data", enc_data),
            ("tt", ts_s),
            ("did", DID),
            ("auth", auth),
            ("cdn", ""),
            ("rate", rate),
            ("hevc", "0"),
            ("fa", "0"),
            ("ive", "0"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())?;

    // 调试开关：JUNLIVE_DUMP_DOUYU=1 时打印每次请求的原始返回。
    // 排查画质问题要看三个字段：
    //   data.rtmp_live  —— 真实流地址后缀（_4000 = 4M、_8000 = 8M…）
    //   data.rate       —— 接口**实际**给我们的档位（请求 0 却回 4 = 被降级）
    //   data.multirates —— 该房间可用档位及各自 rate 值
    if std::env::var("JUNLIVE_DUMP_DOUYU").is_ok() {
        eprintln!(
            "[douyu raw req rate={rate}] {}",
            serde_json::to_string(&v).unwrap_or_default()
        );
    }
    Ok(v)
}

/// 从 getH5PlayV1 的 data 里拼出一条可播地址
fn build_play(d: &Value, name: &str, rid: &str) -> Option<PlayUrl> {
    let rtmp_url = d["rtmp_url"].as_str().unwrap_or("").trim_end_matches('/');
    let rtmp_live = d["rtmp_live"].as_str().unwrap_or("");
    if rtmp_live.is_empty() {
        return None;
    }
    let full = if rtmp_live.starts_with("http") {
        rtmp_live.to_string()
    } else {
        format!("{rtmp_url}/{rtmp_live}")
    };
    let format = if full.contains(".flv") { "flv" } else { "hls" };
    // 斗鱼用 wrap_flv（带断流重连），不用 wrap：
    // 斗鱼的 FLV 直链**固定 300 秒**就被 CDN 主动 EOF（服务端策略，不是网络问题），
    // 断了之后必须重连才有人接着播。重连时代理会用续流上下文重新解析新地址
    // （见 proxy.rs 的 resolve_fresh_url），播放器侧表现为卡一下然后继续。
    Some(PlayUrl {
        proxy: proxy::wrap_flv(&full, hdr(rid), false),
        url: full,
        format: format.into(),
        quality: name.to_string(),
        qualities: vec![],
    })
}

fn md5_hex(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

fn parse_http_date(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc2822(s)
        .ok()
        .map(|d| d.timestamp())
}

/// 斗鱼的 `av` / `avatar` 是**相对路径**（形如 `avatar_v3/202004/xxxx`），
/// 必须拼成 `https://apic.douyucdn.cn/upload/<av>_big.jpg`，否则前端拿到的是
/// 无效地址、只能显示首字母占位图。
fn av_url(v: &str) -> String {
    let v = v.trim();
    if v.is_empty() {
        return String::new();
    }
    if v.starts_with("http://") || v.starts_with("https://") || v.starts_with("//") {
        return v.to_string();
    }
    format!("https://apic.douyucdn.cn/upload/{v}_big.jpg")
}

fn fmt_num(n: i64) -> String {
    if n >= 10000 {
        format!("{:.1}万", n as f64 / 10000.0)
    } else {
        n.to_string()
    }
}
