//! 本地流代理服务器。
//!
//! 存在的理由：
//! 1. 斗鱼/虎牙/B站/抖音的流有防盗链（Referer / UA / Cookie），WebView 里 hls.js
//!    没法自定义这些请求头；
//! 2. 跨域（CORS）问题；
//! 3. 海外流要套代理。
//!
//! 做法：每个上游 URL 在本地注册成一个短 id，前端只播
//! `http://127.0.0.1:{port}/s/{id}`，由 Rust 侧带上正确请求头回源。
//! m3u8 播放列表会被逐行重写，分片地址同样注册成新的 id。

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{StreamExt, TryStreamExt};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct Entry {
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// 是否走系统代理（海外流）
    pub proxy: bool,
    /// 是否对 FLV 做「断流重连 + 时间戳改写」。
    ///
    /// 只有**短连接**的平台需要（虎牙：二三十秒断一次）。
    /// 斗鱼 / B站是长连接，套上重连反而会把原本正常的流搞坏
    /// （重连时新流的时间戳会跳，播放器就卡住），所以按平台分别设置。
    pub flv_reconnect: bool,
}

#[derive(Clone, Default)]
pub struct ProxyState {
    pub map: Arc<Mutex<HashMap<String, Entry>>>,
    counter: Arc<Mutex<u64>>,
    /// 上游 URL → 已分配的代理 id。
    ///
    /// **必须做这个反向映射**：m3u8 每次被重写（即每次刷新播放列表）都会给
    /// 每个分片重新走一遍 wrap()。如果每次都发新 id，播放器拿到的分片 URL
    /// 就会随每次刷新而变 —— hls.js 发现「片段地址变了」会把在飞的请求 abort 掉，
    /// 表现就是分片整片 ERR_ABORTED、currentTime 卡住、最后报网络错误。
    /// （SOOP 这种每次刷新都完整重写播放列表的平台最容易踩。）
    rev: Arc<Mutex<HashMap<String, String>>>,
}

impl ProxyState {
    pub fn register(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        proxy: bool,
        flv_reconnect: bool,
    ) -> String {
        // 先看这个上游地址有没有已经发过 id，参数一致就直接复用
        if let Some(id) = self.rev.lock().unwrap().get(url).cloned() {
            let map = self.map.lock().unwrap();
            if let Some(e) = map.get(&id) {
                let same = e.proxy == proxy
                    && e.flv_reconnect == flv_reconnect
                    && e.headers == headers;
                if same {
                    return id;
                }
            }
        }

        let id = {
            let mut c = self.counter.lock().unwrap();
            *c += 1;
            format!("{:x}{:x}", std::process::id(), *c)
        };
        self.map.lock().unwrap().insert(
            id.clone(),
            Entry {
                url: url.to_string(),
                headers,
                proxy,
                flv_reconnect,
            },
        );
        self.rev
            .lock()
            .unwrap()
            .insert(url.to_string(), id.clone());
        id
    }
}

static STATE: Lazy<ProxyState> = Lazy::new(ProxyState::default);
static PORT: Lazy<Mutex<u16>> = Lazy::new(|| Mutex::new(0));

pub fn port() -> u16 {
    *PORT.lock().unwrap()
}

pub fn base() -> String {
    format!("http://127.0.0.1:{}", port())
}

/// 注册进代理，**原始转发**（不重连、不改时间戳）。
/// 斗鱼 / B站这种长连接平台用这个。
pub fn wrap(url: &str, headers: Vec<(String, String)>, proxy: bool) -> String {
    wrap_ex(url, headers, proxy, false)
}

/// 注册进代理，**带 FLV 断流重连 + 时间戳改写**。虎牙这种短连接平台用这个。
pub fn wrap_flv(url: &str, headers: Vec<(String, String)>, proxy: bool) -> String {
    wrap_ex(url, headers, proxy, true)
}

