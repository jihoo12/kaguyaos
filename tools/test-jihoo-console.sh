#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-console-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        -I src/jihoo tools/jihoo-console-root.jh -o "$output/console.o"
    "${CC:-cc}" -Wall -Wextra -Werror tools/jihoo-console-test.c "$output/console.o" -o "$output/console-test"
    "$output/console-test"
done
