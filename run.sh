#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
source tools/jihoo-env.sh
bash build.sh
image="${KAGUYA_DISK_IMAGE:-nvme.img}"
if [[ ! -e "$image" ]]; then
    KAGUYA_USER_OUTPUT="$PWD/target/jihoo-kernel/nvme-shell/user" bash user/insert.sh
fi

gpu_args=(-vga none -device virtio-vga)
if [[ "${GPU:-virtio}" == gop ]]; then
    gpu_args=(-vga std)
fi
qemu-system-x86_64 \
    -smp "${KAGUYA_CPUS:-2}" \
    -m 256 \
    "${gpu_args[@]}" \
    -bios "${OVMF_BIOS}" \
    -drive format=raw,file=fat:rw:esp \
    -drive "file=$image,if=none,id=nvm,format=raw" \
    -device nvme,serial=deadbeef,drive=nvm \
    -device qemu-xhci,id=xhci,msi=off,msix=off \
    -device usb-kbd,bus=xhci.0 \
    -device e1000,netdev=net0 \
    -netdev user,id=net0,hostfwd=udp::5555-:5555 \
    -serial stdio \
    -d int,cpu_reset \
    -no-reboot \
    -D qemu.log \
    "$@"