// ===================== 续流（流地址到期后自愈） =====================
//
// 所有平台的流地址都带时效，**斗鱼最狠：固定 300 秒，到点 CDN 主动 EOF**
// （与网络无关，有些线路 URL 甚至不带 expire 参数也照样准时断 —— 是服务端策略）。
// 表现就是「看 5 分钟自己结束」。
//
// 参考 DTV 的做法（src-tauri/src/flv_relay.rs）：让**代理层**自己续流，
// 播放器完全无感。区别是 DTV 会在旧流到期前**预取**下一条并在 tag 边界接续；
// 我们这里更简单——重连时**重新解析一次地址**再连，播放器那边看到的是
// 「先是卡一下，然后继续播」，不会断掉。
//
// 之所以能接着播：斗鱼的 FLV tag 用的是**推流会话的绝对时间戳**，
// 重新取流后时间轴是延续的，不用改写。只要剥掉新流的 FLV 文件头即可
// （见下面 FlvState 的处理）。

/// 续流所需的上下文：拿什么参数去重新解析
#[derive(Clone, Debug)]
pub struct RenewCtx {
    pub platform: String,
    pub room_id: String,
    /// 画质名（各平台自己的叫法），重新解析后按它挑回原来那档
    pub quality: String,
}

static RENEW: Mutex<Option<RenewCtx>> = Mutex::new(None);

/// 登记续流上下文。传 None 清除（切房间/关播放器时调用）。
pub fn set_renew(ctx: Option<RenewCtx>) {
    *RENEW.lock().unwrap() = ctx;
}

fn renew_ctx() -> Option<RenewCtx> {
    RENEW.lock().unwrap().clone()
}

/// 用续流上下文重新解析一次地址，挑回同一画质。
/// 失败就返回 None（调用方继续用旧地址，至少不比现在更差）。
/// 供 `flv_renew`（斗鱼 FLV 续流）复用。
pub(crate) async fn resolve_renew_url() -> Option<String> {
    let quality = renew_ctx().map(|c| c.quality).unwrap_or_default();
    resolve_fresh_url(&quality).await
}

async fn resolve_fresh_url(quality: &str) -> Option<String> {
    let ctx = renew_ctx()?;
    let plays = match ctx.platform.as_str() {
        "douyu" => crate::platforms::douyu::play_urls(&ctx.room_id).await.ok()?,
        "bilibili" => {
            let n: i64 = ctx.room_id.parse().ok()?;
            crate::platforms::bilibili::play_urls(n).await.ok()?
        }
        _ => return None,
    };
    // 优先同画质；画质名对不上（平台改名）就退回第一条，有得播总比断着强
    let hit = plays
        .iter()
        .find(|p| p.quality == quality)
        .or_else(|| plays.first())?;
    Some(hit.url.clone())
}

pub fn wrap_ex(
    url: &str,
    headers: Vec<(String, String)>,
    proxy: bool,
    flv_reconnect: bool,
) -> String {
    if port() == 0 {
        return url.to_string();
    }
    let id = STATE.register(url, headers, proxy, flv_reconnect);
    format!("{}/s/{}", base(), id)
}

/// 清掉旧的注册项（切房间时调用，避免内存无限增长）
pub fn clear() {
    STATE.map.lock().unwrap().clear();
    STATE.rev.lock().unwrap().clear();
}

/// 启动本地代理，返回监听端口
pub async fn start() -> u16 {
    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[proxy] bind failed: {e}");
            return 0;
        }
    };
    let p = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    *PORT.lock().unwrap() = p;
    let app = Router::new()
        .route("/s/{id}", get(handle).options(preflight))
        .route("/health", get(|| async { "ok" }))
        .with_state(STATE.clone());
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[proxy] server error: {e}");
        }
    });
    println!("[proxy] listening on 127.0.0.1:{p}");
    p
}

/// FLV 直播流的转发状态。
///
/// 上游是短连接，重连后新流的**时间戳会从 0 重新开始**，而播放器已经播到几十秒，
/// 于是它一直等一个「比当前大」的时间戳 —— 表现就是卡在「缓存中」。
/// 这里把每条 tag 的时间戳统一加上一个偏移，让多次连接拼成一条连续的时间轴。
struct FlvState {
    buf: Vec<u8>,
    header_skipped: bool,
    /// true = 把文件头丢掉（重连时用）；false = 原样输出（第一条连接必须保留，
    /// 否则播放器拿到的第一段不是 `FLV\x01` 开头，直接报 FormatUnsupported）
    strip_header: bool,
    first_ts: Option<i64>,
    offset: i64,
    last_out_ts: i64,
}

