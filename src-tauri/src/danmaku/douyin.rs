//! 抖音弹幕。
//!
//! 通道：`wss://<host>/webcast/im/push/v2/`，帧是 **gzip 压缩的 protobuf**。
//!
//! 协议结构（见 douyin.proto）：
//!   PushFrame { seqId=1, logId=2, service=3, method=4, headersList=5,
//!               payloadEncoding=6, payloadType=7(string), payload=8(bytes) }
//!   Response  { messagesList=1(repeated Message), cursor=2, fetchInterval=3,
//!               now=4, internalExt=5, ..., needAck=9 }
//!   Message   { method=1(string), payload=2(bytes), msgId=3, msgType=4 }
//!   ChatMessage { common=1, user=2, content=3 }
//!   User        { id=1, shortId=2, nickName=3 }
//!
//! 心跳：`PushFrame{ payloadType: "hb" }` 编码后当 **WebSocket Ping** 帧发，每 5 秒一次。
//!
//! 签名：URL 要带 `&signature=`（X-Bogus）。服务端**不认** `a_bogus`（实测 5 个
//! host 全返回 HTTP 200 而非 101），必须用 `sign.js` 算，见 `danmaku/sign.rs`。

use super::*;
use flate2::read::GzDecoder;
use std::io::Read;

const DY_HOSTS: &[&str] = &[
    "webcast3-ws-web-lq.douyin.com",
    "webcast5-ws-web-lf.douyin.com",
    "webcast5-ws-web-hl.douyin.com",
    "webcast3-ws-web-hl.douyin.com",
    "webcast3-ws-web-lf.douyin.com",
];

// ============================ 极简 protobuf ============================

struct Pb<'a> {
    b: &'a [u8],
    p: usize,
}

enum PbVal<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

impl<'a> Pb<'a> {
    fn new(b: &'a [u8]) -> Self {
        Pb { b, p: 0 }
    }

    fn varint(&mut self) -> Option<u64> {
        let mut r = 0u64;
        let mut s = 0u32;
        loop {
            let x = *self.b.get(self.p)?;
            self.p += 1;
            r |= ((x & 0x7f) as u64) << s;
            if x & 0x80 == 0 {
                return Some(r);
            }
            s += 7;
            if s > 63 {
                return None;
            }
        }
    }

    fn next(&mut self) -> Option<(u32, PbVal<'a>)> {
        let buf = self.b;
        let key = self.varint()?;
        let f = (key >> 3) as u32;
        match (key & 7) as u8 {
            0 => Some((f, PbVal::Int(self.varint()?))),
            1 => {
                let s = self.p;
                if s + 8 > buf.len() {
                    return None;
                }
                self.p += 8;
                Some((f, PbVal::Bytes(&buf[s..s + 8])))
            }
            2 => {
                let n = self.varint()? as usize;
                let s = self.p;
                if s + n > buf.len() {
                    return None;
                }
                self.p += n;
                Some((f, PbVal::Bytes(&buf[s..s + n])))
            }
            5 => {
                let s = self.p;
                if s + 4 > buf.len() {
                    return None;
                }
                self.p += 4;
                Some((f, PbVal::Bytes(&buf[s..s + 4])))
            }
            _ => None,
        }
    }
}

/// 心跳：PushFrame{ payloadType(7) = "hb" }
fn dy_heartbeat() -> Vec<u8> {
    let mut v = Vec::new();
    v.push((7 << 3) | 2); // field 7, wire type 2
    v.push(2);
    v.extend_from_slice(b"hb");
    v
}

fn gunzip(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 3 || b[0] != 0x1f || b[1] != 0x8b {
        return None;
    }
    let mut d = GzDecoder::new(b);
    let mut out = Vec::new();
    d.read_to_end(&mut out).ok()?;
    Some(out)
}

fn s_of(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_string()
}

