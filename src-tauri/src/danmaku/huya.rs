use super::*;

// ============================ 虎牙 ============================
//
// wss://cdnws.api.huya.com，TARS 编码（腾讯自研序列化）。
//   注册包 = wscmd{ 0:int32(16), 1:bytes( oos{ 0:list<string> topics, 1:string("") } ) }
//   数据包 = { 0:int32(7), 1:bytes( { 1:int32(kind), 2:bytes(payload) } ) }
//   kind == 1400 是聊天弹幕；payload 里 0 是用户结构（字段 2 是昵称）、3 是内容。
//
// 坑：TARS 的整数是**自适应宽度**的——能塞进 int8 就用 1 字节，其次 int16/int32/int64，
// 长度字段同理。写成固定 4 字节 big-endian 服务端会解析失败（连接正常但零回包）。
// BYTES 也不是裸长度前缀，而是 SimpleList：head(tag,13) + head(0,INT8) + 自适应长度 + 数据。

const HUYA_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36";

/// 心跳包（固定字节，每 20 秒发一次）
const HUYA_HEARTBEAT: &[u8] = b"\x00\x03\x1d\x00\x00\x69\x00\x00\x00\x69\x10\x03\x2c\x3c\x4c\x56\x08\x6f\x6e\x6c\x69\x6e\x65\x75\x69\x66\x0f\x4f\x6e\x55\x73\x65\x72\x48\x65\x61\x72\x74\x42\x65\x61\x74\x7d\x00\x00\x3c\x08\x00\x01\x06\x04\x74\x52\x65\x71\x1d\x00\x00\x2f\x0a\x0a\x0c\x16\x00\x26\x00\x36\x07\x61\x64\x72\x5f\x77\x61\x70\x46\x00\x0b\x12\x03\xae\xf0\x0f\x22\x03\xae\xf0\x0f\x3c\x42\x6d\x52\x02\x60\x5c\x60\x01\x7c\x82\x00\x0b\xb0\x1f\x9c\xac\x0b\x8c\x98\x0c\xa8\x0c";

/// TARS 写入器（只覆盖虎牙用到的类型）
struct TarsWriter {
    buf: Vec<u8>,
}

impl TarsWriter {
    fn new() -> Self {
        TarsWriter { buf: Vec::new() }
    }
    fn finish(self) -> Vec<u8> {
        self.buf
    }
    fn head(&mut self, tag: u8, ty: u8) {
        if tag < 15 {
            self.buf.push((tag << 4) | ty);
        } else {
            self.buf.push(0xf0 | ty);
            self.buf.push(tag);
        }
    }
    /// 自适应整数：int8 / int16 / int32 / int64
    fn write_int(&mut self, tag: u8, v: i64) {
        if v >= i8::MIN as i64 && v <= i8::MAX as i64 {
            self.head(tag, 0);
            self.buf.push(v as i8 as u8);
        } else if v >= i16::MIN as i64 && v <= i16::MAX as i64 {
            self.head(tag, 1);
            self.buf.extend_from_slice(&(v as i16).to_be_bytes());
        } else if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
            self.head(tag, 2);
            self.buf.extend_from_slice(&(v as i32).to_be_bytes());
        } else {
            self.head(tag, 3);
            self.buf.extend_from_slice(&v.to_be_bytes());
        }
    }
    fn write_string(&mut self, tag: u8, s: &str) {
        let b = s.as_bytes();
        if b.len() <= u8::MAX as usize {
            self.head(tag, 6);
            self.buf.push(b.len() as u8);
        } else {
            self.head(tag, 7);
            self.buf.extend_from_slice(&(b.len() as u32).to_be_bytes());
        }
        self.buf.extend_from_slice(b);
    }
    /// BYTES = SimpleList
    fn write_bytes(&mut self, tag: u8, b: &[u8]) {
        self.head(tag, 13);
        self.head(0, 0);
        self.write_int(0, b.len() as i64);
        self.buf.extend_from_slice(b);
    }
    fn write_list_string(&mut self, tag: u8, items: &[String]) {
        self.head(tag, 9);
        self.write_int(0, items.len() as i64);
        for it in items {
            self.write_string(0, it);
        }
    }
}

