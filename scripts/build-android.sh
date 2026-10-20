#!/usr/bin/env bash
# 本地构建 Android APK（Windows 宿主交叉编译到 aarch64-linux-android）
#
# ⚠️ 当前状态：**未打通**。构建能一路走到编译应用本体，最后卡在
#    OpenSSL 的 "install_dev" 阶段（见下面第 6 条）。CI 上构建是正常的。
#
#    根因在 openssl-src 的 host 分支：host 是 pc-windows-gnu 时它调
#    sanitize_sh() 把 `E:\...` 变成 `/E/...`，而 Windows 的 llvm-ranlib.exe
#    不认 `/E/` 这种绝对路径。真正的修法是装 Visual Studio 的 C++ 生成工具，
#    让 host 变成 pc-windows-msvc（那条分支用 `E:/...`，是对的）。
#
# Windows 上这套组合有几个坑，都在这处理了：
#   1. MSYS 自带的 perl 缺一堆核心模块 → 补齐到 /c/perl5lib 用 PERL5LIB 挂上
#   2. OpenSSL 要求「Unix 风格路径」的 perl → 必须用 /usr/bin/perl，不能用 Strawberry
#   3. 没装 make → 用 mingw64 的 mingw32-make 复制成 make.exe
#   4. cc crate 认的是**带横杠**的 CC_aarch64-linux-android，bash 又不许 export 带横杠
#      → 只能靠 env 传
#   5. NDK 路径若带反斜杠会被 sh 当转义吃掉 → 全部正斜杠
#   6. OpenSSL 的 Configure 会生成不带 API 级别的 --target，导致 sys/cdefs.h 报
#      「Unversioned target triples are not supported」→ 用 CFLAGS 补 --target=android24
#   7. rquickjs 的 bindgen 找不到 stdio.h → 用 BINDGEN_EXTRA_CLANG_ARGS 补 sysroot

set -u
cd "$(dirname "$0")/.." || exit 1

NDK="C:/Android/Sdk/ndk/30.0.16248370"
NDKBIN="$NDK/toolchains/llvm/prebuilt/windows-x86_64/bin"
SYSROOT="$NDK/toolchains/llvm/prebuilt/windows-x86_64/sysroot"

export PATH="/c/ProgramData/mingw64/mingw64/bin:/c/Program Files/LLVM/bin:/c/Android/Sdk/cmdline-tools/latest/bin:/c/Android/Sdk/platform-tools:$PATH"
export ANDROID_HOME="C:/Android/Sdk"
export ANDROID_SDK_ROOT="C:/Android/Sdk"
export NDK_HOME="$NDK"
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export JAVA_HOME="C:/Program Files/Eclipse Adoptium/jdk-25.0.1.8-hotspot"
export PERL5LIB=/c/perl5lib
export HTTP_PROXY=http://127.0.0.1:7897 HTTPS_PROXY=http://127.0.0.1:7897
# 关键：target 目录用**正斜杠 Windows 路径**。默认会变成 MSYS 风格的 /E/...，
# 而 Windows 的 llvm-ranlib.exe 不认 /E/ 这种绝对路径（安装 OpenSSL 时炸）。
export CARGO_TARGET_DIR="E:/hermes_app/alllive/src-tauri/target"

exec env \
  "CC_aarch64-linux-android=$NDKBIN/clang.exe --target=aarch64-linux-android24 --sysroot=$SYSROOT" \
  "CXX_aarch64-linux-android=$NDKBIN/clang++.exe --target=aarch64-linux-android24 --sysroot=$SYSROOT" \
  "AR_aarch64-linux-android=$NDKBIN/llvm-ar.exe" \
  "RANLIB_aarch64-linux-android=$NDKBIN/llvm-ranlib.exe" \
  "CFLAGS_aarch64-linux-android=--target=aarch64-linux-android24 --sysroot=$SYSROOT" \
  "BINDGEN_EXTRA_CLANG_ARGS_aarch64-linux-android=--target=aarch64-linux-android24 --sysroot=$SYSROOT" \
  npx tauri android build --apk --debug --target aarch64 "$@"
