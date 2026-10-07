use super::*;
use crate::platforms::youtube::{extract_json, extract_video_id};

// ============================ YouTube ============================
//
// 注意：**播放**不能用自己解析的 HLS（分片强制 PO token，一律 403），
// 走官方 iframe。但**弹幕**可以自己拉 —— live_chat 接口没有 PO token 校验，
// 只要拿到 continuation 就能长轮询，所以这里实现真实弹幕（能飞屏）。
//
// 流程：
//   1. GET watch 页 → ytInitialData → conversationBar.liveChatRenderer
//      .continuations[0].reloadContinuationData.continuation
//   2. POST youtubei/v1/live_chat/get_live_chat {continuation}
//      → actions[] 是消息，响应里带新的 continuation，循环即可

/// 在字符串里取 `pre` 与 `post` 之间的内容（用于抠 API key / 版本号）
fn between(s: &str, pre: &str, post: &str) -> Option<String> {
    let i = s.find(pre)? + pre.len();
    let rest = &s[i..];
    let j = rest.find(post)?;
    Some(rest[..j].to_string())
}

/// 递归找任意一种 continuation（reload / invalidation / timed）
fn find_continuation(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) => {
            for k in [
                "reloadContinuationData",
                "invalidationContinuationData",
                "timedContinuationData",
            ] {
                if let Some(c) = m.get(k).and_then(|x| x.get("continuation")) {
                    if let Some(s) = c.as_str() {
                        if !s.is_empty() {
                            return Some(s.to_string());
                        }
                    }
                }
            }
            m.values().find_map(find_continuation)
        }
        Value::Array(a) => a.iter().find_map(find_continuation),
        _ => None,
    }
}

/// 轮询间隔（毫秒），YouTube 会在响应里给建议值
fn poll_ms(v: &Value) -> u64 {
    fn walk(v: &Value) -> Option<u64> {
        match v {
            Value::Object(m) => {
                if let Some(t) = m.get("timeoutMs").and_then(|x| x.as_u64()) {
                    return Some(t);
                }
                m.values().find_map(walk)
            }
            Value::Array(a) => a.iter().find_map(walk),
            _ => None,
        }
    }
    walk(v).unwrap_or(2000).clamp(1000, 10000)
}

/// 取一条消息的文字（普通文字 + 表情）
fn runs_text(r: &Value) -> String {
    let mut out = String::new();
    if let Some(runs) = r["message"]["runs"].as_array() {
        for run in runs {
            if let Some(t) = run["text"].as_str() {
                out.push_str(t);
            } else if let Some(emoji) = run.get("emoji") {
                // 表情：优先 shortcuts，其次无障碍标签
                let s = emoji["shortcuts"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|x| x.as_str())
                    .map(|x| x.to_string())
                    .or_else(|| {
                        emoji["image"]["accessibility"]
                            ["accessibilityData"]["label"]
                            .as_str()
                            .map(|x| x.to_string())
                    });
                if let Some(s) = s {
                    out.push_str(&s);
                }
            }
        }
    }
    out
}

/// YouTube 的颜色是 ARGB 整数，转成 "#rrggbb"
fn argb_to_hex(v: &Value) -> String {
    match v.as_u64() {
        Some(n) => format!("#{:06x}", n & 0x00ff_ffff),
        None => "#ffffff".into(),
    }
}

