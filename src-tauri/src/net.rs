//! 统一 HTTP 客户端。
//!
//! 两条通道：
//! - `direct()`  国内平台直连（走代理反而更慢/被风控）
//! - `via_proxy()` 海外平台（YouTube / Twitch / SOOP）走用户代理
//!
//! 代理地址自动探测：环境变量 → Windows 系统代理（注册表）→ 无。

use once_cell::sync::OnceCell;
use reqwest::{Client, ClientBuilder};
use std::sync::RwLock;
use std::time::Duration;

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

static PROXY: OnceCell<RwLock<Option<String>>> = OnceCell::new();

fn proxy_cell() -> &'static RwLock<Option<String>> {
    PROXY.get_or_init(|| RwLock::new(detect_proxy()))
}

/// 自动探测系统代理
pub fn detect_proxy() -> Option<String> {
    for k in [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(v) = std::env::var(k) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(normalize(v));
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(p) = read_windows_proxy() {
            return Some(p);
        }
    }
    None
}

fn normalize(v: &str) -> String {
    if v.contains("://") {
        v.to_string()
    } else {
        format!("http://{v}")
    }
}

/// 读 HKCU\...\Internet Settings 的 ProxyServer
#[cfg(windows)]
fn read_windows_proxy() -> Option<String> {
    use std::process::Command;
    let out = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyServer",
        ])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().find(|l| l.contains("ProxyServer"))?;
    let val = line.split("REG_SZ").nth(1)?.trim().to_string();
    if val.is_empty() {
        return None;
    }
    // 可能是 "http=127.0.0.1:7897;https=127.0.0.1:7897" 形式
    let host = val
        .split(';')
        .find_map(|p| p.strip_prefix("http="))
        .unwrap_or(val.as_str());
    let host = host.trim();
    if host.is_empty() {
        None
    } else {
        Some(normalize(host))
    }
}

/// 通过 HTTP 代理开一条 CONNECT 隧道，并完成 TLS 握手。
///
/// WebSocket 用不了 reqwest 的代理设置（那是给 HTTP 请求用的），
/// 所以墙外的 WS（Twitch IRC、YouTube 等）必须自己 CONNECT 再套 TLS。
pub async fn proxy_tunnel(
    host: &str,
    port: u16,
) -> Result<tokio_native_tls::TlsStream<tokio::net::TcpStream>, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let proxy = get_proxy().ok_or_else(|| "没有配置代理，连不上墙外服务".to_string())?;
    let bare = proxy
        .split("://")
        .nth(1)
        .unwrap_or(proxy.as_str())
        .trim_end_matches('/');
    let (phost, pport) = match bare.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>().map_err(|_| format!("代理端口不合法: {p}"))?,
        ),
        None => (bare.to_string(), 8080u16),
    };

    let mut tcp = tokio::net::TcpStream::connect((phost.as_str(), pport))
        .await
        .map_err(|e| format!("连不上代理 {phost}:{pport}: {e}"))?;

    let req = format!(
        "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Connection: keep-alive\r\n\r\n"
    );
    tcp.write_all(req.as_bytes())
        .await
        .map_err(|e| format!("写代理请求失败: {e}"))?;

    // 读到响应头结束（\r\n\r\n）
    let mut head: Vec<u8> = Vec::with_capacity(256);
    let mut one = [0u8; 1];
    loop {
        let n = tcp
            .read(&mut one)
            .await
            .map_err(|e| format!("读代理响应失败: {e}"))?;
        if n == 0 {
            break;
        }
        head.push(one[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
        if head.len() > 8192 {
            return Err("代理响应异常（头过长）".into());
        }
    }
    let text = String::from_utf8_lossy(&head);
    let status = text.lines().next().unwrap_or("").to_string();
    if !status.contains(" 200") {
        return Err(format!("代理 CONNECT 被拒: {status}"));
    }

    let cx = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let cx = tokio_native_tls::TlsConnector::from(cx);
    cx.connect(host, tcp)
        .await
        .map_err(|e| format!("TLS 握手失败: {e}"))
}

pub fn get_proxy() -> Option<String> {
    proxy_cell().read().ok().and_then(|g| g.clone())
}

pub fn set_proxy(p: Option<String>) {
    if let Ok(mut w) = proxy_cell().write() {
        *w = p.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    }
}

fn base(timeout: u64) -> ClientBuilder {
    Client::builder()
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .cookie_store(true)
        .user_agent(UA)
}

/// 国内平台直连客户端
pub fn direct() -> Client {
    base(20).build().unwrap_or_else(|_| Client::new())
}

/// 海外平台客户端（走代理）
pub fn via_proxy() -> Client {
    let mut b = base(25);
    if let Some(p) = get_proxy() {
        if let Ok(px) = reqwest::Proxy::all(&p) {
            b = b.proxy(px);
        }
    }
    b.build().unwrap_or_else(|_| Client::new())
}

/// 本地流代理服务器内部回源用（永远直连，不再套代理，避免二次代理）
///
/// 注意：**不能设总 timeout**。直播流是无限长的，一旦设了 timeout，
/// N 秒后连接会被掐断，播放器就报 UnrecoverableEarlyEof / MediaMSEError。
pub fn relay() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .user_agent(UA)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// 海外流回源（YouTube/Twitch 的分片必须走代理），同样不设总 timeout
pub fn relay_proxy() -> Client {
    let mut b = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .user_agent(UA);
    if let Some(p) = get_proxy() {
        if let Ok(px) = reqwest::Proxy::all(&p) {
            b = b.proxy(px);
        }
    }
    b.build().unwrap_or_else(|_| Client::new())
}

// ---------------------------------------------------------------- YouTube Cookie

/// Cookie 落盘位置：%APPDATA%\junlive\<name>
fn cookie_path(name: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var("APPDATA").ok()?;
    let dir = std::path::Path::new(&base).join("junlive");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(name))
}

