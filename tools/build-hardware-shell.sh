#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target/hardware-shell/esp/EFI/BOOT
rustc --target x86_64-unknown-none -C linker-flavor=ld.lld -C linker=rust-lld \
    -C link-arg=-Tuser/linker.ld -C link-arg=--oformat=binary -O \
    -o target/hardware-shell/init.kef user/src/init.rs
export KAGUYA_INIT_KEF="$PWD/target/hardware-shell/init.kef"
cargo build --target x86_64-unknown-uefi --features hardware-shell --target-dir target/hardware-shell/build
cp target/hardware-shell/build/x86_64-unknown-uefi/debug/os.efi target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI
printf '%s\n' 'Diskless shell EFI: target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI' 'Keyboard input is not enabled yet. This script does not write to physical disks.'