pub(super) async fn run_youtube(
    app: Option<AppHandle>,
    room_id: String,
    my: u64,
) -> Result<(), String> {
    let vid = extract_video_id(&room_id);
    if vid.is_empty() {
        return Err("YouTube 视频 ID 为空".into());
    }
    let c = net::via_proxy();
    let ck = net::yt_cookie_header();
    let watch = format!("https://www.youtube.com/watch?v={vid}&hl=zh-CN");

    let html = c
        .get(&watch)
        .header("Referer", "https://www.youtube.com/")
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .header("Cookie", ck.clone())
        .send()
        .await
        .map_err(|e| format!("YouTube 请求失败（请检查代理设置）：{e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    let data = extract_json(&html, "ytInitialData").ok_or("解析 ytInitialData 失败")?;
    let mut cont = find_continuation(&data).ok_or("该直播没有开启聊天室")?;

    let key = between(&html, "\"INNERTUBE_API_KEY\":\"", "\"")
        // ⚠️ 这里**不要**再内置任何 API key。
        // 曾经内置过一个 Google API Key 当兜底，被 GitHub secret scanning
        // 判定为泄露（告警 #1）—— 仓库公开，内置 key 等于把配额送人。
        // 正常情况下 INNERTUBE_API_KEY 都能从页面取到，走不到兜底；
        // 真要兜底就用环境变量 JUNLIVE_YT_API_KEY。
        .or_else(|| {
            std::env::var("JUNLIVE_YT_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .ok_or("没能从页面取到 YouTube 内部 API key（可用环境变量 JUNLIVE_YT_API_KEY 指定）")?;
    let ver = between(&html, "\"INNERTUBE_CLIENT_VERSION\":\"", "\"")
        .unwrap_or_else(|| "2.20240101.00.00".into());

    let api = format!(
        "https://www.youtube.com/youtubei/v1/live_chat/get_live_chat?key={key}&prettyPrint=false"
    );

    // 首次轮询会一次性返回几十条进房前的历史消息，全部飘出来会刷屏。
    // 用消息自带的 timestampUsec 过滤：早于进房时刻的一律不显示。
    let start_usec = now_ms() * 1000;

    loop {
        if session_id() != my {
            break;
        }

        let body = serde_json::json!({
            "context": { "client": {
                "clientName": "WEB",
                "clientVersion": ver,
                "hl": "zh-CN",
                "gl": "US"
            }},
            "continuation": cont
        });

        let resp = c
            .post(&api)
            .header("Content-Type", "application/json")
            .header("Origin", "https://www.youtube.com")
            .header("Referer", &watch)
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Cookie", ck.clone())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("live_chat 请求失败：{e}"))?;

        if !resp.status().is_success() {
            return Err(format!("live_chat 返回 {}", resp.status()));
        }
        let j: Value = resp.json().await.map_err(|e| e.to_string())?;

        let actions = j["continuationContents"]["liveChatContinuation"]["actions"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        for a in &actions {
            let item = &a["addChatItemAction"]["item"];
            for kind in [
                "liveChatTextMessageRenderer",
                "liveChatPaidMessageRenderer",
                "liveChatMembershipItemRenderer",
            ] {
                let r = &item[kind];
                if r.is_null() {
                    continue;
                }
                // 进房前的历史消息不显示
                let ts = r["timestampUsec"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .or_else(|| r["timestampUsec"].as_u64());
                if let Some(t) = ts {
                    if t < start_usec {
                        continue;
                    }
                }
                let user = r["authorName"]["simpleText"].as_str().unwrap_or("");
                let text = runs_text(r);
                // 普通消息用 authorNameTextColor；SuperChat 用 bodyBackgroundColor
                let color = if kind == "liveChatPaidMessageRenderer" {
                    "#ffd54f".to_string()
                } else {
                    let h = argb_to_hex(&r["authorNameTextColor"]);
                    if h == "#000000" {
                        "#ffffff".to_string()
                    } else {
                        h
                    }
                };
                emit(&app, "youtube", user, &text, &color);
            }
        }

        let wait = poll_ms(&j);
        if let Some(nc) = find_continuation(&j) {
            cont = nc;
        } else {
            // 没给新 continuation（直播结束等）→ 稍等再试
        }

        let mut left = wait;
        while left > 0 && session_id() == my {
            let step = left.min(500);
            tokio::time::sleep(Duration::from_millis(step)).await;
            left -= step;
        }
    }
    Ok(())
}
