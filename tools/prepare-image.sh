#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
image="${1:-${KAGUYA_DISK_IMAGE:-nvme.img}}"
source tools/jihoo-env.sh
bash tools/build-kef-tool.sh
if [[ -e "$image" || -L "$image" ]]; then
    if [[ ! -f "$image" ]]; then echo "Refusing non-file image: $image" >&2; exit 1; fi
    target/jihoo-tools/kef-tool check "$image"
else
    # Only this newly created host file is initialized. Existing images must pass check.
    python3 - "$image" <<'PY'
import sys
with open(sys.argv[1], 'xb') as image:
    image.truncate(1024 * 1024 * 1024)
PY
    target/jihoo-tools/kef-tool format "$image"
fi
