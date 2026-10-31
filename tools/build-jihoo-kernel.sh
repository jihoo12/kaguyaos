#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode="${1:-hardware-test}"
case "$mode" in
    hardware-test) entry=boot ;;
    cpu-test) entry=cpu_test ;;
    *) echo 'Standalone build supports hardware-test or cpu-test' >&2; exit 2 ;;
esac
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-kernel/$mode"
mkdir -p "$output/esp/EFI/BOOT"
compiler="${CC:-clang}"
for source in arch standalone/entry standalone/cpu; do
    name="${source##*/}"
    "$compiler" -target x86_64-unknown-windows -ffreestanding -fno-pic \
        -c "src/jihoo/$source.S" -o "$output/$name.obj"
done
"$JIHOO_BIN" build -O2 -mno-red-zone -mgeneral-regs-only \
    -I src/jihoo/runtime -I src/jihoo \
    "src/jihoo/standalone/$entry.jh" "$output/arch.obj" "$output/entry.obj" "$output/cpu.obj" -o "$output/os.efi"
cp "$output/os.efi" "$output/esp/EFI/BOOT/BOOTX64.EFI"
printf 'Standalone jihoo EFI: %s/os.efi\n' "$output"
