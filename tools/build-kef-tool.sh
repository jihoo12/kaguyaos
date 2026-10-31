#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
mkdir -p target/jihoo-tools
"$JIHOO_BIN" build -O2 -I src/jihoo tools/kef-tool/main.jh -o target/jihoo-tools/kef-tool
