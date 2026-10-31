#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-file-table-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        -I src/jihoo/runtime -I src/jihoo tools/jihoo-file-table-root.jh -o "$output/table.o"
    "${CC:-cc}" -pthread -Wall -Wextra -Werror tools/jihoo-file-table-test.c tools/jihoo-network-platform.c "$output/table.o" -o "$output/table-test"
    "$output/table-test"
done
