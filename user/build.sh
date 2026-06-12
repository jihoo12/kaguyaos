#!/usr/bin/env bash
set -e

# Change directory to the workspace root
cd "$(dirname "$0")/.."

echo "🔨 Installing target x86_64-unknown-none..."
rustup target add x86_64-unknown-none || true

echo "🔨 Compiling user/init.rs to user/init.kef..."
rustc --target x86_64-unknown-none \
      -C relocation-model=pic \
      -C linker-flavor=ld.lld \
      -C linker=rust-lld \
      -C link-arg=-Tuser/linker.ld \
      -C link-arg=--oformat=binary \
      -O \
      -o user/init.kef \
      user/init.rs

echo "✅ Successfully built user/init.kef!"
ls -lh user/init.kef
