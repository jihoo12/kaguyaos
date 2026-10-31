#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-pipe-owner-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        -I src/jihoo/runtime -I src/jihoo tools/jihoo-pipe-owner-root.jh -o "$output/pipe.o"
    "${CC:-cc}" -pthread -Wall -Wextra -Werror tools/jihoo-pipe-owner-test.c "$output/pipe.o" -o "$output/pipe-test"
    "$output/pipe-test"
done
