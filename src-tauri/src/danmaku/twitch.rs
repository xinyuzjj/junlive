use super::*;

// ============================ Twitch ============================

pub(super) async fn run_twitch(app: Option<AppHandle>, channel: String, my: u64) -> Result<(), String> {
    let ch = channel.trim().to_lowercase();
    // Twitch 在墙外：WebSocket 用不了 reqwest 的代理设置，
    // 先走 HTTP CONNECT 开隧道 + TLS，再在这条流上跑 WS 协议。
    let stream = net::proxy_tunnel("irc-ws.chat.twitch.tv", 443).await?;
    let req = "wss://irc-ws.chat.twitch.tv:443"
        .into_client_request()
        .map_err(|e| e.to_string())?;
    let (ws, _) = tokio_tungstenite::client_async(req, stream)
        .await
        .map_err(|e| format!("连接 Twitch 聊天失败: {e}"))?;
    let (mut write, mut read) = ws.split();

    // 匿名登录
    let nick = format!("justinfan{}", 10000 + (now_ms() % 80000));
    for line in [
        "CAP REQ :twitch.tv/tags twitch.tv/commands".to_string(),
        "PASS SCHMOOPIIE".to_string(),
        format!("NICK {nick}"),
        format!("JOIN #{ch}"),
    ] {
        write
            .send(Message::Text(line))
            .await
            .map_err(|e| e.to_string())?;
    }

    loop {
        if session_id() != my {
            break;
        }
        let txt = match read.next().await {
            Some(Ok(Message::Text(t))) => t,
            Some(Ok(Message::Ping(p))) => {
                let _ = write.send(Message::Pong(p)).await;
                continue;
            }
            Some(Ok(_)) => continue,
            _ => break,
        };
        for line in txt.lines() {
            if line.starts_with("PING") {
                let _ = write.send(Message::Text("PONG :tmi.twitch.tv".into())).await;
                continue;
            }
            let Some(idx) = line.find(" PRIVMSG ") else {
                continue;
            };
            let head = &line[..idx];
            let rest = &line[idx + 9..];
            let Some(ci) = rest.find(" :") else { continue };
            let text = &rest[ci + 2..];

            let nick = head
                .rsplit(':')
                .next()
                .unwrap_or("")
                .split('!')
                .next()
                .unwrap_or("")
                .to_string();
            let color = line
                .split(';')
                .find_map(|p| p.strip_prefix("color="))
                .map(|c| c.trim().to_string())
                .filter(|c| c.starts_with('#'))
                .unwrap_or_else(|| "#ffffff".into());
            emit(&app, "twitch", &nick, text, &color);
        }
    }
    Ok(())
}
