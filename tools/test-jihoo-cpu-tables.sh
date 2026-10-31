#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-cpu-tables-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        -I src/jihoo tools/jihoo-cpu-tables-root.jh -o "$output/tables.o"
    "${CC:-cc}" -Wall -Wextra -Werror tools/jihoo-cpu-tables-test.c "$output/tables.o" -o "$output/tables-test"
    "$output/tables-test"
done
