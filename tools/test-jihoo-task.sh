#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-task-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        -I src/jihoo/runtime -I src/jihoo tools/jihoo-task-root.jh -o "$output/task.o"
    "${CC:-cc}" -Wall -Wextra -Werror -mgeneral-regs-only tools/jihoo-task-test.c \
        tools/jihoo-task-switch.S tools/jihoo-network-platform.c "$output/task.o" -o "$output/task-test"
    "$output/task-test"
done
