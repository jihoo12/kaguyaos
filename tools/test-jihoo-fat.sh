#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-fat-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        src/jihoo/fat.jh -o "$output/fat.o"
    for backend in rust jihoo; do
        flags=()
        if [[ "$backend" == jihoo ]]; then flags+=(--cfg 'feature="jihoo-kernel"'); fi
        rustc --edition=2024 --cfg 'feature="hardware-shell"' "${flags[@]}" \
            tools/jihoo-fat-test.rs -C "link-arg=$output/fat.o" -o "$output/fat-test"
        "$output/fat-test"
    done
done
