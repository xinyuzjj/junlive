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
mod immersive;
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

/// 把回放 m3u8 包成前端能直接播的 `PlayUrl`。
///
/// 走本地代理的理由跟直播一样：分片带防盗链参数，直连会被拦；
/// 代理还会把 m3u8 里的分片地址逐行换成本地地址（hls.js 才能续着拉）。
fn mk_replay_play(m3u8: &str, qualities: Vec<ReplayQuality>) -> PlayUrl {
    let name = qualities
        .iter()
        .find(|q| q.url == m3u8)
        .map(|q| q.name.clone())
        .unwrap_or_else(|| "回放".to_string());
    PlayUrl {
        url: m3u8.to_string(),
        proxy: proxy::wrap(
            m3u8,
            vec![("Referer".to_string(), "https://v.douyu.com/".to_string())],
            false,
        ),
        format: "hls".to_string(),
        quality: name,
        qualities,
    }
}

/// 回放地址缓存：hash_id → 带签名的 m3u8。
///
/// **为什么能缓存**：斗鱼回放取流接口 `getStreamUrlWeb` 的签名只绑定
/// `(vid, tt)` 而**不随时间过期** —— 实测把十分钟前抓到的请求原样重放，
/// 依然返回正常结果（改 tt 或改 vid 才会「权限不足」）。所以解析一次就够。
type ReplayCache = std::collections::HashMap<String, (String, Vec<ReplayQuality>)>;
fn replay_cache() -> &'static std::sync::Mutex<ReplayCache> {
    static C: std::sync::OnceLock<std::sync::Mutex<ReplayCache>> = std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 解析回放的播放地址（带签名的 m3u8）。
