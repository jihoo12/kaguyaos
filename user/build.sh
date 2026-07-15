#!/usr/bin/env bash
set -e

# Change directory to the workspace root
cd "$(dirname "$0")/.."

echo "Installing target x86_64-unknown-none..."
rustup target add x86_64-unknown-none || true

RUSTC_FLAGS="--target x86_64-unknown-none \
      -C linker-flavor=ld.lld \
      -C linker=rust-lld \
      -C link-arg=-Tuser/linker.ld \
      -C link-arg=--oformat=binary \
      -O"

PROGRAMS="init ls cat rm write"

for prog in $PROGRAMS; do
    echo "Compiling user/src/${prog}.rs -> user/${prog}.kef..."
    rustc $RUSTC_FLAGS \
          -o "user/${prog}.kef" \
          "user/src/${prog}.rs"
    echo "  Built user/${prog}.kef"
done

echo ""
echo "All user programs built:"
ls -lh user/*.kef
