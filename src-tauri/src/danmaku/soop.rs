use super::*;

// ============================ SOOP ============================
//
// SOOP 弹幕是 WebSocket + 自己的一套二进制分帧协议（不是 JSON、也不是标准文本协议）：
//
//   连接  wss://{CHDOMAIN}:{CHPT+1}/Websocket/{roomId}
//         **必须带子协议 `Sec-WebSocket-Protocol: chat`** —— 不带的话服务器
//         直接断连接（表现是 TLS 握手后立刻 EOF，很容易误判成「被墙」或「要登录」）。
//         CHPT 是明文端口，TLS 要 +1（9000 -> 9001）；直接连 CHPT 会卡在 TLS 握手。
//         另外要带 `Origin: https://play.sooplive.co.kr`。
//
//   帧格式  \x1b\x09 + service(4位) + bodyLen(6位) + "00"  = 14 字节头
//           body 里的字段用 \x0c 分隔
//
//   要发的三个包：
//     连接  \x1b\x09 0001 000006 00 \x0c\x0c\x0c16\x0c
//     进房  \x1b\x09 0002 <len:6> 00 \x0c <CHATNO> \x0c\x0c\x0c\x0c\x0c
//           其中 len = CHATNO 的 UTF-8 字节数 + 6
//     心跳  \x1b\x09 0000 000001 00 \x0c
//
//   收包：service == 5 是聊天弹幕，body 按 \x0c 切，fields[1] = 内容、fields[6] = 昵称。
//         fields[1] 是 "-1"/"1" 或含 "|" 的是系统/协议消息，要丢掉。
//
// 协议来自 pure_live_TV 的 soop_danmaku.dart（Dart），此处按同样规则用 Rust 重写。

/// 转义前缀：ESC + TAB
const PFX: &[u8] = b"\x1b\t";
/// 字段分隔符
const SEP: u8 = 0x0c;

/// 按协议拼一个包：14 字节头 + body
fn packet(service: &str, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(14 + body.len());
    v.extend_from_slice(PFX);
    v.extend_from_slice(format!("{service:0>4}").as_bytes());
    v.extend_from_slice(format!("{:0>6}", body.len()).as_bytes());
    v.extend_from_slice(b"00");
    v.extend_from_slice(body);
    v
}

/// 连接包
fn connect_packet() -> Vec<u8> {
    packet("0001", b"\x0c\x0c\x0c16\x0c")
}

/// 进房包：body = \x0c + CHATNO + \x0c*5，声明长度 = CHATNO 字节数 + 6
fn join_packet(chat_no: &str) -> Vec<u8> {
    let mut body = vec![SEP];
    body.extend_from_slice(chat_no.as_bytes());
    body.extend_from_slice(&[SEP; 5]);
    packet("0002", &body)
}

/// 心跳包
fn ping_packet() -> Vec<u8> {
    packet("0000", &[SEP])
}

/// 从播放接口拿聊天室地址：CHATNO / CHDOMAIN（或 CHIP）/ CHPT
async fn chat_endpoint(bjid: &str) -> Result<(String, String), String> {
    let form = [
        ("bid", bjid),
        ("bno", ""),
        ("from_api", "0"),
        ("mode", "landing"),
        ("player_type", "html5"),
        ("pwd", ""),
        ("quality", ""),
        ("stream_type", "common"),
        ("type", "live"),
    ];
    let v: Value = net::via_proxy()
        .post("https://live.sooplive.com/afreeca/player_live_api.php")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .header("Referer", format!("https://play.sooplive.com/{bjid}"))
        .header("Origin", "https://play.sooplive.co.kr")
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("SOOP 聊天室信息请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("SOOP 聊天室信息解析失败: {e}"))?;

    let ch = v.get("CHANNEL").cloned().unwrap_or(Value::Null);
    let chat_no = ch
        .get("CHATNO")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let host = ch
        .get("CHDOMAIN")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    // CHPT 是明文端口，TLS 用 +1；拿不到就按官方常见值 9000
    let plain: u16 = ch
        .get("CHPT")
        .and_then(|x| x.as_u64())
        .map(|n| n as u16)
        .filter(|n| *n > 0 && *n < 65535)
        .unwrap_or(9000);

    if chat_no.is_empty() || host.is_empty() {
        return Err("SOOP 聊天室信息不完整（CHATNO / CHDOMAIN 缺失）".into());
    }
    Ok((chat_no, format!("wss://{host}:{}/Websocket/{bjid}", plain + 1)))
}

pub(super) async fn run_soop(app: Option<AppHandle>, rid: String, my: u64) -> Result<(), String> {
    let (chat_no, url) = chat_endpoint(&rid).await?;

    let mut req = url
        .into_client_request()
        .map_err(|e| format!("SOOP 弹幕地址无效: {e}"))?;
    // 关键：不带这个子协议，服务器握手后直接断
    req.headers_mut()
        .insert("Sec-WebSocket-Protocol", "chat".parse().unwrap());
    req.headers_mut()
        .insert("Origin", "https://play.sooplive.co.kr".parse().unwrap());

    let (ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| format!("连接 SOOP 弹幕失败: {e}"))?;
    let (mut write, mut read) = ws.split();

    write
        .send(Message::Binary(connect_packet()))
        .await
        .map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    write
        .send(Message::Binary(join_packet(&chat_no)))
        .await
        .map_err(|e| e.to_string())?;

    let mut hb = interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = hb.tick() => {
                if write.send(Message::Binary(ping_packet())).await.is_err() { break; }
            }
            m = read.next() => {
                if session_id() != my { break; }
                let data = match m {
                    Some(Ok(Message::Binary(b))) => b,
                    Some(Ok(_)) => continue,
                    _ => break,
                };
                for (user, text) in parse_frames(&data) {
                    emit(&app, "soop", &user, &text, "#ffffff");
                }
            }
        }
    }
    Ok(())
}

/// 解析一个可能含多个包的二进制帧，返回其中的聊天弹幕
fn parse_frames(data: &[u8]) -> Vec<(String, String)> {
    const HEADER: usize = 14;
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + HEADER <= data.len() {
        // 每个包必须以 ESC+TAB 开头，对不上说明流错位，直接放弃这一帧
        if data[off] != 0x1b || data[off + 1] != 0x09 {
            break;
        }
        let service = std::str::from_utf8(&data[off + 2..off + 6])
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        let body_len = std::str::from_utf8(&data[off + 6..off + 12])
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok());
        let (service, body_len) = match (service, body_len) {
            (Some(s), Some(l)) => (s, l),
            _ => break,
        };
        let end = off + HEADER + body_len;
        if end > data.len() {
            break;
        }
        // service 5 = 聊天
        if service == 5 {
            let body = &data[off + HEADER..end];
            let fields: Vec<String> = body
                .split(|b| *b == SEP)
                .map(|p| String::from_utf8_lossy(p).trim().to_string())
                .collect();
            if fields.len() > 6 {
                let text = fields[1].clone();
                let user = fields[6].clone();
                // 系统/协议消息：内容为 -1 / 1，或带 "|"（管理指令）
                if !text.is_empty()
                    && !user.is_empty()
                    && text != "-1"
                    && text != "1"
                    && !text.contains('|')
                {
                    out.push((user, text));
                }
            }
        }
        off = end;
    }
    out
}
