//! SOOP（原 AfreecaTV / 非洲台）
//!
//! 接口全部是 2026-10 实测出来的（老的 `api.sooplive.co.kr` 已 DNS 失效，
//! 别再照抄旧项目）。播放流程参考 streamlink 的 afreecatv 插件：
//!
//! 1. `player_live_api.php` type=live → CHANNEL{BNO, RMD, BJNICK, TITLE, VIEWPRESET}
//! 2. 每个清晰度再来一次 type=aid&quality=<name> → CHANNEL.AID
//! 3. `{RMD}/broad_stream_assign.html?return_type=gs_cdn_pc_web&broad_key={BNO}-common-{q}-hls`
//!    → `view_url`
//! 4. 播放 `{view_url}?aid={AID}`
//!
//! 注意 CHANNEL 里还有个现成的 `TS` 字段（HLS 地址），但它直拉 **403**，
//! 必须走上面 2~4 步拿带 aid 的地址。

use crate::model::*;
use crate::net;
use crate::proxy;
use serde_json::Value;

/// 频道信息 / 播放鉴权（type=live / type=aid）
const CHANNEL_API: &str = "https://live.sooplive.com/afreeca/player_live_api.php";
/// 直播列表
const LIST_API: &str = "https://live.sooplive.com/api/main_broad_list_api.php";
/// 搜索 / 分类
const SCH_API: &str = "https://sch.sooplive.com/api.php";
/// 主播资料（免密，拿头像）
const STATION_API: &str = "https://chapi.sooplive.co.kr/api";

fn play_referer(bjid: &str) -> String {
    format!("https://play.sooplive.co.kr/{bjid}")
}

fn hdr(bjid: &str) -> Vec<(String, String)> {
    vec![
        ("Referer".into(), play_referer(bjid)),
        ("Origin".into(), "https://play.sooplive.co.kr".into()),
        ("User-Agent".into(), net::UA.into()),
    ]
}

fn clean(bjid: &str) -> String {
    bjid.trim()
        .trim_start_matches('@')
        .rsplit('/')
        .next()
        .unwrap_or(bjid)
        .to_string()
}

/// 接口里很多图片是 `//host/path` 协议相对地址，补上 https:
fn fix_url(u: &str) -> String {
    let u = u.trim();
    if u.is_empty() {
        return String::new();
    }
    if u.starts_with("//") {
        format!("https:{u}")
    } else {
        u.to_string()
    }
}

