#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source tools/jihoo-env.sh
image="${KAGUYA_DISK_IMAGE:-nvme.img}"
output="${KAGUYA_USER_OUTPUT:-$PWD/target/jihoo-user}"
for program in init ls cat write rm ping wget; do
    if [[ ! -f "$output/$program.kef" ]]; then
        echo "Missing $output/$program.kef; run ./user/build.sh first" >&2
        exit 1
    fi
done
bash tools/prepare-image.sh "$image"
for program in init ls cat write rm ping wget; do
    target/jihoo-tools/kef-tool insert "$image" "$output/$program.kef" "$program.kef"
done
printf 'All programs inserted into %s\n' "$image"
