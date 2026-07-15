#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

PROGRAMS="init ls cat rm write"

for prog in $PROGRAMS; do
    if [ -f "user/${prog}.kef" ]; then
        cargo run --manifest-path tools/kef-tool/Cargo.toml -- insert nvme.img "user/${prog}.kef" "${prog}.kef"
    fi
done

echo "All programs inserted into nvme.img"
