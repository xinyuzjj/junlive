//! FLV 续流：在旧流到期前预取新流，并在 **FLV tag 边界**上接续成一条不中断的流。
//!
//! 算法照抄 DTV 的 `src-tauri/src/flv_relay.rs`（chen-zeong/DTV）—— 这是实测能
//! 「看十几个小时不断」的做法，**不要凭直觉改**：
//!
//! - 斗鱼直链寿命**固定 300 秒**，到点 CDN 主动 EOF，与网络无关
//!   （部分线路 URL 甚至不带 `expire` 参数但同样准时断开，说明是服务端策略）。
//!   斗鱼官方网页端就是每 ~300 秒重新调一次取流接口续签。
//! - **不改写时间戳**：斗鱼 FLV tag 用的是**推流会话的绝对时间戳**，
//!   重新取流后时间轴是延续的而非归零。上一版我们按虎牙那套「重连后把时间戳
//!   接到已播位置」去改写，结果把斗鱼直接改坏（时间戳被推飞、播放器报错）。
//! - **首段原样转发**（不逐 tag 重写）：逐 tag 转发会把一次写放大成几十次，
//!   反而拖累流畅度；首段只在旁边解析一遍，用来记下时间轴位置。
//! - **接续段**：丢掉文件头、SCRIPT 与初始化 tag，从「第一个越过旧流位置的
//!   关键帧」开始接；之后 CDN 回吐的重复 GOP 也一并丢（按时间轴判断）。

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use std::time::{Duration, Instant};

/// 上游直链寿命。斗鱼固定 300 秒。
const STREAM_TTL: Duration = Duration::from_secs(300);
/// 提前量：到这个点就去预取下一段（而不是等 EOF，那时播放器已经卡了）
const RENEW_LEAD: Duration = Duration::from_secs(8);
const RETRY_DELAY: Duration = Duration::from_millis(500);
const MAX_CONSECUTIVE_FAILURES: u32 = 5;
/// 缓冲超过这个量还吐不出 tag，说明上游不是可解析的 FLV，退化为纯直通
const MAX_PENDING_BUFFER: usize = 8 * 1024 * 1024;

/// 允许关键帧「略微靠后」的容忍度（毫秒）。
///
/// 换 CDN 节点后新流常比旧流慢一点（实测约 1.5 s），它的关键帧会落在
/// 已转发位置**之后一点点**。严格「必须超前」就得再等一个 GOP ——
/// 实测那就是 1525 ms 的空档，正是「每 5 分钟卡一下」。
///
/// 为什么回退是安全的：播放器本身落后转发位置好几秒，这点重叠落在
/// **还没播到的区间**里，MSE 直接覆盖，不会重复播放。
const KEYFRAME_TOLERANCE_MS: u32 = 2500;

const TAG_TYPE_AUDIO: u8 = 8;
const TAG_TYPE_VIDEO: u8 = 9;
const TAG_TYPE_SCRIPT: u8 = 18;

struct FlvTag {
    raw: Bytes,
    tag_type: u8,
    timestamp: u32,
    is_keyframe: bool,
    is_sequence_header: bool,
}

#[derive(Default)]
struct FlvDemuxer {
    buf: BytesMut,
    header_parsed: bool,
}

impl FlvDemuxer {
    fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    fn try_parse_header(&mut self) -> bool {
        if self.header_parsed {
            return true;
        }
        if self.buf.len() < 13 {
            return false;
        }
        if &self.buf[..3] == b"FLV" {
            let data_offset =
                u32::from_be_bytes([self.buf[5], self.buf[6], self.buf[7], self.buf[8]]) as usize;
            let total = data_offset.saturating_add(4);
            if self.buf.len() < total {
                return false;
            }
            let _ = self.buf.split_to(total);
        }
        self.header_parsed = true;
        true
    }

    fn take_header(&mut self) {
        if self.try_parse_header() {
            // 文件头已消费（split_to 里丢掉了），这里只是保证解析发生过
        }
    }

    fn is_desynced(&self) -> bool {
        self.buf.len() > MAX_PENDING_BUFFER
    }

