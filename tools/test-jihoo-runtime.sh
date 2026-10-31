#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-runtime-test"
mkdir -p "$output"
for optimization in 0 2; do
    "$JIHOO_BIN" emit-obj --lib "-O$optimization" -mno-red-zone -mgeneral-regs-only \
        -I src/jihoo/runtime -I src/jihoo tools/jihoo-runtime-root.jh -o "$output/runtime.o"
    objdump -d "$output/runtime.o" > "$output/disassembly.txt"
    if rg -q '\bsyscall\b|%[xyz]mm[0-9]' "$output/disassembly.txt"; then
        echo 'Unexpected Linux syscall or vector register in kernel runtime object' >&2
        exit 1
    fi
    rustc --edition=2024 tools/jihoo-runtime-test.rs -C "link-arg=$output/runtime.o" \
        -o "$output/runtime-test"
    "$output/runtime-test"
done
