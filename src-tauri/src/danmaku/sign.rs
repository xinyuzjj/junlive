//! 抖音 X-Bogus 签名。
//!
//! 抖音 WS 的 `signature` 参数 = `sign.js` 里 `get_sign(md5(参数串))` 的结果。
//! 这个算法（byted_acrawler）是深度混淆的 JS，没法手写移植，所以把 `sign.js`
//! 内嵌进二进制，用 **QuickJS**（rquickjs）在本地跑一次算出签名。
//!
//! 为什么不用 DTV 的 deno_core：那是完整 V8，本机 GNU 工具链编不了。
//! 为什么不用 boa_engine：它的解析器跑不了 sign.js（`if (c) expr ; else {}` 会报
//! `unexpected token 'else'`），而 `node --check` 确认这个写法是合法的。
//!
//! 性能：单次约 30ms，只在**建立连接时**调用一次。
//! 注意 QuickJS 的 `Runtime` 不是 Send，所以每次现建现用、不跨 await 持有。

use rquickjs::{Context, Function, Runtime};

const SIGN_JS: &str = include_str!("sign.js");

const SIGN_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// 抖音 WS 签名要签的参数（顺序固定，逗号连接）
fn to_sign_str(room_id: &str, uid: &str) -> String {
    let vals: [(&str, &str); 13] = [
        ("live_id", "1"),
        ("aid", "6383"),
        ("version_code", "180800"),
        ("webcast_sdk_version", "1.0.14-beta.0"),
        ("room_id", room_id),
        ("sub_room_id", ""),
        ("sub_channel_id", ""),
        ("did_rule", "3"),
        ("user_unique_id", uid),
        ("device_platform", "web"),
        ("device_type", ""),
        ("ac", ""),
        ("identity", "audience"),
    ];
    vals.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn md5_hex(s: &str) -> String {
    // md5 0.7 的 API：compute 直接返回 Digest
    format!("{:x}", md5::compute(s.as_bytes()))
}

/// 算一次 X-Bogus
pub fn x_bogus(room_id: &str, uid: &str) -> Result<String, String> {
    let md5 = md5_hex(&to_sign_str(room_id, uid));
    let rt = Runtime::new().map_err(|e| format!("QuickJS Runtime 创建失败: {e}"))?;
    let ctx = Context::full(&rt).map_err(|e| format!("QuickJS Context 创建失败: {e}"))?;

    ctx.with(|ctx| {
        // sign.js 需要最简的浏览器环境（DTV 在 deno 里也是这么喂的）
        let boot = format!(
            "globalThis.window = globalThis;\
             globalThis.self = globalThis;\
             globalThis.document = {{}};\
             globalThis.navigator = {{ userAgent: \"{SIGN_UA}\" }};"
        );
        ctx.eval::<(), _>(boot.as_bytes())
            .map_err(|e| format!("环境初始化失败: {e}"))?;
        ctx.eval::<(), _>(SIGN_JS.as_bytes())
            .map_err(|e| format!("sign.js 载入失败: {e}"))?;
        let f: Function = ctx
            .globals()
            .get("get_sign")
            .map_err(|e| format!("取不到 get_sign: {e}"))?;
        let sig: String = f
            .call::<_, String>((md5,))
            .map_err(|e| format!("get_sign 调用失败: {e}"))?;
        if sig.is_empty() {
            return Err("get_sign 返回空".to_string());
        }
        Ok(sig)
    })
}
