/// 本插件没有需要在 JS 侧暴露的 Rust 命令（命令全在 Kotlin 侧），
/// 这里只为把 android/ 目录告诉 tauri 的构建流程。
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