/// 从 Response 字节里抽出所有聊天弹幕 (昵称, 内容)
fn dy_parse_chat(resp: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut r = Pb::new(resp);
    while let Some((f, v)) = r.next() {
        if f != 1 {
            continue;
        }
        let m = match v {
            PbVal::Bytes(b) => b,
            _ => continue,
        };
        let mut method = String::new();
        let mut payload: &[u8] = &[];
        let mut mm = Pb::new(m);
        while let Some((f2, v2)) = mm.next() {
            match (f2, v2) {
                (1, PbVal::Bytes(s)) => method = s_of(s),
                (2, PbVal::Bytes(p)) => payload = p,
                _ => {}
            }
        }
        if !method.contains("ChatMessage") || payload.is_empty() {
            continue;
        }
        let mut nick = String::new();
        let mut text = String::new();
        let mut cm = Pb::new(payload);
        while let Some((f3, v3)) = cm.next() {
            match (f3, v3) {
                (2, PbVal::Bytes(u)) => {
                    let mut um = Pb::new(u);
                    while let Some((f4, v4)) = um.next() {
                        if f4 == 3 {
                            if let PbVal::Bytes(n) = v4 {
                                nick = s_of(n);
                            }
                        }
                    }
                }
                (3, PbVal::Bytes(t)) => text = s_of(t),
                _ => {}
            }
        }
        if !text.trim().is_empty() {
            out.push((nick, text));
        }
    }
    out
}

/// 从 PushFrame 里取 payload(8) 与 payloadType(7)
fn dy_unwrap(frame: &[u8]) -> (Vec<u8>, String) {
    let mut r = Pb::new(frame);
    let mut payload: Option<&[u8]> = None;
    let mut ptype = String::new();
    let mut any = false;
    while let Some((f, v)) = r.next() {
        any = true;
        match (f, v) {
            (7, PbVal::Bytes(s)) => ptype = s_of(s),
            (8, PbVal::Bytes(p)) => payload = Some(p),
            _ => {}
        }
    }
    if !any {
        return (frame.to_vec(), ptype);
    }
    (payload.map(|p| p.to_vec()).unwrap_or_default(), ptype)
}

fn dy_pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ============================ 主流程 ============================