/// 调频道接口。`kind` = "live"（拿房间信息）或 "aid"（拿播放凭证）
async fn channel_call(bjid: &str, bno: &str, quality: &str, kind: &str) -> Result<Value, String> {
    let c = net::via_proxy();
    let v: Value = c
        .post(CHANNEL_API)
        .header("Referer", play_referer(bjid))
        .header("Origin", "https://play.sooplive.co.kr")
        .form(&[
            ("bid", bjid),
            ("bno", bno),
            ("from_api", "0"),
            ("mode", "landing"),
            ("player_type", "html5"),
            ("pwd", ""),
            ("quality", quality),
            ("stream_type", "common"),
            ("type", kind),
        ])
        .send()
        .await
        .map_err(|e| format!("SOOP 请求失败（请检查代理设置）：{e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(v)
}

pub async fn categories() -> Result<Vec<Category>, String> {
    let c = net::via_proxy();
    let url = format!(
        "{SCH_API}?m=categoryList&szKeyword=&szOrder=view_cnt&nPageNo=1\
         &nListCnt=120&nOffset=0&szPlatform=pc"
    );
    let v: Value = c
        .get(&url)
        .header("Referer", "https://www.sooplive.com/directory/category")
        .send()
        .await
        .map_err(|e| format!("SOOP 请求失败（请检查代理设置）：{e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut out = vec![Category {
        id: "all".into(),
        name: "全部".into(),
        children: vec![],
    }];
    for it in v["data"]["list"].as_array().cloned().unwrap_or_default() {
        let no = it["category_no"].as_str().unwrap_or("");
        let name = it["category_name"].as_str().unwrap_or("");
        if no.is_empty() || name.is_empty() {
            continue;
        }
        out.push(Category {
            id: no.to_string(),
            name: name.to_string(),
            children: vec![],
        });
    }
    Ok(out)
}

/// 直播列表。category 传分类号（如 `00040017`）或 `all`
pub async fn rooms(category: &str, page: u32) -> Result<RoomList, String> {
    let c = net::via_proxy();
    let cat = category.trim();
    // selectType=action 是「全部」，selectType=cate 是具体分区
    let (sel_type, sel_val) = if cat.is_empty() || cat == "all" {
        ("action", "all")
    } else {
        ("cate", cat)
    };
    let url = format!(
        "{LIST_API}?selectType={sel_type}&selectValue={sel_val}&orderType=view_cnt\
         &pageNo={page}&strmLangType=&lang=ko_KR"
    );
    let v: Value = c
        .get(&url)
        .header("Referer", "https://www.sooplive.com/live/all")
        .send()
        .await
        .map_err(|e| format!("SOOP 请求失败（请检查代理设置）：{e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    for it in v["broad"].as_array().cloned().unwrap_or_default() {
        let bjid = it["user_id"].as_str().unwrap_or("");
        if bjid.is_empty() {
            continue;
        }
        list.push(Room {
            platform: "soop".into(),
            room_id: bjid.to_string(),
            title: it["broad_title"].as_str().unwrap_or("").to_string(),
            streamer: it["user_nick"].as_str().unwrap_or(bjid).to_string(),
            cover: fix_url(it["broad_thumb"].as_str().unwrap_or("")),
            area: it["category_name"].as_str().unwrap_or("").to_string(),
            online: fmt_cnt(it["current_view_cnt"].as_str().unwrap_or("")),
            live: true,
            // 列表接口不带头像，进房间时再用 station 接口补
            avatar: String::new(),
            replay: false,
        });
    }
    let total: u64 = v["total_cnt"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(RoomList {
        has_more: (page as u64) * 60 < total,
        rooms: list,
        page,
    })
}

/// 搜索主播（`m=bjSearch`）。返回的是主播，不一定在播。
pub async fn search(keyword: &str, page: u32) -> Result<RoomList, String> {
    let c = net::via_proxy();
    let kw = urlencoding::encode(keyword);
    let url = format!("{SCH_API}?m=bjSearch&keyword={kw}&page={page}");
    let v: Value = c
        .get(&url)
        .header("Referer", "https://www.sooplive.com/search")
        .send()
        .await
        .map_err(|e| format!("SOOP 请求失败（请检查代理设置）：{e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    for it in v["DATA"].as_array().cloned().unwrap_or_default() {
        let bjid = it["user_id"].as_str().unwrap_or("");
        if bjid.is_empty() {
            continue;
        }
        let logo = fix_url(it["station_logo"].as_str().unwrap_or(""));
        list.push(Room {
            platform: "soop".into(),
            room_id: bjid.to_string(),
            title: it["station_title"].as_str().unwrap_or("").to_string(),
            streamer: it["user_nick"]
                .as_str()
                .or_else(|| it["station_name"].as_str())
                .unwrap_or(bjid)
                .to_string(),
            cover: logo.clone(),
            area: String::new(),
            online: fmt_cnt(it["favorite_cnt"].as_str().unwrap_or("")),
            live: true,
            avatar: logo,
            replay: false,
        });
    }
    Ok(RoomList {
        rooms: list,
        page,
        has_more: false,
    })
}

/// 主播头像（`chapi/api/{bj}/station`，免密）
async fn station_avatar(bjid: &str) -> String {
    let c = net::via_proxy();
    let url = format!("{STATION_API}/{bjid}/station");
    match c
        .get(&url)
        .header("Referer", "https://www.sooplive.com/")
        .send()
        .await
    {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => fix_url(v["profile_image"].as_str().unwrap_or("")),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    }
}

pub async fn room_detail(bjid: &str) -> Result<RoomDetail, String> {
    let bjid = clean(bjid);
    if bjid.is_empty() {
        return Err("请填写 SOOP 主播 ID".into());
    }
    let v = channel_call(&bjid, "", "", "live").await?;
    let ch = &v["CHANNEL"];
    if ch.is_null() {
        return Err(format!("频道 {bjid} 不存在"));
    }
    // RESULT=1 表示在播；否则是未开播/不存在
    let live = ch["RESULT"].as_i64().unwrap_or(0) == 1;
    let bno = ch["BNO"].as_str().unwrap_or("");

    let avatar = station_avatar(&bjid).await;
    let room = Room {
        platform: "soop".into(),
        room_id: bjid.clone(),
        title: ch["TITLE"].as_str().unwrap_or("").to_string(),
        streamer: ch["BJNICK"].as_str().unwrap_or(&bjid).to_string(),
        cover: if bno.is_empty() {
            String::new()
        } else {
            // 直播封面就是 liveimg 的缩略图
            format!("https://liveimg.sooplive.com/m/{bno}")
        },
        area: ch["CATEGORY_TAGS"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        online: fmt_cnt(ch["CTUSER"].as_str().unwrap_or("")),
        live,
        avatar,
        replay: false,
    };

    let plays = if live {
        build_plays(ch, &bjid, bno).await
    } else {
        Vec::new()
    };

    Ok(RoomDetail {
        room,
        description: String::new(),
        plays,
    })
}

/// 按 VIEWPRESET 逐档取「带 aid 的 HLS 地址」
async fn build_plays(ch: &Value, bjid: &str, bno: &str) -> Vec<PlayUrl> {
    let rmd = ch["RMD"].as_str().unwrap_or("");
    let presets = ch["VIEWPRESET"].as_array().cloned().unwrap_or_default();
    if rmd.is_empty() || bno.is_empty() || presets.is_empty() {
        return Vec::new();
    }

    let headers = hdr(bjid);
    let c = net::via_proxy();
    let mut out = Vec::new();

    for p in &presets {
        let q = p["name"].as_str().unwrap_or("");
        let label = p["label"].as_str().unwrap_or(q);
        // "auto" 是自适应，没有独立码流，跳过
        if q.is_empty() || q == "auto" {
            continue;
        }

        // 2) 拿该清晰度的 AID
        let aid = match channel_call(bjid, bno, q, "aid").await {
            Ok(v) => v["CHANNEL"]["AID"].as_str().unwrap_or("").to_string(),
            Err(_) => String::new(),
        };
        if aid.is_empty() {
            continue;
        }

        // 3) 拿 view_url
        let assign = format!(
            "{rmd}/broad_stream_assign.html?return_type=gs_cdn_pc_web\
             &broad_key={bno}-common-{q}-hls"
        );
        let vu: String = match c
            .get(&assign)
            .header("Referer", play_referer(bjid))
            .header("User-Agent", net::UA)
            .send()
            .await
        {
            Ok(r) => match r.json::<Value>().await {
                Ok(v) => v["view_url"].as_str().unwrap_or("").to_string(),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        };
        if vu.is_empty() {
            continue;
        }

        // 4) 播放地址必须带 aid，否则 CDN 直接 403
        let sep = if vu.contains('?') { '&' } else { '?' };
        let url = format!("{vu}{sep}aid={aid}");
        out.push(PlayUrl {
            proxy: proxy::wrap(&url, headers.clone(), true),
            url,
            format: "hls".into(),
            quality: label.to_string(),
            qualities: vec![],
        });
    }

    // 高清晰度排前面（前端默认播 plays[0]）
    out.reverse();
    out
}

fn fmt_cnt(s: &str) -> String {
    match s.parse::<i64>() {
        Ok(n) if n >= 10000 => format!("{:.1}万", n as f64 / 10000.0),
        Ok(n) => n.to_string(),
        Err(_) => s.to_string(),
    }
}
