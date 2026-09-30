#!/bin/sh
# Build rust-core for wasm32-unknown-unknown inside an Ubuntu 24.04 sandbox that has
# apt + crates.io access but NO rustup (static.rust-lang.org is blocked).
#
# NOT the official toolchain: this uses Ubuntu's repackaged Rust 1.85.1 and builds
# std from source with -Zbuild-std. Use it to get a real wasm to test against; do the
# release build with your own toolchain (rustup + `rustup target add wasm32-unknown-unknown`).
#
# Two workarounds, both needed (found by trying, see CC0_PHASE_7_NOTES.md):
#   1. Debian strips the `dlmalloc` dependency from the packaged std sources but the wasm
#      allocator code still uses it -> restore it in a private, writable copy of the sources.
#      Pin `=0.2.7`: 0.2.14 fails with "can't find crate for compiler_builtins".
#   2. Ubuntu's rustc has no bundled `rust-lld` -> point the wasm32 linker at apt's wasm-ld.
#
# usage: build-wasm-sandbox.sh <path-to-rust-core> [<cargo-target-dir>]
# output: <target-dir>/wasm32-unknown-unknown/release/anthroforge_core.wasm
set -eu
CORE_DIR=$(cd "${1:?usage: $0 <path-to-rust-core> [target-dir]}" && pwd)
TARGET_DIR=${2:-$CORE_DIR/target}

apt-get install -y rustc-1.85 cargo-1.85 rust-1.85-src lld >/dev/null
export PATH=/usr/lib/rust-1.85/bin:$PATH

WORK=$(mktemp -d)
cp -rL /usr/lib/rust-1.85/lib/rustlib/src/rust "$WORK/rust"
cat >> "$WORK/rust/library/std/Cargo.toml" <<'EOF'

[target.'cfg(all(target_family = "wasm", not(target_os = "emscripten")))'.dependencies]
dlmalloc = { version = "=0.2.7", features = ["rustc-dep-of-std"] }
EOF

export RUSTC_BOOTSTRAP=1                                   # allow -Z flags on a stable-channel rustc
export __CARGO_TESTS_ONLY_SRC_ROOT="$WORK/rust/library"    # must be the library/ dir itself
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER=/usr/bin/wasm-ld
export CARGO_TARGET_DIR="$TARGET_DIR"

# cd is required: rust-core/.cargo/config.toml (the getrandom backend cfg) is found relative to cwd.
cd "$CORE_DIR"
cargo build --release --lib --target wasm32-unknown-unknown -Zbuild-std=std,panic_abort

rm -rf "$WORK"
ls -l "$TARGET_DIR/wasm32-unknown-unknown/release/anthroforge_core.wasm"