///
/// 为什么绕这么大一圈：斗鱼回放取流接口 `wgapi/vodnc/front/stream/getStreamUrlWeb`
/// 要求签名（`v`+`did`+`tt`+`sign`+`vid`），不签名直接返回「权限不足」；
/// 签名算法在混淆 JS 里且字符串是动态拼的，静态搜不出来。
///
/// 所以这里用「**让官方页面自己算**」的办法：
///   1. 开一个隐藏 WebView 打开回放页（带 `ap=1`，页面会自动起播）
///   2. 页面里的播放器拿到 m3u8 后会写进 `<video>.currentSrc`
///   3. 注入脚本轮询这个值，拿到后跳到一个假域名把地址带出来
///   4. 我们用 `on_navigation` 拦下这次跳转，读出地址，关掉窗口
///
/// 拿到之后就能用**我们自己的播放器**放了（走本地代理解决防盗链），
/// 不再需要内嵌整个官方页面。
#[tauri::command]
async fn resolve_replay(app: tauri::AppHandle, hash_id: String) -> Result<PlayUrl, String> {
    use std::sync::{Arc, Mutex};
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    if let Some((u, qs)) = replay_cache().lock().unwrap().get(&hash_id) {
        return Ok(mk_replay_play(u, qs.clone()));
    }

    let label = format!("vodprobe{}", hash_id.replace(|c: char| !c.is_alphanumeric(), ""));
    // 每次解析前先清掉可能残留的同名窗口
    if let Some(old) = tauri::Manager::get_webview_window(&app, &label) {
        let _ = old.close();
    }

    let target: tauri::Url = format!("https://v.douyu.com/show/{hash_id}?ap=1")
        .parse()
        .map_err(|e| format!("URL 解析失败: {e}"))?;

    let sink: Arc<Mutex<Option<tokio::sync::oneshot::Sender<String>>>> =
        Arc::new(Mutex::new(None));
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    *sink.lock().unwrap() = Some(tx);
    let sink_nav = sink.clone();

    // 注入脚本：把播放地址「带出去」。
    //
    // 为什么不能只看 <video>.currentSrc：斗鱼播放器走 MSE，
    // currentSrc 是 `blob:` 地址，拿不到真实 m3u8。
    //
    // 主路是**钩 `getStreamUrlWeb` 的响应体** —— 它一次给全所有清晰度
    // （标清480P / 高清720P / 1080P60 / 原画2K60），有了它用户才能切清晰度。
    // 钩 m3u8 请求只是兜底（万一响应体结构变了），故意延后 250ms 发，
    // 让响应体那条路先赢 —— 不然只能拿到一个地址、没有档位列表。
    const PROBE_JS: &str = r#"
(function () {
  var done = false;
  function report(payload) {
    if (done) return;
    done = true;
    try {
      location.href = 'https://junlive.local/vod?d=' +
        encodeURIComponent(JSON.stringify(payload));
    } catch (e) {}
  }
  function later(u) {
    setTimeout(function () { report({ url: u, qualities: [] }); }, 250);
  }
  function hit(u) { return u && String(u).indexOf('.m3u8') >= 0; }

  var oOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (m, u) {
    try {
      this.__u = String(u);
      if (hit(u)) later(String(u));
    } catch (e) {}
    return oOpen.apply(this, arguments);
  };

  var oSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function () {
    var self = this;
    try {
      if (self.__u && self.__u.indexOf('getStreamUrlWeb') >= 0) {
        self.addEventListener('load', function () {
          try {
            var j = JSON.parse(self.responseText);
            var tv = j && j.data && j.data.thumb_video;
            if (!tv) return;
            var qs = [];
            for (var k in tv) {
              var q = tv[k];
              if (q && q.url) {
                qs.push({ id: k, name: q.name || k, url: q.url,
                          bitrate: q.bit_rate || 0, level: q.level || 0 });
              }
            }
            if (!qs.length) return;
            qs.sort(function (a, b) { return (b.level || 0) - (a.level || 0); });
            report({ url: qs[0].url, qualities: qs });
          } catch (e) {}
        });
      }
    } catch (e) {}
    return oSend.apply(this, arguments);
  };

  var oFetch = window.fetch;
  if (oFetch) {
    window.fetch = function (input) {
      try {
        var u = (typeof input === 'string') ? input : (input && input.url);
        if (hit(u)) later(String(u));
      } catch (e) {}
      return oFetch.apply(this, arguments);
    };
  }

  // 兜底：页面已经在加载途中（钩子装晚了）时，从 resource timing 里翻
  var n = 0;
  var t = setInterval(function () {
    n++;
    var list = performance.getEntriesByType('resource');
    for (var i = 0; i < list.length; i++) {
      if (hit(list[i].name)) { clearInterval(t); later(list[i].name); return; }
    }
    var v = document.querySelector('video');
    var s = v && (v.currentSrc || v.src);
    if (hit(s)) { clearInterval(t); later(s); return; }
    if (n > 120) clearInterval(t);
  }, 500);
})();
"#;

    let w = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(target))
        .title("正在解析回放地址…")
        .visible(false)
        .inner_size(1280.0, 800.0)
        // initialization_script 在页面自己的脚本之前执行 —— 必须这么早，
        // 否则播放器已经发完请求，XHR 钩子就白装了
        .initialization_script(PROBE_JS)
        .on_navigation(move |u| {
            let s = u.as_str();
            if let Some(rest) = s.strip_prefix("https://junlive.local/vod?d=") {
                if let Some(tx) = sink_nav.lock().unwrap().take() {
                    let _ = tx.send(rest.to_string());
                }
                return false; // 拦下这次跳转，只取参数
            }
            true
        })
        .build()
        .map_err(|e| format!("打开解析窗口失败: {e}"))?;

    let got = tokio::time::timeout(std::time::Duration::from_secs(35), rx).await;
    let _ = w.close();

    match got {
        Ok(Ok(raw)) => {
            let json = urlencoding::decode(&raw)
                .map(|s| s.into_owned())
                .unwrap_or(raw);
            let v: serde_json::Value =
                serde_json::from_str(&json).map_err(|e| format!("解析回放数据失败: {e}"))?;
            let url = v["url"].as_str().unwrap_or("").to_string();
            if url.is_empty() {
                return Err("没能拿到回放地址".into());
            }
            // 每档都包一层本地代理，前端切清晰度时直接用 proxy
            let mut qualities: Vec<ReplayQuality> = Vec::new();
            if let Some(arr) = v["qualities"].as_array() {
                for q in arr {
                    let qu = q["url"].as_str().unwrap_or("");
                    if qu.is_empty() {
                        continue;
                    }
                    // 用户明确不要「原画2K60」（1440p60a）：码率跟 1080P60 几乎一样
                    // （8171K vs 7975K）却更吃带宽，删掉少一个选项。
                    if q["id"].as_str().unwrap_or("") == "1440p60a" {
                        continue;
                    }
                    qualities.push(ReplayQuality {
                        id: q["id"].as_str().unwrap_or("").to_string(),
                        name: q["name"].as_str().unwrap_or("").to_string(),
                        url: qu.to_string(),
                        proxy: proxy::wrap(
                            qu,
                            vec![("Referer".to_string(), "https://v.douyu.com/".to_string())],
                            false,
                        ),
                        bitrate: q["bitrate"].as_i64().unwrap_or(0),
                        level: q["level"].as_i64().unwrap_or(0),
                    });
                }
            }
            replay_cache()
                .lock()
                .unwrap()
                .insert(hash_id, (url.clone(), qualities.clone()));
            Ok(mk_replay_play(&url, qualities))
        }
        Ok(Err(_)) => Err("解析窗口已关闭".into()),
        Err(_) => Err("解析回放地址超时（斗鱼页面结构可能变了）".into()),
    }
}