impl FlvState {
    fn new() -> Self {
        FlvState {
            buf: Vec::new(),
            header_skipped: false,
            strip_header: false,
            first_ts: None,
            offset: 0,
            last_out_ts: 0,
        }
    }

    /// 每次（重）连上游时调用。strip_header=true 表示这次要把文件头剥掉。
    fn begin(&mut self, strip_header: bool) {
        self.buf.clear();
        self.strip_header = strip_header;
        self.header_skipped = false;
        self.first_ts = None;
    }

    /// 喂一段原始字节，返回可以转发出去的字节（已改写时间戳、已剥文件头）
    fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        self.buf.extend_from_slice(data);
        let mut out: Vec<u8> = Vec::new();

        // FLV 文件头 9 字节 + PreviousTagSize0 4 字节
        if !self.header_skipped {
            if self.buf.len() < 13 {
                return out;
            }
            if self.strip_header {
                // 重连：丢掉，避免播放器以为来了第二个文件
                self.buf.drain(..13);
            } else {
                // 第一条连接：必须原样透传，否则 FormatUnsupported
                out.extend_from_slice(&self.buf[..13]);
                self.buf.drain(..13);
            }
            self.header_skipped = true;
        }

        let mut pos = 0usize;
        while self.buf.len() - pos >= 15 {
            let dsize = ((self.buf[pos + 1] as usize) << 16)
                | ((self.buf[pos + 2] as usize) << 8)
                | (self.buf[pos + 3] as usize);
            let total = 11 + dsize + 4;
            if self.buf.len() - pos < total {
                break;
            }
            // 时间戳：低 24 位 + 高 8 位
            let ts_lo = ((self.buf[pos + 4] as i64) << 16)
                | ((self.buf[pos + 5] as i64) << 8)
                | (self.buf[pos + 6] as i64);
            let ts = ((self.buf[pos + 7] as i64) << 24) | ts_lo;

            if self.first_ts.is_none() {
                self.first_ts = Some(ts);
                // 重连：把新流的起点接到「已经播到的位置」后面一点
                if self.last_out_ts > 0 {
                    self.offset = self.last_out_ts + 30 - ts;
                }
            }

            let new_ts = ts + self.offset;
            if new_ts > self.last_out_ts {
                self.last_out_ts = new_ts;
            }
            self.buf[pos + 4] = ((new_ts >> 16) & 0xff) as u8;
            self.buf[pos + 5] = ((new_ts >> 8) & 0xff) as u8;
            self.buf[pos + 6] = (new_ts & 0xff) as u8;
            self.buf[pos + 7] = ((new_ts >> 24) & 0xff) as u8;

            pos += total;
        }

        if pos > 0 {
            out.extend_from_slice(&self.buf[..pos]);
            self.buf.drain(..pos);
        }
        out
    }
}

/// 把 mpsc 接收端包成 Stream，用于「上游断开 → 重连 → 继续灌」的直播流
struct ChannelStream(tokio::sync::mpsc::Receiver<Result<bytes::Bytes, std::io::Error>>);

impl futures_util::Stream for ChannelStream {
    type Item = Result<bytes::Bytes, std::io::Error>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

async fn preflight() -> Response {
    let mut r = StatusCode::NO_CONTENT.into_response();
    cors(r.headers_mut());
    r
}

fn cors(h: &mut axum::http::HeaderMap) {
    h.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("*"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET,HEAD,OPTIONS"),
    );
    h.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("*"),
    );
}

