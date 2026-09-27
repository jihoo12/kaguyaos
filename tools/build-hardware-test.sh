#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --target x86_64-unknown-uefi --features hardware-test --target-dir target/hardware-test/build
mkdir -p target/hardware-test/esp/EFI/BOOT
cp target/hardware-test/build/x86_64-unknown-uefi/debug/os.efi target/hardware-test/esp/EFI/BOOT/BOOTX64.EFI
printf '%s\n' 'Hardware-test EFI: target/hardware-test/esp/EFI/BOOT/BOOTX64.EFI' 'This script does not write to any physical disk.'
