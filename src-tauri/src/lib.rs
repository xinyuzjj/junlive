//! JunLive —— 多平台直播聚合桌面客户端
//!
//! 后端职责：
//! 1. 解析 7 个平台的房间信息与播放地址（统一成 model 里的结构）
//! 2. 本地流代理：解决防盗链 / CORS / 海外代理
//! 3. 代理设置

mod danmaku;
mod flv_renew;
mod model;
mod net;
mod platforms;
mod proxy;

use model::*;

#[tauri::command]
async fn list_platforms() -> Vec<PlatformInfo> {
    platforms::all()
}

#[tauri::command]
async fn get_categories(platform: String) -> Result<Vec<Category>, String> {
    platforms::categories(&platform).await
}

#[tauri::command]
async fn get_rooms(
    platform: String,
    category: String,
    page: Option<u32>,
) -> Result<RoomList, String> {
    platforms::rooms(&platform, &category, page.unwrap_or(1)).await
}

#[tauri::command]
async fn search_rooms(
    platform: String,
    keyword: String,
    page: Option<u32>,
) -> Result<RoomList, String> {
    platforms::search(&platform, &keyword, page.unwrap_or(1)).await
}

#[tauri::command]
async fn get_room(platform: String, room_id: String) -> Result<RoomDetail, String> {
    platforms::room_detail(&platform, &room_id).await
}

/// 登记「续流上下文」：流地址到期后由本地代理自己重新解析，播放器无感。
///
/// 为什么必须做：所有平台的流地址都带时效，**斗鱼固定 300 秒就被 CDN 主动 EOF**
/// （服务端策略，跟网络无关），所以不管的话看 5 分钟必断。
/// room_id 传空字符串表示清除（切房间 / 离开播放页时调）。
#[tauri::command]
async fn set_stream_renew(
    platform: String,
    room_id: String,
    quality: String,
) -> Result<(), String> {
    proxy::set_renew(if room_id.is_empty() {
        None
    } else {
        Some(proxy::RenewCtx {
            platform,
            room_id,
            quality,
        })
    });
    Ok(())
}

#[tauri::command]
async fn get_proxy_setting() -> Option<String> {
    net::get_proxy()
}

#[tauri::command]
async fn set_proxy_setting(proxy: Option<String>) {
    net::set_proxy(proxy);
}

#[tauri::command]
async fn get_proxy_port() -> u16 {
    proxy::port()
}

#[tauri::command]
async fn start_danmaku(
    app: tauri::AppHandle,
    platform: String,
    room_id: String,
) -> Result<(), String> {
    danmaku::start(Some(app), &platform, &room_id).await
}

#[tauri::command]
async fn stop_danmaku() {
    danmaku::stop();
}

#[tauri::command]
async fn get_bilibili_cookie() -> Option<String> {
    net::get_bili_cookie()
}

#[tauri::command]
async fn set_bilibili_cookie(cookie: Option<String>) {
    net::set_bili_cookie(cookie);
}

/// 打开一个 YouTube 登录窗口，登录成功后自动把 Cookie 抓下来存好。
///
/// 背景：YouTube 对机房/代理 IP 会返回 `LOGIN_REQUIRED`（bot 墙），
/// 手动从浏览器导出 Cookie 太麻烦。这里直接开一个 WebView 让用户登录，
/// 再通过 Tauri 的 `cookies_for_url` 把 Cookie 读出来。
/// 登录成功的判定：出现 `SAPISID` / `__Secure-3PSID` / `SID` / `LOGIN_INFO` 之一。
#[tauri::command]
async fn youtube_login(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let target: tauri::Url = "https://www.youtube.com/"
        .parse()
        .map_err(|e| format!("URL 解析失败: {e}"))?;

    let w = WebviewWindowBuilder::new(&app, "yt_login", WebviewUrl::External(target.clone()))
        .title("登录 YouTube —— 登录成功后本窗口会自动关闭")
        .inner_size(1000.0, 720.0)
        .build()
        .map_err(|e| format!("打开登录窗口失败: {e}"))?;

    // 最多等 5 分钟
    for _ in 0..300 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        // 用户手动把窗口关了（或点错）→ 立刻收工返回，
        // 否则轮询会一直跑，前端按钮会一直卡在「等待登录中」不能点。
        if tauri::Manager::get_webview_window(&app, "yt_login").is_none() {
            return Err("登录窗口已关闭，未检测到登录".into());
        }
        let cs = match w.cookies_for_url(target.clone()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let logged = cs.iter().any(|c| {
            matches!(
                c.name(),
                "SAPISID" | "__Secure-3PSID" | "SID" | "LOGIN_INFO" | "__Secure-1PSID"
            )
        });
        if logged {
            let header = cs
                .iter()
                .map(|c| format!("{}={}", c.name(), c.value()))
                .collect::<Vec<_>>()
                .join("; ");
            net::set_yt_cookie(Some(header));
            let _ = w.close();
            return Ok("登录成功，YouTube Cookie 已自动保存".into());
        }
    }
    let _ = w.close();
    Err("登录超时（5 分钟），请重试".into())
}