/// TARS 读取器
struct TarsReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> TarsReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        TarsReader { buf, pos: 0 }
    }
    fn byte(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos + n > self.buf.len() {
            return None;
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }
    fn head(&mut self) -> Option<(u8, u8)> {
        let b = self.byte()?;
        let ty = b & 0x0f;
        let mut tag = b >> 4;
        if tag == 15 {
            tag = self.byte()?;
        }
        Some((tag, ty))
    }
    /// 按类型读整数（自适应）
    fn read_int(&mut self, ty: u8) -> Option<i64> {
        Some(match ty {
            0 => self.byte()? as i8 as i64,
            1 => i16::from_be_bytes(self.take(2)?.try_into().ok()?) as i64,
            2 => i32::from_be_bytes(self.take(4)?.try_into().ok()?) as i64,
            3 => i64::from_be_bytes(self.take(8)?.try_into().ok()?),
            12 => 0,
            _ => return None,
        })
    }
    /// 跳过一个完整字段（head + 值）
    fn skip_any(&mut self) -> Option<()> {
        let (_, ty) = self.head()?;
        self.skip_value(ty)
    }

    /// 跳过值（调用时 head 已经读掉了）
    fn skip_value(&mut self, ty: u8) -> Option<()> {
        match ty {
            0 => {
                self.take(1)?;
            }
            1 => {
                self.take(2)?;
            }
            2 => {
                self.take(4)?;
            }
            3 => {
                self.take(8)?;
            }
            4 => {
                self.take(4)?;
            }
            5 => {
                self.take(8)?;
            }
            6 => {
                let n = self.byte()? as usize;
                self.take(n)?;
            }
            7 => {
                let n = u32::from_be_bytes(self.take(4)?.try_into().ok()?) as usize;
                self.take(n)?;
            }
            8 => {
                let (_, t) = self.head()?;
                let mut count = self.read_int(t)? as usize;
                if count > 100_000 {
                    count = 100_000;
                }
                for _ in 0..count {
                    self.skip_any()?;
                    self.skip_any()?;
                }
                return Some(());
            }
            9 => {
                let (_, t) = self.head()?;
                let mut count = self.read_int(t)? as usize;
                if count > 100_000 {
                    count = 100_000;
                }
                for _ in 0..count {
                    self.skip_any()?;
                }
                return Some(());
            }
            10 => {
                loop {
                    let save = self.pos;
                    let (tag, t) = self.head()?;
                    if tag == 0 && t == 11 {
                        break;
                    }
                    self.pos = save;
                    self.skip_any()?;
                }
            }
            11 => {
                let n = self.byte()? as usize;
                self.take(n)?;
            }
            12 => {}
            13 => {
                // SimpleList：head(0,INT8) 是元素类型，紧跟着长度字段自己的 head
                let _ = self.head()?;
                let (_, t) = self.head()?;
                let n = self.read_int(t)? as usize;
                self.take(n)?;
            }
            _ => return None,
        }
        Some(())
    }

    fn read_int32(&mut self, want: u8) -> Option<i32> {
        loop {
            let save = self.pos;
            let (tag, ty) = self.head()?;
            if tag == want {
                if let Some(v) = self.read_int(ty) {
                    return Some(v as i32);
                }
                self.pos = save;
                return None;
            }
            self.pos = save;
            self.skip_any()?;
        }
    }
    fn read_string(&mut self, want: u8) -> Option<String> {
        loop {
            let save = self.pos;
            let (tag, ty) = self.head()?;
            if tag == want {
                if ty == 6 {
                    let n = self.byte()? as usize;
                    return self.take(n).map(|s| String::from_utf8_lossy(s).to_string());
                }
                if ty == 7 {
                    let n = u32::from_be_bytes(self.take(4)?.try_into().ok()?) as usize;
                    return self.take(n).map(|s| String::from_utf8_lossy(s).to_string());
                }
                self.pos = save;
                return None;
            }
            self.pos = save;
            self.skip_any()?;
        }
    }
    fn read_bytes(&mut self, want: u8) -> Option<Vec<u8>> {
        loop {
            let save = self.pos;
            let (tag, ty) = self.head()?;
            if tag == want {
                if ty == 13 {
                    // SimpleList 布局：head(tag,13) + head(0,INT8)[元素类型]
                    //                 + 长度字段(自带 head) + 数据
                    let _ = self.head()?;
                    let (_, t) = self.head()?;
                    let n = self.read_int(t)? as usize;
                    return self.take(n).map(|s| s.to_vec());
                }
                if ty == 6 {
                    let n = self.byte()? as usize;
                    return self.take(n).map(|s| s.to_vec());
                }
                if ty == 7 {
                    let n = u32::from_be_bytes(self.take(4)?.try_into().ok()?) as usize;
                    return self.take(n).map(|s| s.to_vec());
                }
                self.pos = save;
                return None;
            }
            self.pos = save;
            self.skip_any()?;
        }
    }

    /// 读一个 STRUCT 字段：head(tag, 10) ... head(0, 11)，返回内部字节。
    /// TARS 的 struct 是嵌套类型，不能当 bytes(13) 读。
    fn read_struct(&mut self, want: u8) -> Option<Vec<u8>> {
        loop {
            let save = self.pos;
            let (tag, ty) = self.head()?;
            if tag == want && ty == 10 {
                let begin = self.pos;
                loop {
                    let save2 = self.pos;
                    let (t2, ty2) = self.head()?;
                    if t2 == 0 && ty2 == 11 {
                        return Some(self.buf[begin..save2].to_vec());
                    }
                    self.pos = save2;
                    self.skip_any()?;
                }
            }
            self.pos = save;
            self.skip_any()?;
        }
    }
}

