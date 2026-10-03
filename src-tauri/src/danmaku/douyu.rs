use super::*;

// ============================ 斗鱼 ============================

/// 斗鱼弹幕包：小端，包长重复两次 + magic 689 + 消息 + 结尾 0
fn dy_pack(msg: &str) -> Vec<u8> {
    let b = msg.as_bytes();
    let len = b.len() + 9;
    let mut v = Vec::with_capacity(len);
    v.extend_from_slice(&(len as u32).to_le_bytes());
    v.extend_from_slice(&(len as u32).to_le_bytes());
    v.extend_from_slice(&689u16.to_le_bytes());
    v.push(0);
    v.push(0);
    v.extend_from_slice(b);
    v.push(0);
    v
}

pub(super) async fn run_douyu(app: Option<AppHandle>, rid: String, my: u64) -> Result<(), String> {
    let req = "wss://danmuproxy.douyu.com:8506/"
        .into_client_request()
        .map_err(|e| e.to_string())?;
    // 注意：这里**不能**设 Sec-WebSocket-Protocol —— 斗鱼服务器不回子协议，
    // tungstenite 会直接报 "Server sent no subprotocol" 并断开。
    let (ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| format!("连接斗鱼弹幕失败: {e}"))?;
    let (mut write, mut read) = ws.split();

    write
        .send(Message::Binary(dy_pack(&format!(
            "type@=loginreq/roomid@={rid}/"
        ))))
        .await
        .map_err(|e| e.to_string())?;
    write
        .send(Message::Binary(dy_pack(&format!(
            "type@=joingroup/rid@={rid}/gid@=1/"
        ))))
        .await
        .map_err(|e| e.to_string())?;

    let mut hb = interval(Duration::from_secs(40));
    loop {
        tokio::select! {
            _ = hb.tick() => {
                if write.send(Message::Binary(dy_pack("type@=mrkl/"))).await.is_err() { break; }
            }
            m = read.next() => {
                if session_id() != my { break; }
                let data = match m {
                    Some(Ok(Message::Binary(b))) => b,
                    Some(Ok(_)) => continue,
                    _ => break,
                };
                if data.len() < 13 { continue; }
                // 12 字节头 + 结尾 1 字节
                let content = String::from_utf8_lossy(&data[12..data.len() - 1]);
                let mut map: HashMap<String, String> = HashMap::new();
                for item in content.split('/') {
                    if item.is_empty() { continue; }
                    if let Some((k, v)) = item.split_once("@=") {
                        map.insert(k.to_string(), v.replace("@S", "/").replace("@A", "@"));
                    }
                }
                if map.get("type").map(|t| t == "chatmsg").unwrap_or(false) {
                    let user = map.get("nn").cloned().unwrap_or_default();
                    let text = map.get("txt").cloned().unwrap_or_default();
                    let color = map
                        .get("col")
                        .and_then(|c| c.parse::<i64>().ok())
                        .filter(|n| *n > 0)
                        .map(|n| format!("#{:06x}", n & 0xffffff))
                        .unwrap_or_else(|| "#ffffff".into());
                    emit(&app, "douyu", &user, &text, &color);
                }
            }
        }
    }
    Ok(())
}

