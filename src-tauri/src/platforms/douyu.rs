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
