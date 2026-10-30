#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-heap-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        src/jihoo/heap.jh -o "$output/heap.o"
    rustc --edition=2024 tools/jihoo-heap-test.rs -C "link-arg=$output/heap.o" \
        -o "$output/heap-test"
    "$output/heap-test"
done
