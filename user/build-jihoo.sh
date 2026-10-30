#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

output="${KAGUYA_USER_OUTPUT:-$PWD/target/jihoo-user}"
mkdir -p "$output"

if [[ -z "${JIHOO_BIN:-}" ]]; then
    if [[ -d ../jihoo/compiler ]]; then
        compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
        JIHOO_BIN="$compiler_package/bin/jihoo"
    else
        JIHOO_BIN=jihoo
    fi
fi
linker="${JIHOO_LD:-ld.lld}"
compiler="${CC:-cc}"
build_dir="$output/.jihoo"
mkdir -p "$build_dir"
"$compiler" -c -ffreestanding user/jihoo/runtime.S -o "$build_dir/runtime.o"

for program in "$@"; do
    printf 'Compiling user/jihoo/%s.jh -> %s/%s.kef...\n' "$program" "$output" "$program"
    "$JIHOO_BIN" emit-obj --lib -O2 -mno-red-zone -mgeneral-regs-only \
        "user/jihoo/$program.jh" -o "$build_dir/$program.o"
    "$linker" -T user/linker.ld --oformat=binary --apply-dynamic-relocs \
        -o "$output/$program.kef" "$build_dir/runtime.o" "$build_dir/$program.o"
done
