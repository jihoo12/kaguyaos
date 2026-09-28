#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target/hardware-shell/esp/EFI/BOOT
for program in init ls cat write rm; do
    rustc --target x86_64-unknown-none -C linker-flavor=ld.lld -C linker=rust-lld \
        -C link-arg=-Tuser/linker.ld -C link-arg=--oformat=binary -O \
        -o "target/hardware-shell/$program.kef" "user/src/$program.rs"
done
export KAGUYA_PROGRAM_DIR="$PWD/target/hardware-shell"
export KAGUYA_XHCI_INDEX="${KAGUYA_XHCI_INDEX:-0}"
if [[ ! "$KAGUYA_XHCI_INDEX" =~ ^[0-9]+$ ]]; then
    echo 'KAGUYA_XHCI_INDEX must be a nonnegative integer' >&2
    exit 1
fi
export KAGUYA_INIT_KEF="$PWD/target/hardware-shell/init.kef"
cargo build --target x86_64-unknown-uefi --features hardware-shell --target-dir target/hardware-shell/build
cp target/hardware-shell/build/x86_64-unknown-uefi/debug/os.efi target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI
printf '%s\n' 'Diskless shell EFI: target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI' 'Polled USB boot keyboard enabled. This script does not write to physical disks.'