/// 拿虎牙弹幕的 uid —— 必须是房间页里的 `lp`（主播 uid）。
///
/// 注意：`mp.huya.com` 的 `profileInfo.yyid` 是**另一个值**，用它订 topic 只会收到
/// 房间状态广播，收不到聊天弹幕。DTV 也是从房间页 HTML 里抠 lp/ayyuid 的。
async fn huya_uid(rid: &str) -> Result<String, String> {
    let c = net::direct();
    let html = c
        .get(format!("https://www.huya.com/{rid}"))
        .header("User-Agent", HUYA_UA)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Referer", "https://www.huya.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    // 1) TT_PROFILE_INFO = { ... } → 取 lp / ayyuid / yyuid
    if let Some(i) = html.find("TT_PROFILE_INFO") {
        if let Some(off) = html[i..].find('{') {
            let start = i + off;
            let mut depth = 0i32;
            for (k, ch) in html[start..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            let js = &html[start..start + k + 1];
                            if let Ok(v) = serde_json::from_str::<Value>(js) {
                                for key in ["lp", "ayyuid", "yyuid"] {
                                    if let Some(n) = v.get(key) {
                                        let got = match n {
                                            Value::Number(x) => x.to_string(),
                                            Value::String(x) => x.clone(),
                                            _ => String::new(),
                                        };
                                        if !got.is_empty() && got != "0" {
                                            return Ok(got);
                                        }
                                    }
                                }
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // 2) 退而求其次：直接找 "lp":数字 / "ayyuid":数字 / "yyuid":数字
    for key in ["\"lp\"", "\"ayyuid\"", "\"yyuid\""] {
        if let Some(i) = html.find(key) {
            let digits: String = html[i + key.len()..]
                .trim_start_matches(|ch: char| ch == ':' || ch == ' ' || ch == '"')
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if !digits.is_empty() && digits != "0" {
                return Ok(digits);
            }
        }
    }

    // 3) 最后兜底：profileRoom 的 yyid，再不行用房间号
    if let Ok(v) = c
        .get(format!(
            "https://mp.huya.com/cache.php?m=Live&do=profileRoom&roomid={rid}&showSecret=1"
        ))
        .header("User-Agent", HUYA_UA)
        .header("Accept", "*/*")
        .header("Origin", "https://www.huya.com")
        .header("Referer", "https://www.huya.com/")
        .send()
        .await
        .map_err(|e| e.to_string())
    {
        if let Ok(j) = v.json::<Value>().await {
            if let Some(n) = j["data"]["profileInfo"]["yyid"].as_i64() {
                if n > 0 {
                    return Ok(n.to_string());
                }
            }
        }
    }
    Ok(rid.to_string())
}

pub(super) async fn run_huya(app: Option<AppHandle>, rid: String, my: u64) -> Result<(), String> {
    let uid = huya_uid(&rid).await?;
    println!("[huya] rid={rid} uid(lp)={uid}");
    let topics = vec![format!("live:{uid}"), format!("chat:{uid}")];

    let mut oos = TarsWriter::new();
    oos.write_list_string(0, &topics);
    oos.write_string(1, "");
    let mut wscmd = TarsWriter::new();
    wscmd.write_int(0, 16);
    wscmd.write_bytes(1, &oos.finish());
    let reg = wscmd.finish();

    let (ws, _) = tokio_tungstenite::connect_async("wss://cdnws.api.huya.com")
        .await
        .map_err(|e| format!("连接虎牙弹幕失败: {e}"))?;
    let (mut write, mut read) = ws.split();
    write
        .send(Message::Binary(reg))
        .await
        .map_err(|e| e.to_string())?;

    let mut hb = interval(Duration::from_secs(20));
    loop {
        tokio::select! {
            _ = hb.tick() => {
                if write.send(Message::Binary(HUYA_HEARTBEAT.to_vec())).await.is_err() { break; }
            }
            m = read.next() => {
                if session_id() != my { break; }
                let data = match m {
                    Some(Ok(Message::Binary(b))) => b,
                    Some(Ok(_)) => continue,
                    _ => break,
                };
                // 排障用：HUYA_DEBUG=1 时打印每帧 kind 并落盘 payload
                if std::env::var("HUYA_DEBUG").is_ok() {
                    println!("[huya] frame {}B kind={:?}", data.len(), huya_kind(&data));
                    if let Some(pl) = huya_payload(&data) {
                        let hx: String = pl.iter().map(|b| format!("{:02x}", b)).collect();
                        use std::io::Write;
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open("E:/hermes_app/cache/scratch/huya_dump.txt")
                        {
                            let _ = writeln!(
                                f,
                                "{}\t{}\t{}",
                                huya_kind(&data).unwrap_or(-1),
                                pl.len(),
                                hx
                            );
                        }
                    }
                }
                if let Some((user, text)) = huya_parse(&data) {
                    emit(&app, "huya", &user, &text, "#ffffff");
                }
            }
        }
    }
    Ok(())
}

/// 从 payload 的 tag=4（list<struct>）里取出第一个 struct 的原始字节
fn huya_first_struct(payload: &[u8]) -> Option<Vec<u8>> {
    let mut r = TarsReader::new(payload);
    loop {
        let save = r.pos;
        let (tag, ty) = r.head()?;
        if tag == 4 && ty == 9 {
            // LIST 的布局和 SimpleList 不同：head(tag,9) 之后**直接**是元素个数
            // （write_int32 自带 head），没有额外的元素类型字节。
            let (_, t) = r.head()?;
            let count = r.read_int(t)?;
            if count < 1 {
                return None;
            }
            // 第一个元素必须是 STRUCT
            let (_, ety) = r.head()?;
            if ety != 10 {
                return None;
            }
            let begin = r.pos;
            // 扫到 STRUCT_END（tag=0, ty=11）
            loop {
                let save2 = r.pos;
                let (t2, ty2) = r.head()?;
                if t2 == 0 && ty2 == 11 {
                    return Some(payload[begin..save2].to_vec());
                }
                r.pos = save2;
                r.skip_any()?;
            }
        }
        r.pos = save;
        r.skip_any()?;
    }
}

/// 取一帧的 payload 原始字节（诊断用）
fn huya_payload(data: &[u8]) -> Option<Vec<u8>> {
    let mut top = TarsReader::new(data);
    if top.read_int32(0)? != 7 {
        return None;
    }
    let b1 = top.read_bytes(1)?;
    let mut inner = TarsReader::new(&b1);
    let _ = inner.read_int32(1);
    inner.read_bytes(2)
}

/// 取一帧的 kind（b1 的 tag=1）。只为诊断用。
fn huya_kind(data: &[u8]) -> Option<i32> {
    let mut top = TarsReader::new(data);
    if top.read_int32(0)? != 7 {
        return None;
    }
    let b1 = top.read_bytes(1)?;
    let mut inner = TarsReader::new(&b1);
    inner.read_int32(1)
}

/// DTV 那一版的老结构：nested == 1400 时
///   payload = {0: struct{2: 昵称}, 3: string(内容), 6: struct{0: 颜色}}
fn huya_parse_dtv(payload: &[u8]) -> Option<(String, String)> {
    let mut r = TarsReader::new(payload);
    let text = r.read_string(3).unwrap_or_default();
    if text.trim().is_empty() || text.len() > 200 {
        return None;
    }
    // user 是个 STRUCT：{0: uid i64, 1: imid i64, 2: 昵称, 3: gender}
    let user = {
        let mut u = TarsReader::new(payload);
        match u.read_struct(0) {
            Some(b) => TarsReader::new(&b).read_string(2).unwrap_or_default(),
            None => String::new(),
        }
    };
    Some((user, text))
}

/// 现网结构（实际抓包扒出来的）：payload 的 tag=4 是 list<struct{1: 昵称, 2: 内容}>
fn huya_parse_list(payload: &[u8]) -> Option<(String, String)> {
    let st = huya_first_struct(payload)?;
    let mut s = TarsReader::new(&st);
    let user = s.read_string(1).unwrap_or_default();
    let text = s.read_string(2).unwrap_or_default();
    if user.trim().is_empty() || text.trim().is_empty() || text.len() > 200 {
        return None;
    }
    Some((user, text))
}

/// 虎牙弹幕。两种结构都试：
///   top     = {0: int32(7), 1: bytes(b1)}
///   b1      = {0: int8, 1: int16(kind), 2: bytes(payload)}
///   payload = DTV 老协议：kind==1400，{0:user{2:name}, 3:text, 6:fmt}
///             现网协议：{0..3: int32, 4: list<struct{0:int8, 1:name, 2:text}>}
fn huya_parse(data: &[u8]) -> Option<(String, String)> {
    let mut top = TarsReader::new(data);
    if top.read_int32(0)? != 7 {
        return None;
    }
    let b1 = top.read_bytes(1)?;
    let mut inner = TarsReader::new(&b1);
    let nested = inner.read_int32(1).unwrap_or(-1);
    let payload = inner.read_bytes(2)?;

    if nested == 1400 {
        if let Some(v) = huya_parse_dtv(&payload) {
            return Some(v);
        }
    }
    huya_parse_list(&payload)
}