#[tauri::command]
async fn get_youtube_cookie() -> Option<String> {
    net::get_yt_cookie()
}

#[tauri::command]
async fn set_youtube_cookie(cookie: Option<String>) {
    net::set_yt_cookie(cookie);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| {
            // 启动本地流代理
            tauri::async_runtime::spawn(async {
                proxy::start().await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_platforms,
            get_categories,
            get_rooms,
            search_rooms,
            get_room,
            set_stream_renew,
            get_proxy_setting,
            set_proxy_setting,
            get_proxy_port,
            get_youtube_cookie,
            youtube_login,
            set_youtube_cookie,
            get_bilibili_cookie,
            set_bilibili_cookie,
            start_danmaku,
            stop_danmaku
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 命令行自测模式（不启动 GUI）：
///   junlive.exe --test platforms
///   junlive.exe --test categories bilibili
///   junlive.exe --test rooms bilibili 英雄联盟
///   junlive.exe --test search douyu 英雄联盟
///   junlive.exe --test room huya 149721
pub fn cli_test(args: &[String]) {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runtime error: {e}");
            return;
        }
    };
    rt.block_on(async move {
        let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
        match cmd {
            "danmaku" => {
                let platform = args.get(1).cloned().unwrap_or_default();
                let room = args.get(2).cloned().unwrap_or_default();
                if platform.is_empty() || room.is_empty() {
                    println!("用法: --test danmaku <平台> <房间号>");
                    return;
                }
                let secs: u64 = args
                    .get(3)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30);
                println!("弹幕诊断: {platform} / {room}（{secs} 秒）");
                if let Err(e) = danmaku::start(None, &platform, &room).await {
                    println!("启动失败: {e}");
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                println!("--- {secs} 秒结束 ---");
            }
            "platforms" => {
                for p in platforms::all() {
                    println!("{}\t{}\t{}", p.id, p.name, if p.danmaku { "有弹幕" } else { "-" });
                }
            }
            "categories" => {
                let p = args.get(1).cloned().unwrap_or_default();
                match platforms::categories(&p).await {
                    Ok(cs) => {
                        println!("{} 个顶级分区", cs.len());
                        for c in cs.iter().take(40) {
                            if c.children.is_empty() {
                                println!("  {:<12} {}", c.id, c.name);
                            } else {
                                println!("  {:<12} {}  ({} 个子分区: {})", c.id, c.name, c.children.len(),
                                    c.children.iter().take(6).map(|x| x.name.as_str()).collect::<Vec<_>>().join("/"));
                            }
                        }
                    }
                    Err(e) => println!("ERR {e}"),
                }
            }
            "rooms" => {
                let p = args.get(1).cloned().unwrap_or_default();
                let c = args.get(2).cloned().unwrap_or_default();
                match platforms::rooms(&p, &c, 1).await {
                    Ok(r) => {
                        println!("共 {} 个房间", r.rooms.len());
                        for x in r.rooms.iter().take(10) {
                            println!("  [{}] {} | {} | {}", x.room_id, x.streamer, x.title, x.online);
                        }
                    }
                    Err(e) => println!("ERR {e}"),
                }
            }
            "search" => {
                let p = args.get(1).cloned().unwrap_or_default();
                let k = args.get(2).cloned().unwrap_or_default();
                match platforms::search(&p, &k, 1).await {
                    Ok(r) => {
                        println!("共 {} 条结果", r.rooms.len());
                        for x in r.rooms.iter().take(10) {
                            println!("  [{}] {} | {} | {}", x.room_id, x.streamer, x.title, x.online);
                        }
                    }
                    Err(e) => println!("ERR {e}"),
                }
            }
            "room" => {
                let p = args.get(1).cloned().unwrap_or_default();
                let id = args.get(2).cloned().unwrap_or_default();
                match platforms::room_detail(&p, &id).await {
                    Ok(d) => {
                        println!("平台={} 房间={} 主播={}", d.room.platform, d.room.room_id, d.room.streamer);
                        println!("标题={}", d.room.title);
                        println!("开播={} 人气={} 分区={}", d.room.live, d.room.online, d.room.area);
                        println!("播放地址 {} 条", d.plays.len());
                        for pl in &d.plays {
                            let u: String = pl.url.chars().take(130).collect();
                            println!("  [{}] {} {}", pl.quality, pl.format, u);
                            println!("      proxy = {}", pl.proxy);
                        }
                    }
                    Err(e) => println!("ERR {e}"),
                }
            }
            "serve" => {
                let p = args.get(1).cloned().unwrap_or_default();
                let id = args.get(2).cloned().unwrap_or_default();
                let port = proxy::start().await;
                println!("本地代理已启动: http://127.0.0.1:{port}");
                match platforms::room_detail(&p, &id).await {
                    Ok(d) => {
                        println!(
                            "房间: {} | {} | 开播={}",
                            d.room.streamer, d.room.title, d.room.live
                        );
                        if d.plays.is_empty() {
                            println!("没有播放地址");
                        }
                        for pl in d.plays.iter().take(1) {
                            println!(
                                "[{}] {} {}",
                                pl.quality,
                                pl.format,
                                pl.url.chars().take(80).collect::<String>()
                            );
                            println!("   代理地址: {}", pl.proxy);
                            let c = reqwest::Client::new();
                            match c.get(&pl.proxy).send().await {
                                Ok(mut r) => {
                                    let status = r.status();
                                    let ct = r
                                        .headers()
                                        .get("content-type")
                                        .and_then(|v| v.to_str().ok())
                                        .unwrap_or("")
                                        .to_string();
                                    let is_text = ct.contains("mpegurl")
                                        || ct.contains("json")
                                        || ct.starts_with("text/");
                                    if !is_text {
                                        // 二进制流（FLV）：持续读 25 秒，验证不会被总超时掐断
                                        let deadline = tokio::time::Instant::now()
                                            + std::time::Duration::from_secs(25);
                                        let mut total = 0usize;
                                        let mut chunks = 0usize;
                                        let mut eof = false;
                                        loop {
                                            let left = deadline.saturating_duration_since(
                                                tokio::time::Instant::now(),
                                            );
                                            if left.is_zero() {
                                                break;
                                            }
                                            match tokio::time::timeout(left, r.chunk()).await {
                                                Ok(Ok(Some(b))) => {
                                                    total += b.len();
                                                    chunks += 1;
                                                    if total > 6_000_000 {
                                                        break;
                                                    }
                                                }
                                                Ok(Ok(None)) => {
                                                    eof = true;
                                                    break;
                                                }
                                                Ok(Err(e)) => {
                                                    println!("   读流出错: {e}");
                                                    break;
                                                }
                                                Err(_) => break,
                                            }
                                        }
                                        println!(
                                            "   代理响应: {status} {ct} → 25 秒内 {chunks} 块 / {total} 字节{}",
                                            if eof { "（流提前结束！）" } else { "" }
                                        );
                                        continue;
                                    }
                                    let body = r.text().await.unwrap_or_default();
                                    println!("   代理响应: {status} {ct}  长度={}", body.len());
                                    println!(
                                        "   内容前 320 字:\n{}",
                                        body.chars().take(320).collect::<String>()
                                    );
                                    // 顺便实测一个分片能不能拉下来（验证防盗链请求头有没有传对）
                                    if let Some(seg) =
                                        body.lines().find(|l| l.trim().starts_with("http"))
                                    {
                                        let seg = seg.trim().to_string();
                                        match c.get(&seg).send().await {
                                            Ok(sr) => {
                                                let st = sr.status();
                                                let n = sr
                                                    .bytes()
                                                    .await
                                                    .map(|b| b.len())
                                                    .unwrap_or(0);
                                                println!("   >>> 分片实测: {st}  {n} 字节");
                                            }
                                            Err(e) => println!("   >>> 分片请求失败: {e}"),
                                        }
                                    }
                                }
                                Err(e) => println!("   代理请求失败: {e}"),
                            }
                        }
                        // HOLD=1 时让代理继续存活，方便外部脚本走完整条 HLS 链
                        if std::env::var("HOLD").is_ok() {
                            println!("   (HOLD=1，代理保持 300 秒)");
                            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                        }
                    }
                    Err(e) => println!("ERR {e}"),
                }
            }
            _ => println!(
                "用法:\n  junlive.exe --test platforms\n  junlive.exe --test categories <平台>\n  junlive.exe --test rooms <平台> <分区>\n  junlive.exe --test search <平台> <关键词>\n  junlive.exe --test room <平台> <房间号>\n  junlive.exe --test serve <平台> <房间号>   (启动代理并实测拉流)"
            ),
        }
    });
}