    fn next_tag(&mut self) -> Option<FlvTag> {
        if !self.try_parse_header() {
            return None;
        }
        if self.buf.len() < 11 {
            return None;
        }
        let data_size = ((self.buf[1] as usize) << 16)
            | ((self.buf[2] as usize) << 8)
            | (self.buf[3] as usize);
        let total = 11 + data_size + 4;
        if self.buf.len() < total {
            return None;
        }
        let raw = self.buf.split_to(total).freeze();
        let tag_type = raw[0];
        let ts_lo = ((raw[4] as u32) << 16) | ((raw[5] as u32) << 8) | (raw[6] as u32);
        let timestamp = ((raw[7] as u32) << 24) | ts_lo;
        let (is_keyframe, is_sequence_header) = match tag_type {
            TAG_TYPE_VIDEO if data_size > 0 => {
                let first = raw[11] >> 4;
                let second = if data_size > 1 { raw[12] } else { 1 };
                (first == 1, second == 0)
            }
            TAG_TYPE_AUDIO if data_size > 0 => (false, raw[11] == 0),
            _ => (false, false),
        };
        Some(FlvTag {
            raw,
            tag_type,
            timestamp,
            is_keyframe,
            is_sequence_header,
        })
    }
}

/// 记录已转发到的时间轴位置，接续时用来丢掉新流回吐的重复 tag。
/// 音视频分开记，避免交错导致误丢。
#[derive(Default)]
struct Timeline {
    last_video_ts: Option<u32>,
    last_audio_ts: Option<u32>,
}

impl Timeline {
    fn observe(&mut self, tag: &FlvTag) {
        match tag.tag_type {
            TAG_TYPE_VIDEO => self.last_video_ts = Some(tag.timestamp),
            TAG_TYPE_AUDIO => self.last_audio_ts = Some(tag.timestamp),
            _ => {}
        }
    }

    /// tag 是否在已转发位置之后、但靠后不超过 `tol_ms`（允许的小幅回退）。
    fn within_behind(&self, tag: &FlvTag, tol_ms: u32) -> bool {
        let last = match tag.tag_type {
            TAG_TYPE_VIDEO => self.last_video_ts,
            TAG_TYPE_AUDIO => self.last_audio_ts,
            _ => None,
        };
        match last {
            Some(prev) => tag.timestamp.saturating_add(tol_ms) >= prev,
            None => true,
        }
    }

    /// 接受一个略微靠后的关键帧后，把时间轴位置回退到它。
    /// 不回退的话，后面那些 tag 会被判成「重复」全部丢掉，等于白接。
    fn rewind_to(&mut self, tag: &FlvTag) {
        self.last_video_ts = Some(tag.timestamp);
        self.last_audio_ts = Some(tag.timestamp);
    }

    fn is_ahead(&self, tag: &FlvTag) -> bool {
        let last = match tag.tag_type {
            TAG_TYPE_VIDEO => self.last_video_ts,
            TAG_TYPE_AUDIO => self.last_audio_ts,
            _ => None,
        };
        match last {
            Some(prev) => tag.timestamp > prev,
            None => true,
        }
    }
}

/// 向上游取一段新的流。返回 bytes 流。
pub async fn open_stream(url: &str, headers: &[(String, String)], use_proxy: bool) -> Option<impl futures_util::Stream<Item = reqwest::Result<Bytes>>> {
    let client = if use_proxy {
        crate::net::relay_proxy()
    } else {
        crate::net::relay()
    };
    let mut rb = client.get(url);
    for (k, v) in headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let resp = rb.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    Some(resp.bytes_stream())
}

/// 已建连的上游流。boxed 之后才能在变量之间替换。
pub type BoxBody =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<Bytes>> + Send>>;

/// `open_stream` 的 boxed 版本，便于在变量/任务之间传递。
pub async fn open_stream_boxed(
    url: &str,
    headers: &[(String, String)],
    use_proxy: bool,
) -> Option<BoxBody> {
    let s = open_stream(url, headers, use_proxy).await?;
    Some(Box::pin(s))
}

/// 预取缓冲的容量（块数）。用有界 channel 而不是无限缓冲：
/// 满了之后上游读取自然阻塞（背压），内存不会失控。
const PREFETCH_CHUNKS: usize = 512;