fn load_cookie_file(name: &str) -> Option<String> {
    cookie_path(name)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn save_cookie_file(name: &str, v: &Option<String>) {
    if let Some(p) = cookie_path(name) {
        match v {
            Some(s) => {
                let _ = std::fs::write(p, s);
            }
            None => {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

static YT_COOKIE: OnceCell<RwLock<Option<String>>> = OnceCell::new();

/// 用户在设置里粘贴的 YouTube Cookie（用来绕过「请确认你不是机器人」）
pub fn get_yt_cookie() -> Option<String> {
    YT_COOKIE
        .get_or_init(|| RwLock::new(load_cookie_file("yt_cookie.txt")))
        .read()
        .ok()
        .and_then(|g| g.clone())
}

pub fn set_yt_cookie(c: Option<String>) {
    let v = c.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    match &v {
        Some(s) => println!("[cookie] YouTube Cookie 已保存（{} 字节）", s.len()),
        None => println!("[cookie] YouTube Cookie 已清空"),
    }
    if let Ok(mut w) = YT_COOKIE.get_or_init(|| RwLock::new(None)).write() {
        *w = v.clone();
    }
    save_cookie_file("yt_cookie.txt", &v);
}

/// 请求 YouTube 时用的 Cookie 头
pub fn yt_cookie_header() -> String {
    get_yt_cookie().unwrap_or_else(|| "SOCS=CAI; CONSENT=YES+cb".to_string())
}

// ---------------------------------------------------------------- B站 Cookie

static BILI_COOKIE: OnceCell<RwLock<Option<String>>> = OnceCell::new();

/// 用户在设置里粘贴的 B站 Cookie。
///
/// B站对 `getDanmuInfo` 做了接口级风控（未登录 / 机房 IP 会返回 -352），
/// 带上浏览器里导出的 Cookie 就能正常拿弹幕 token。
pub fn get_bili_cookie() -> Option<String> {
    BILI_COOKIE
        .get_or_init(|| RwLock::new(None))
        .read()
        .ok()
        .and_then(|g| g.clone())
}

pub fn set_bili_cookie(c: Option<String>) {
    let cell = BILI_COOKIE.get_or_init(|| RwLock::new(None));
    if let Ok(mut w) = cell.write() {
        *w = c.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    }
}

/// 给 B站请求带上 Cookie（没配就返回空串，用匿名身份）
pub fn bili_cookie_header() -> String {
    get_bili_cookie().unwrap_or_default()
}

// ---------------------------------------------------------------- B站 WBI 签名
//
// B站从 2023 年起对搜索、弹幕等接口强制要求 wbi 签名：
//   nav 接口取 img_url/sub_url → 文件名当 key → MIXIN_KEY_ENC_TAB 混淆出 mixin_key
//   → 参数排序拼接（含 wts 时间戳）→ MD5(query + mixin_key) 作为 w_rid
// 缺 w_rid 会直接吃 -352 风控。

const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// B站接口挑 UA，桌面 Firefox 最稳
pub const BILI_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:138.0) Gecko/20100101 Firefox/138.0";

/// B站前端的 URL 编码：`!'()*` 直接丢弃，其余非安全字符按 UTF-8 逐字节 %XX。
/// 注意必须用 ASCII 判断——Rust 的 is_alphanumeric() 对中文也返回 true。
fn wbi_enc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "-_.~".contains(c) {
            out.push(c);
        } else if "!'()*".contains(c) {
            // 与 B站前端一致：不编码也不输出
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

fn wbi_mixin_key(orig: &str) -> String {
    let b = orig.as_bytes();
    MIXIN_KEY_ENC_TAB
        .iter()
        .take(32)
        .map(|&i| b[i] as char)
        .collect()
}

/// 从 `https://.../7cd084941338484aae1a.png` 取出 `7cd084941338484aae1a`
fn take_filename(u: &str) -> String {
    u.rsplit_once('/')
        .and_then(|(_, s)| s.rsplit_once('.'))
        .map(|(s, _)| s.to_string())
        .unwrap_or_default()
}

/// 取 wbi 密钥（img_key, sub_key）
pub async fn wbi_keys(c: &reqwest::Client) -> Result<(String, String), String> {
    let cookie = bili_cookie_header();
    let mut req = c
        .get("https://api.bilibili.com/x/web-interface/nav")
        .header("User-Agent", BILI_UA);
    if !cookie.is_empty() {
        req = req.header("Cookie", cookie);
    }
    let nav: serde_json::Value = req
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let ik = take_filename(nav["data"]["wbi_img"]["img_url"].as_str().unwrap_or(""));
    let sk = take_filename(nav["data"]["wbi_img"]["sub_url"].as_str().unwrap_or(""));
    if ik.is_empty() || sk.is_empty() {
        return Err("拿不到 B站 WBI 密钥".into());
    }
    Ok((ik, sk))
}

/// 签名并返回完整 query string（已含 wts 与 w_rid）
pub fn wbi_sign(params: Vec<(&str, String)>, img_key: &str, sub_key: &str) -> String {
    let mixin = wbi_mixin_key(&format!("{img_key}{sub_key}"));
    let mut p = params;
    let wts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    p.push(("wts", wts.to_string()));
    p.sort_by(|a, b| a.0.cmp(b.0));
    let query = p
        .iter()
        .map(|(k, v)| format!("{}={}", wbi_enc(k), wbi_enc(v)))
        .collect::<Vec<_>>()
        .join("&");
    let digest = md5::compute(format!("{query}{mixin}").as_bytes());
    format!("{query}&w_rid={digest:x}")
}
