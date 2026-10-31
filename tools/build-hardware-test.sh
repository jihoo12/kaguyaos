#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
bash tools/build-jihoo-kernel.sh hardware-test
mkdir -p target/hardware-test/esp/EFI/BOOT
cp target/jihoo-kernel/hardware-test/os.efi target/hardware-test/esp/EFI/BOOT/BOOTX64.EFI
printf '%s\n' 'Hardware-test EFI: target/hardware-test/esp/EFI/BOOT/BOOTX64.EFI' 'This script does not write to any physical disk.'
