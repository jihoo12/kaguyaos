#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode="${1:-hardware-test}"
case "$mode" in
    hardware-test) entry=boot ;;
    cpu-test) entry=cpu_test ;;
    ring3-test) entry=ring3_test ;;
    filesystem-test) entry=filesystem_test ;;
    *) echo 'Standalone build supports hardware-test, cpu-test, ring3-test or filesystem-test' >&2; exit 2 ;;
esac
if [[ -z "${JIHOO_BIN:-}" ]]; then
    compiler_package="$(nix build path:../jihoo#jihoo --no-link --print-out-paths)"
    JIHOO_BIN="$compiler_package/bin/jihoo"
fi
output="$PWD/target/jihoo-kernel/$mode"
mkdir -p "$output/esp/EFI/BOOT"
compiler="${CC:-clang}"
if [[ "$mode" == ring3-test || "$mode" == filesystem-test ]]; then
    JIHOO_BIN="$JIHOO_BIN" KAGUYA_USER_OUTPUT="$output/user" bash user/build-jihoo.sh verify
fi
python3 - "$output" "$mode" <<'PY'
from pathlib import Path
import json
import sys
output = Path(sys.argv[1])
image = output / 'user/verify.kef'
data = '.incbin ' + json.dumps(str(image)) + '\n' if sys.argv[2] in ('ring3-test', 'filesystem-test') else ''
(output / 'probe.S').write_text('''
.text
.globl jihoo_probe_data
jihoo_probe_data:
    leaq probe_start(%rip), %rax
    ret
.globl jihoo_probe_size
jihoo_probe_size:
    leaq probe_end(%rip), %rax
    leaq probe_start(%rip), %rcx
    subq %rcx, %rax
    ret
.section .rdata,"dr"
probe_start:
''' + data + 'probe_end:\n')
PY
"$compiler" -target x86_64-unknown-windows -ffreestanding -fno-pic \
    -c "$output/probe.S" -o "$output/probe.obj"
for source in arch standalone/entry standalone/cpu; do
    name="${source##*/}"
    "$compiler" -target x86_64-unknown-windows -ffreestanding -fno-pic \
        -c "src/jihoo/$source.S" -o "$output/$name.obj"
done
"$JIHOO_BIN" build -O2 -mno-red-zone -mgeneral-regs-only \
    -I src/jihoo/runtime -I src/jihoo \
    "src/jihoo/standalone/$entry.jh" "$output/arch.obj" "$output/entry.obj" "$output/cpu.obj" "$output/probe.obj" -o "$output/os.efi"
cp "$output/os.efi" "$output/esp/EFI/BOOT/BOOTX64.EFI"
printf 'Standalone jihoo EFI: %s/os.efi\n' "$output"
