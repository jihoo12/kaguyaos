#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-file-data-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        src/jihoo/fat.jh -o "$output/fat.o"
    for backend in rust jihoo; do
        flags=()
        if [[ "$backend" == jihoo ]]; then flags+=(--cfg 'feature="jihoo-kernel"'); fi
        rustc --edition=2024 "${flags[@]}" tools/jihoo-file-data-test.rs \
            -C "link-arg=$output/fat.o" -o "$output/file-data-test"
        "$output/file-data-test" > "$output/$backend-O$optimization.log"
    done
    if ! cmp -s "$output/rust-O$optimization.log" "$output/jihoo-O$optimization.log"; then
        diff -u "$output/rust-O$optimization.log" "$output/jihoo-O$optimization.log"
        exit 1
    fi
    printf 'File data -O%s: Rust/jihoo results, buffers, disk contents and I/O traces match; 500 model operations passed\n' "$optimization"
done