async fn handle(
    Path(id): Path<String>,
    State(st): State<ProxyState>,
    req: axum::http::Request<Body>,
) -> Response {
    let entry = st.map.lock().unwrap().get(&id).cloned();
    let Some(e) = entry else {
        return (StatusCode::NOT_FOUND, "stream not registered").into_response();
    };

    let client = if e.proxy {
        crate::net::relay_proxy()
    } else {
        crate::net::relay()
    };
    let mut rb = client.get(&e.url);
    for (k, v) in &e.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    if let Some(r) = req.headers().get(header::RANGE) {
        rb = rb.header(header::RANGE, r.clone());
    }

    let resp = match rb.send().await {
        Ok(r) => {
            if std::env::var("PROXY_DEBUG").is_ok() {
                println!(
                    "[proxy] {} -> {} {}",
                    e.url.chars().take(1200).collect::<String>(),
                    r.status(),
                    r.headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                );
                if !r.status().is_success() {
                    for (k, v) in r.headers().iter() {
                        let ks = k.as_str();
                        if ks.starts_with("x-goog") || ks == "www-authenticate" || ks == "server" {
                            println!("      hdr {ks}: {}", v.to_str().unwrap_or("?"));
                        }
                    }
                }
            }
            r
        }
        Err(err) => {
            let mut r = (StatusCode::BAD_GATEWAY, format!("upstream error: {err}")).into_response();
            cors(r.headers_mut());
            return r;
        }
    };

    let status = resp.status();
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let clen = resp.headers().get(header::CONTENT_LENGTH).cloned();
    let crange = resp.headers().get(header::CONTENT_RANGE).cloned();
    let is_m3u8 = ct.contains("mpegurl")
        || ct.contains("x-mpegURL")
        || e.url.contains(".m3u8")
        || e.url.contains("playlist");

    if is_m3u8 && status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let rewritten = rewrite_m3u8(&text, &e);
        let mut r = Response::new(Body::from(rewritten));
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/vnd.apple.mpegurl"),
        );
        r.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        );
        cors(r.headers_mut());
        return r;
    }

    // FLV 直播流：虎牙/斗鱼这类上游是短连接（二三十秒断一次），断了必须重连，
    // 否则播放器播几秒就停。
    //
    // 关键坑：重连后的新流会**再带一个 FLV 文件头**（`FLV\x01...`，9 字节 + 4 字节
    // PreviousTagSize0）。直接灌给播放器，它会以为来了第二个文件而卡住/暂停——
    // 上一版就是栽在这里，还把原本正常的斗鱼搞坏了。所以从第二条连接起剥掉这 13 字节。
    let is_flv = ct.contains("x-flv") || e.url.contains(".flv");

    // 【续流路径】有续流上下文时（斗鱼），走 DTV 那套「在 tag 边界接续」的做法：
    // 旧流到期前预取新地址、接续时丢文件头/初始化 tag、等第一个越过旧时间轴的
    // 关键帧、丢掉 CDN 回吐的重复 GOP —— **不改时间戳**（斗鱼是绝对时间戳，
    // 改了反而坏；上一版就是照虎牙那套改写把斗鱼改崩的）。
    // 参考：github.com/chen-zeong/DTV 的 src-tauri/src/flv_relay.rs
    if is_flv && status.is_success() && e.flv_reconnect && renew_ctx().is_some() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(64);
        let headers = e.headers.clone();
        let use_proxy = e.proxy;
        let body = resp.bytes_stream();
        tokio::spawn(async move {
            crate::flv_renew::relay(body, headers, use_proxy, tx).await;
        });
        let mut r = Response::new(Body::from_stream(ChannelStream(rx)));
        *r.status_mut() = status;
        let ctv = if ct.is_empty() { "video/x-flv" } else { ct.as_str() };
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(ctv).unwrap_or(HeaderValue::from_static("video/x-flv")),
        );
        r.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
        return r;
    }

    // 【旧的 FLV 重连路径】虎牙这种短连接平台用：重连后会**改写时间戳**，
    // 把多次连接拼成一条连续时间轴。
    // 注意：**不要给斗鱼用这条** —— 斗鱼是绝对时间戳，改写会直接搞坏。
    if is_flv && status.is_success() && e.flv_reconnect {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(64);
        let url = e.url.clone();
        let headers = e.headers.clone();
        let use_proxy = e.proxy;
        tokio::spawn(async move {
            let mut flv = FlvState::new();
            let mut first = true;
            let mut cur_url = url.clone();
            loop {
                // 首段之外，每次重连都重新解析一次地址 —— 旧地址可能已经过期
                // （斗鱼 300 秒），拿旧地址重连只会立刻再次 EOF，然后永远卡住。
                // 画质取续流上下文里的当前值，这样用户在界面上换画质也能跟上。
                if !first {
                    let q = renew_ctx().map(|c| c.quality).unwrap_or_default();
                    if let Some(fresh) = resolve_fresh_url(&q).await {
                        if fresh != cur_url {
                            eprintln!("[proxy] 流地址已续签，用新地址重连");
                            cur_url = fresh;
                        }
                    }
                }
                let client = if use_proxy {
                    crate::net::relay_proxy()
                } else {
                    crate::net::relay()
                };
                let mut rb = client.get(&cur_url);
                for (k, v) in &headers {
                    rb = rb.header(k.as_str(), v.as_str());
                }
                flv.begin(!first);
                first = false;
                if let Ok(resp) = rb.send().await {
                    let mut st = resp.bytes_stream();
                    while let Some(chunk) = st.next().await {
                        match chunk {
                            Ok(b) => {
                                let ready = flv.feed(&b);
                                if !ready.is_empty()
                                    && tx.send(Ok(bytes::Bytes::from(ready))).await.is_err()
                                {
                                    return; // 播放器那边已断开
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
                if tx.is_closed() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        });

        let mut r = Response::new(Body::from_stream(ChannelStream(rx)));
        *r.status_mut() = status;
        let ctv = if ct.is_empty() { "video/x-flv" } else { ct.as_str() };
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(ctv).unwrap_or(HeaderValue::from_static("video/x-flv")),
        );
        r.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
        return r;
    }

    let stream = resp
        .bytes_stream()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()));
    let mut r = Response::new(Body::from_stream(stream));
    *r.status_mut() = status;
    if !ct.is_empty() {
        if let Ok(v) = HeaderValue::from_str(&ct) {
            r.headers_mut().insert(header::CONTENT_TYPE, v);
        }
    }
    if let Some(v) = clen {
        r.headers_mut().insert(header::CONTENT_LENGTH, v);
    }
    if let Some(v) = crange {
        r.headers_mut().insert(header::CONTENT_RANGE, v);
    }
    r.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    r.headers_mut().insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("*"),
    );
    r
}

/// 把 m3u8 里的分片/子列表/密钥地址全部换成本地代理地址
fn rewrite_m3u8(text: &str, e: &Entry) -> String {
    let base = reqwest::Url::parse(&e.url).ok();
    let mut out = String::with_capacity(text.len() + 1024);
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            out.push('\n');
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            // 处理带 URI="..." 的标签：#EXT-X-KEY / #EXT-X-MAP / #EXT-X-MEDIA 等
            if rest.contains("URI=\"") {
                out.push_str(&rewrite_tag_uri(line, base.as_ref(), e));
            } else {
                out.push_str(line);
            }
            out.push('\n');
        } else {
            let abs = resolve(base.as_ref(), line);
            let wrapped = wrap(&abs, e.headers.clone(), e.proxy);
            out.push_str(&wrapped);
            out.push('\n');
        }
    }
    out
}

fn rewrite_tag_uri(line: &str, base: Option<&reqwest::Url>, e: &Entry) -> String {
    let mut result = String::with_capacity(line.len() + 64);
    let mut rest = line;
    while let Some(pos) = rest.find("URI=\"") {
        let start = pos + 5;
        result.push_str(&rest[..start]);
        let after = &rest[start..];
        if let Some(end) = after.find('"') {
            let uri = &after[..end];
            let abs = resolve(base, uri);
            let wrapped = wrap(&abs, e.headers.clone(), e.proxy);
            result.push_str(&wrapped);
            rest = &after[end..];
        } else {
            result.push_str(after);
            rest = "";
            break;
        }
    }
    result.push_str(rest);
    result
}

fn resolve(base: Option<&reqwest::Url>, target: &str) -> String {
    if target.starts_with("http://") || target.starts_with("https://") {
        return target.to_string();
    }
    if let Some(b) = base {
        if let Ok(u) = b.join(target) {
            return u.to_string();
        }
    }
    target.to_string()
}
