#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-shared-test"
mkdir -p "$output"
if "$JIHOO_BIN" check --lib -I src/jihoo tools/jihoo-guard-escape.jh >"$output/escape.log" 2>&1; then
    echo 'A guard escaped its local lock' >&2
    exit 1
fi
rg -q 'goes away here while it is still borrowed' "$output/escape.log"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        -I src/jihoo/runtime -I src/jihoo tools/jihoo-shared-root.jh -o "$output/shared.o"
    "${CC:-cc}" -pthread -Wall -Wextra -Werror tools/jihoo-shared-test.c "$output/shared.o" -o "$output/shared-test"
    "$output/shared-test"
done
