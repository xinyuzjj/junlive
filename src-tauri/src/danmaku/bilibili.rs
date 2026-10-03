use super::*;

// ============================ 哔哩哔哩 ============================

pub(super) async fn run_bilibili(app: Option<AppHandle>, rid: u64, my: u64) -> Result<(), String> {
    let c = net::direct();
    let cookie = net::bili_cookie_header();

    // 1) 拿 WBI 密钥
    let (img_key, sub_key) = net::wbi_keys(&c).await?;

    // 2) 签名后请求 getDanmuInfo
    let query = net::wbi_sign(
        vec![
            ("id", rid.to_string()),
            ("type", "0".to_string()),
            ("web_location", "444.8".to_string()),
        ],
        &img_key,
        &sub_key,
    );
    let mut req = c
        .get(format!(
            "https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?{query}"
        ))
        .header("User-Agent", net::BILI_UA)
        .header("Referer", "https://live.bilibili.com/")
        .header("Origin", "https://live.bilibili.com");
    if !cookie.is_empty() {
        req = req.header("Cookie", cookie);
    }
    let info: Value = req
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let token = info["data"]["token"].as_str().unwrap_or("").to_string();
    if token.is_empty() {
        let code = info["code"].as_i64().unwrap_or(0);
        return Err(format!(
            "拿不到弹幕 token（code={code}，wbi 签名可能已过期）"
        ));
    }
    let (host, port) = info["data"]["host_list"]
        .as_array()
        .and_then(|a| a.first())
        .map(|h| {
            (
                h["host"]
                    .as_str()
                    .unwrap_or("broadcastlv.chat.bilibili.com")
                    .to_string(),
                h["wss_port"].as_u64().unwrap_or(443),
            )
        })
        .unwrap_or(("broadcastlv.chat.bilibili.com".to_string(), 443));

    let (ws, _) = tokio_tungstenite::connect_async(format!("wss://{host}:{port}/sub"))
        .await
        .map_err(|e| format!("连接弹幕服务器 {host}:{port} 失败: {e}"))?;
    let (mut write, mut read) = ws.split();

    let auth = serde_json::json!({
        "uid": 0, "roomid": rid, "protover": 3,
        "platform": "web", "type": 2, "key": token
    })
    .to_string();
    write
        .send(Message::Binary(pack(7, 1, auth.as_bytes())))
        .await
        .map_err(|e| e.to_string())?;

    let mut hb = interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = hb.tick() => {
                if write.send(Message::Binary(pack(2, 1, b""))).await.is_err() { break; }
            }
            m = read.next() => {
                if session_id() != my { break; }
                let data = match m {
                    Some(Ok(Message::Binary(b))) => b,
                    Some(Ok(_)) => continue,
                    _ => break,
                };
                for body in decode_packets(&data) {
                    handle_bili_body(&app, &body);
                }
            }
        }
    }
    Ok(())
}

fn pack(op: u32, proto: u16, body: &[u8]) -> Vec<u8> {
    let len = 16 + body.len();
    let mut v = Vec::with_capacity(len);
    v.extend_from_slice(&(len as u32).to_be_bytes());
    v.extend_from_slice(&16u16.to_be_bytes());
    v.extend_from_slice(&proto.to_be_bytes());
    v.extend_from_slice(&op.to_be_bytes());
    v.extend_from_slice(&1u32.to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn decode_packets(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 16 <= data.len() {
        let plen = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let hlen = u16::from_be_bytes([data[i + 4], data[i + 5]]) as usize;
        let proto = u16::from_be_bytes([data[i + 6], data[i + 7]]);
        if plen < hlen || i + plen > data.len() {
            break;
        }
        let body = &data[i + hlen..i + plen];
        if proto == 3 {
            if let Ok(raw) = brotli_decode(body) {
                out.extend(decode_packets(&raw));
            }
        } else if proto == 1 || proto == 0 {
            out.push(body.to_vec());
        }
        i += plen;
    }
    out
}

fn brotli_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut d = brotli::Decompressor::new(data, 4096);
    d.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn handle_bili_body(app: &Option<AppHandle>, body: &[u8]) {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    let cmd = v["cmd"].as_str().unwrap_or("");
    if !cmd.starts_with("DANMU_MSG") {
        return;
    }
    let info = match v["info"].as_array() {
        Some(a) => a,
        None => return,
    };
    let text = info.get(1).and_then(|x| x.as_str()).unwrap_or("");
    let user = info
        .get(2)
        .and_then(|x| x.as_array())
        .and_then(|a| a.get(1))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let color = info
        .first()
        .and_then(|x| x.as_array())
        .and_then(|a| a.get(3))
        .and_then(|x| x.as_i64())
        .map(|n| format!("#{:06x}", n & 0xffffff))
        .unwrap_or_else(|| "#ffffff".into());
    emit(&app, "bilibili", user, text, &color);
}

