#!/bin/bash
# 一键构建 Rust 静态库并重新生成 Swift 绑定；重复执行会覆盖同名产物。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$APP_DIR/../.." && pwd)"
PROFILE="${1:-debug}"
HOST_TARGET="${HOST_TARGET:-aarch64-apple-darwin}"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

if [[ ! -x "$CARGO" ]]; then
    CARGO="$(command -v cargo)"
fi

case "$PROFILE" in
    debug)
        CARGO_PROFILE="dev"
        ;;
    release)
        CARGO_PROFILE="release"
        ;;
    *)
        echo "用法：$0 [debug|release]" >&2
        exit 2
        ;;
esac

GENERATED_DIR="$APP_DIR/Sources/EventKyuCore/Generated"
FFI_DIR="$APP_DIR/Sources/EventKyuCoreFFI"
LIB_DIR="$ROOT_DIR/target/$HOST_TARGET/$PROFILE"
mkdir -p "$GENERATED_DIR" "$FFI_DIR"
# 只清理 UniFFI 产物，避免模块改名后残留旧绑定造成重复定义。
rm -f "$GENERATED_DIR"/*.swift "$GENERATED_DIR"/*.h "$GENERATED_DIR"/*.modulemap

"$CARGO" build --locked -p eventkyu-uniffi \
    --target "$HOST_TARGET" --profile "$CARGO_PROFILE"

# 直接使用 crate 内置的 UniFFI CLI，无需全局安装 uniffi-bindgen。
"$CARGO" run --locked -p eventkyu-uniffi \
    --features cli --bin uniffi-bindgen -- \
    generate --language swift \
    --out-dir "$GENERATED_DIR" \
    "$LIB_DIR/libeventkyu_uniffi.dylib"

cp "$LIB_DIR/libeventkyu_uniffi.a" "$FFI_DIR/libeventkyu_uniffi.a"
cp "$GENERATED_DIR/EventKyuCoreFFI.h" "$FFI_DIR/EventKyuCoreFFI.h"
cp "$GENERATED_DIR/EventKyuCoreFFI.modulemap" "$FFI_DIR/module.modulemap"

echo "EventKyu Core（$PROFILE / $HOST_TARGET）已生成："
echo "  Swift：$GENERATED_DIR/EventKyuCore.swift"
echo "  静态库：$FFI_DIR/libeventkyu_uniffi.a"