/// 提前把下一段流**建好连并开始读**，数据先进内存缓冲。
///
/// 为什么不能只预取「地址」：等旧流 EOF 才去建连，DNS+TCP+TLS 全落在
/// 播放器的关键路径上。实测这样空档 1799 ms —— 正好是一次「缓冲中」。
/// 提前建连并把数据先读进缓冲后，切换时数据已经在手里，空档只剩解析开销。
pub async fn prefetch_stream(
    url: String,
    headers: Vec<(String, String)>,
    use_proxy: bool,
) -> Option<BoxBody> {
    let s = open_stream_boxed(&url, &headers, use_proxy).await?;
    let (tx, rx) = tokio::sync::mpsc::channel::<reqwest::Result<Bytes>>(PREFETCH_CHUNKS);
    tokio::spawn(async move {
        let mut s = s;
        while let Some(item) = s.next().await {
            if tx.send(item).await.is_err() {
                break;
            }
        }
    });
    // 用 unfold 把 channel 包回 Stream，避免引入额外依赖
    let out = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|it| (it, rx))
    });
    Some(Box::pin(out))
}

/// 把上游 FLV 续成一条逻辑上不中断的流，逐块发给 `tx`。
///
/// 重新取地址这件事由 `crate::proxy::resolve_renew_url()` 负责（它读续流上下文）。
/// 如果没登记续流上下文，就退化成「上游断了就结束」（与改造前一致）。
pub async fn relay(
    first_body: impl futures_util::Stream<Item = reqwest::Result<Bytes>> + Unpin + Send + 'static,
    headers: Vec<(String, String)>,
    use_proxy: bool,
    tx: tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
) {
    let mut timeline = Timeline::default();
    let mut track_timeline = true;
    let mut failures: u32 = 0;

    let mut body: BoxBody = Box::pin(first_body);
    let mut demuxer = FlvDemuxer::default();
    let mut continuation = false;
    let mut waiting_keyframe = false;
    let mut started_at = Instant::now();
    let mut prefetch: Option<tokio::task::JoinHandle<Option<BoxBody>>> = None;
    // 续流空档测量：上游断开 → 新流首批数据发出的间隔。
    // 「斗鱼每 5 分钟卡一下」到底是后端接续慢、还是播放器缓冲太小，
    // 靠这个数字判断，不要凭感觉改。
    let mut gap_started: Option<Instant> = None;
    // 空档细分：解析地址 / 建连 / 首块 / 等到可播关键帧，各自花了多久。
    // 只有量出来才知道该优化哪一段，别猜。
    let mut t_resolve: Option<Instant> = None;
    let mut t_first_chunk: Option<Instant> = None;
    // 接续时选中的关键帧相对已转发位置差多少毫秒（负值 = 小幅回退）
    let mut kf_delta: i64 = 0;

    loop {
        // 快到期了就去预取下一段的**地址**（不是等 EOF —— 那时播放器已经卡了）
        if continuation || !track_timeline {
            // 只有需要续流的场景才预取
        } else if started_at.elapsed() + RENEW_LEAD >= STREAM_TTL && prefetch.is_none() {
            let h = headers.clone();
            let up = use_proxy;
            prefetch = Some(tokio::spawn(async move {
                let u = crate::proxy::resolve_renew_url().await?;
                prefetch_stream(u, h, up).await
            }));
        }

        match body.next().await {
            Some(Ok(chunk)) => {
                if !continuation {
                    // 首段：原样转发（只旁路解析一遍记时间轴）
                    if tx.send(Ok(chunk.clone())).await.is_err() {
                        return;
                    }
                    if track_timeline {
                        demuxer.push(&chunk);
                        demuxer.take_header();
                        while let Some(tag) = demuxer.next_tag() {
                            timeline.observe(&tag);
                        }
                        if demuxer.is_desynced() {
                            eprintln!("[flv-renew] 上游不是可解析的 FLV，退化为纯直通");
                            track_timeline = false;
                        }
                    }
                } else {
                    demuxer.push(&chunk);
                    demuxer.take_header();
                    let mut out = BytesMut::new();
                    while let Some(tag) = demuxer.next_tag() {
                        if waiting_keyframe {
                            // 丢掉元数据与初始化 tag（播放器已经初始化过了）
                            if tag.tag_type == TAG_TYPE_SCRIPT || tag.is_sequence_header {
                                continue;
                            }
                            // 从第一个关键帧开始接。**允许它略微靠后** ——
                            // 换 CDN 节点后新流常慢一点，严格超前要再等一个
                            // GOP（实测 1525 ms 空档）。重叠部分落在未播放
                            // 区间，MSE 覆盖掉，不会重复播。
                            let usable = tag.tag_type == TAG_TYPE_VIDEO
                                && tag.is_keyframe
                                && (timeline.is_ahead(&tag)
                                    || timeline.within_behind(&tag, KEYFRAME_TOLERANCE_MS));
                            if !usable {
                                continue;
                            }
                            kf_delta = timeline
                                .last_video_ts
                                .map(|p| tag.timestamp as i64 - p as i64)
                                .unwrap_or(0);
                            timeline.rewind_to(&tag);
                            waiting_keyframe = false;
                        } else if tag.tag_type != TAG_TYPE_SCRIPT && !timeline.is_ahead(&tag) {
                            // CDN 会回吐一个 GOP 的缓冲，重复部分直接丢
                            continue;
                        }
                        timeline.observe(&tag);
                        out.extend_from_slice(&tag.raw);
                    }
                    if !out.is_empty() {
                        if let Some(t0) = gap_started.take() {
                            let total = t0.elapsed().as_secs_f64() * 1000.0;
                            let wait_kf = t_first_chunk
                                .map(|t| t.elapsed().as_secs_f64() * 1000.0)
                                .unwrap_or(0.0);
                            eprintln!(
                                "[flv-renew] 续流空档 {total:.0} ms = 等到可播关键帧 {wait_kf:.0} ms + 其余（关键帧相对位置 {kf_delta:+} ms）"
                            );
                        }
                        if tx.send(Ok(out.freeze())).await.is_err() {
                            return;
                        }
                    }
                }
            }
            other => {
                if gap_started.is_none() {
                    gap_started = Some(Instant::now());
                    t_resolve = Some(Instant::now());
                }
                if let Some(Err(e)) = other {
                    eprintln!("[flv-renew] 上游读取错误: {e}");
                }
                let elapsed = started_at.elapsed().as_secs_f32();

                // 优先用**已经建好连、并且已经在读**的预取流：
                // 解析地址 + 建连 + 首块都不在关键路径上，空档最小。
                let mut ready: Option<BoxBody> = None;
                if let Some(h) = prefetch.take() {
                    if let Ok(Some(b)) = h.await {
                        ready = Some(b);
                    }
                }
                if let Some(b) = ready {
                    eprintln!("[flv-renew] 上一段存活 {elapsed:.1}s，已接续新流（用预取连接）");
                    failures = 0;
                    body = b;
                    demuxer = FlvDemuxer::default();
                    continuation = true;
                    waiting_keyframe = true;
                    started_at = Instant::now();
                    t_first_chunk = Some(Instant::now());
                    continue;
                }

                // 预取没成功（或没有续流上下文）→ 退回同步取地址 + 建连
                let next_url = crate::proxy::resolve_renew_url().await;
                let Some(url) = next_url else {
                    failures += 1;
                    eprintln!("[flv-renew] 续流失败 {failures}/{MAX_CONSECUTIVE_FAILURES}");
                    if failures >= MAX_CONSECUTIVE_FAILURES {
                        let _ = tx
                            .send(Err(std::io::Error::new(
                                std::io::ErrorKind::BrokenPipe,
                                "续流失败",
                            )))
                            .await;
                        return;
                    }
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                };

                match open_stream_boxed(&url, &headers, use_proxy).await {
                    Some(s) => {
                        let now = Instant::now();
                        t_first_chunk = Some(now);
                        let d_res = t_resolve
                            .map(|t| now.duration_since(t).as_secs_f64() * 1000.0)
                            .unwrap_or(0.0);
                        eprintln!(
                            "[flv-renew] 上一段存活 {elapsed:.1}s，已接续新流（同步路径：解析地址+建连 {d_res:.0} ms）"
                        );
                        failures = 0;
                        body = s;
                        demuxer = FlvDemuxer::default();
                        continuation = true;
                        waiting_keyframe = true;
                        started_at = Instant::now();
                    }
                    None => {
                        failures += 1;
                        eprintln!("[flv-renew] 新流打不开 {failures}/{MAX_CONSECUTIVE_FAILURES}");
                        if failures >= MAX_CONSECUTIVE_FAILURES {
                            let _ = tx
                                .send(Err(std::io::Error::new(
                                    std::io::ErrorKind::BrokenPipe,
                                    "续流失败",
                                )))
                                .await;
                            return;
                        }
                        tokio::time::sleep(RETRY_DELAY).await;
                    }
                }
            }
        }
    }
}