pub(super) async fn run_douyin(app: Option<AppHandle>, rid: String, my: u64) -> Result<(), String> {
    let c = net::direct();
    // 预热：拿 ttwid。**WS 握手必须带这个 Cookie**，否则服务端返回 200 而不是 101。
    let mut ttwid = String::new();
    if let Ok(r) = c
        .get("https://live.douyin.com/")
        .header("User-Agent", net::UA)
        .send()
        .await
    {
        for v in r.headers().get_all("set-cookie").iter() {
            if let Ok(s) = v.to_str() {
                if let Some(rest) = s.strip_prefix("ttwid=") {
                    ttwid = rest.split(';').next().unwrap_or("").to_string();
                }
            }
        }
    }
    if ttwid.is_empty() {
        println!("[douyin] 没拿到 ttwid，WS 握手大概率会被拒");
    }

    let enter = format!(
        "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=douyin_web&live_id=1\
&device_platform=web&language=zh-CN&enter_from=web_live&cookie_enabled=true\
&screen_width=1920&screen_height=1080&browser_language=zh-CN&browser_platform=Win32\
&browser_name=Chrome&browser_version=131.0.0.0&web_rid={rid}"
    );
    let v: Value = c
        .get(&enter)
        .header("Referer", format!("https://live.douyin.com/{rid}"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| format!("抖音房间解析失败: {e}"))?;

    let room_id = v["data"]["data"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|r| r["id_str"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("抖音房间 {rid} 没拿到 room_id（可能未开播）"))?;
    let uid = v["data"]["user"]["id_str"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "1028753014402254".to_string());

    println!("[douyin] rid={rid} room_id={room_id} uid={uid}");

    let hb = dy_heartbeat();

    loop {
        if session_id() != my {
            break;
        }

        let ms = now_ms();
        let cursor = format!("d-1_u-1_fh-7392091211001140287_t-{ms}_r-1");
        let internal_ext = format!(
            "internal_src:dim|wss_push_room_id:{room_id}|wss_push_did:{uid}\
|first_req_ms:{}|fetch_time:{ms}|seq:1|wss_info:0-{ms}-0-0|wrds_v:7392094459690748497",
            ms.saturating_sub(100)
        );
        let query = format!(
            "app_name=douyin_web&version_code=180800&webcast_sdk_version=1.0.14-beta.0\
&update_version_code=1.0.14-beta.0&compress=gzip&device_platform=web&cookie_enabled=true\
&screen_width=1536&screen_height=864&browser_language=zh-CN&browser_platform=Win32\
&browser_name=Mozilla&browser_version=5.0%20(Windows%20NT%2010.0;%20Win64;%20x64)%20AppleWebKit/537.36%20(KHTML,%20like%20Gecko)%20Chrome/131.0.0.0%20Safari/537.36\
&browser_online=true&tz_name=Asia/Shanghai&cursor={}&internal_ext={}\
&host=https://live.douyin.com&aid=6383&live_id=1&did_rule=3&endpoint=live_pc&support_wrds=1\
&user_unique_id={uid}&im_path=/webcast/im/fetch/&identity=audience&need_persist_msg_count=15\
&insert_task_id=&live_reason=&room_id={room_id}&heartbeatDuration=0",
            dy_pct(&cursor),
            dy_pct(&internal_ext),
        );

        // 签名：QuickJS 跑内嵌的 sign.js 算 X-Bogus（a_bogus 服务端不认，实测过）
        let sig = match sign::x_bogus(&room_id, &uid) {
            Ok(v) => v,
            Err(e) => {
                println!("[douyin] 算签名失败: {e}");
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };
        let tmpl = format!(
            "wss://{{host}}/webcast/im/push/v2/?{query}&signature={}",
            dy_pct(&sig)
        );

        let mut ws = None;
        for host in DY_HOSTS {
            let u = tmpl.replace("{host}", host);
            let mut req = match u.into_client_request() {
                Ok(r) => r,
                Err(e) => {
                    println!("[douyin] {host} 请求构造失败: {e}");
                    continue;
                }
            };
            {
                let h = req.headers_mut();
                let _ = h.insert("origin", "https://live.douyin.com".parse().unwrap());
                let _ = h.insert(
                    "referer",
                    format!("https://live.douyin.com/{rid}").parse().unwrap(),
                );
                let _ = h.insert("user-agent", net::UA.parse().unwrap());
                let _ = h.insert("accept", "application/json, text/plain, */*".parse().unwrap());
                let _ = h.insert(
                    "accept-language",
                    "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap(),
                );
                if !ttwid.is_empty() {
                    let _ = h.insert("cookie", format!("ttwid={ttwid}").parse().unwrap());
                }
            }
            match tokio::time::timeout(
                Duration::from_secs(12),
                tokio_tungstenite::connect_async(req),
            )
            .await
            {
                Ok(Ok((s, _))) => {
                    println!("[douyin] 已连接 {host}");
                    ws = Some(s);
                    break;
                }
                Ok(Err(e)) => println!("[douyin] {host} 失败: {e}"),
                Err(_) => println!("[douyin] {host} 超时"),
            }
        }

        let ws = match ws {
            Some(s) => s,
            None => {
                println!("[douyin] 所有 host 都连不上，10 秒后重试");
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };

        let (mut write, mut read) = ws.split();
        // 立刻发一次心跳
        let _ = write.send(Message::Ping(hb.clone())).await;

        let mut ticker = interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if write.send(Message::Ping(hb.clone())).await.is_err() {
                        break;
                    }
                }
                m = read.next() => {
                    if session_id() != my { return Ok(()); }
                    let data = match m {
                        Some(Ok(Message::Binary(b))) => b,
                        Some(Ok(Message::Ping(p))) => {
                            let _ = write.send(Message::Pong(p)).await;
                            continue;
                        }
                        Some(Ok(_)) => continue,
                        _ => break,
                    };
                    // 帧本身是明文 protobuf；payload(8) 里面才是 gzip 压的 Response
                    let (payload, ptype) = dy_unwrap(&data);
                    if payload.is_empty() {
                        continue;
                    }
                    let plen = payload.len();
                    let resp = gunzip(&payload).unwrap_or(payload);
                    if std::env::var("DOUYIN_DEBUG").is_ok() {
                        println!(
                            "[douyin] 帧 {}B type={ptype:?} payload={plen}B 解压后={}B",
                            data.len(),
                            resp.len()
                        );
                    }
                    for (u, t) in dy_parse_chat(&resp) {
                        emit(&app, "douyin", &u, &t, "#ffffff");
                    }
                    if std::env::var("DOUYIN_DEBUG").is_ok() {
                        let mut names: Vec<String> = Vec::new();
                        let mut r = Pb::new(&resp);
                        while let Some((f, v)) = r.next() {
                            if f != 1 {
                                continue;
                            }
                            if let PbVal::Bytes(m) = v {
                                let mut mm = Pb::new(m);
                                while let Some((f2, v2)) = mm.next() {
                                    if f2 == 1 {
                                        if let PbVal::Bytes(s) = v2 {
                                            names.push(s_of(s));
                                        }
                                    }
                                }
                            }
                        }
                        if !names.is_empty() {
                            println!("[douyin] method 列表: {names:?}");
                        }
                    }
                }
            }
        }
        println!("[douyin] 连接断开，3 秒后重连");
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    Ok(())
}
