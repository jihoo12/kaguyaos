#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
bash tools/build-jihoo-kernel.sh hardware-shell
mkdir -p target/hardware-shell/esp/EFI/BOOT
cp target/jihoo-kernel/hardware-shell/os.efi target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI
printf '%s\n' 'Diskless shell EFI: target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI' 'Polled USB keyboard enabled; storage, network, timers and APs remain off.'