/// 一场回放的完整分段列表（斗鱼把一场直播切成若干 2 小时的段）。
#[tauri::command]
async fn get_replay_parts(
    platform: String,
    room_id: String,
    hash_id: String,
    show_start: Option<i64>,
) -> Result<Vec<Replay>, String> {
    platforms::replay_parts(&platform, &room_id, &hash_id, show_start.unwrap_or(0)).await
}

/// 一场回放的历史弹幕。
#[tauri::command]
async fn get_replay_danmaku(
    platform: String,
    hash_id: String,
    start_time: Option<i64>,
) -> Result<Vec<ReplayDanmaku>, String> {
    platforms::replay_danmaku(&platform, &hash_id, start_time.unwrap_or(0)).await
}

/// 一场回放的 AI 看点（拿不到就空列表，前端不显示）。
#[tauri::command]
async fn get_replay_highlights(
    platform: String,
    hash_id: String,
    start_time: Option<i64>,
    duration: Option<i64>,
) -> Result<Vec<ReplayHighlight>, String> {
    platforms::replay_highlights(
        &platform,
        &hash_id,
        start_time.unwrap_or(0),
        duration.unwrap_or(0),
    )
    .await
}

/// 主播的直播回放列表（当前只有斗鱼）。
#[tauri::command]
async fn get_replays(
    platform: String,
    room_id: String,
    page: Option<u32>,
) -> Result<ReplayPage, String> {
    platforms::replays(&platform, &room_id, page.unwrap_or(1)).await
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

use immersive::android_immersive;


/// 需要走代理的平台（海外）。
///
/// ⚠️ 必须与 `net.rs` 的分流保持一致，也与前端 `src/embed.ts` 的
/// `OVERSEAS_PLATFORMS` 一致：
///   国内 bilibili / douyu / huya / douyin —— 直连
///   海外 twitch / youtube / soop        —— 必须走代理
///
/// 之前这里是**全局**给 WebView 挂代理，等于让斗鱼虎牙B站抖音也绕代理走一圈，
/// 那是「国内外混在一起」，既慢又可能触发风控。三处必须一致，改一处就要改三处。
pub const OVERSEAS_PLATFORMS: &[&str] = &["twitch", "youtube", "soop"];

/// 这个平台要不要走代理。放到子模块里定义，避免 `#[tauri::command]`
/// 生成的宏在 lib.rs 顶层与 `generate_handler!` 撞名（E0255）。
pub mod routing {
    use super::OVERSEAS_PLATFORMS;

    /// 设置页/播放页切换平台时问一句：国内直连，海外走代理。
    #[tauri::command]
    pub fn needs_proxy(platform: String) -> bool {
        OVERSEAS_PLATFORMS.contains(&platform.as_str())
    }
}

/// **刻意不做** WebView 代理注入 —— 查过之后发现这是多余的，而且有害。
///
/// 曾经的错误做法：给 WebView2 塞 `--proxy-server`、给 Linux 塞
/// `http_proxy`，想让官方播放器能走代理。当时的判断是
/// 「iframe 里的请求不经过 Rust，读不到我们存的代理」。
///
/// 查证结果（对照 github.com/ilanzgx/multistream —— 它全项目零代理代码）：
///   ① WebView2 / WebKit **本来就默认读系统代理**
///      （Windows 读 HKCU\...\Internet Settings 的 ProxyServer；
///        本机实测 ProxyEnable=1、ProxyServer=127.0.0.1:7897）。
///   ② 强行塞 `--proxy-server` 反而会**覆盖**系统设置 ——
///      用户改了系统代理但应用还按旧地址走，表现为「改了设置也没用」。
///   ③ 正确做法是**根本不用 iframe 播放器**：
///      Twitch 改回自建流（GQL -> usher -> 本地代理回源），
///      请求由 Rust 发，`net::relay_proxy()` 读得到代理；
///      m3u8 与分片都经 `proxy::wrap(url, headers, true)`，
///      第三个参数 true = 走代理，国内平台传 false = 直连。
///      这套分流在 `proxy.rs:371` 已经实现好了。
///
/// 保留 `OVERSEAS_PLATFORMS` / `needs_proxy` 作为**唯一的分流真相源**，
/// 给前端判断当前平台是否海外用（与 embed.ts 里的同名常量必须一致）。

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
            get_replays,
            get_replay_parts,
            get_replay_danmaku,
            get_replay_highlights,
            resolve_replay,
            set_stream_renew,
            android_immersive,
            routing::needs_proxy,
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
                        // 可选显示条数：`categories <plat> [n]`（默认 40，排查时给大值）
                        let lim: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(40);
                        for c in cs.iter().take(lim) {
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
                // 可选页码：`rooms <plat> <cat> [page]`，用来验翻页有没有重复
                let pg: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
                match platforms::rooms(&p, &c, pg).await {
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
            "rparts" => {
                let p = args.get(1).cloned().unwrap_or_default();
                let id = args.get(2).cloned().unwrap_or_default();
                let h = args.get(3).cloned().unwrap_or_default();
                let st: i64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
                match platforms::replay_parts(&p, &id, &h, st).await {
                    Ok(rs) => {
                        println!("共 {} 段", rs.len());
                        for r in rs.iter().take(10) {
                            println!(
                                "  [{}] {} start={} | {}",
                                r.hash_id, r.duration, r.start_time, r.title
                            );
                        }
                    }
                    Err(e) => println!("ERR {e}"),
                }
            }
            "rdanmaku" => {
                let p = args.get(1).cloned().unwrap_or_default();
                let h = args.get(2).cloned().unwrap_or_default();
                let st: i64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
                match platforms::replay_danmaku(&p, &h, st).await {
                    Ok(ms) => {
                        println!("共 {} 条弹幕", ms.len());
                        for m in ms.iter().take(8) {
                            println!("  [{:>7.1}s] {} | {}", m.time, m.user, m.text);
                        }
                        if let Some(last) = ms.last() {
                            println!("  最后一条在 {:.1}s", last.time);
                        }
                    }
                    Err(e) => println!("ERR {e}"),
                }
            }
            "rhl" => {
                let p = args.get(1).cloned().unwrap_or_default();
                let h = args.get(2).cloned().unwrap_or_default();
                let st: i64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
                let du: i64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
                match platforms::replay_highlights(&p, &h, st, du).await {
                    Ok(hs) => {
                        println!("共 {} 条看点（段起点 {st} 时长 {du}）", hs.len());
                        for x in hs.iter().take(8) {
                            println!("  [{:>7.1}s] {}", x.time, x.title);
                        }
                    }
                    Err(e) => println!("ERR {e}"),
                }
            }
            "replays" => {
                let p = args.get(1).cloned().unwrap_or_default();
                let id = args.get(2).cloned().unwrap_or_default();
                let pg: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
                match platforms::replays(&p, &id, pg).await {
                    Ok(rs) => {
                        println!("本页 {} 条 / 共 {} 场", rs.list.len(), rs.total);
                        for r in rs.list.iter().take(12) {
                            println!("  [{}] {} | {} | {}", r.hash_id, r.time, r.duration, r.title);
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
                        println!(
                            "开播={} 录播={} 人气={} 分区={}",
                            d.room.live,
                            d.room.replay,
                            d.room.online,
                            d.room.area
                        );
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
