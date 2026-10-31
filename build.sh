#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
source tools/jihoo-env.sh
bash tools/build-jihoo-kernel.sh nvme-shell
mkdir -p target/kaguya esp/EFI/BOOT
cp target/jihoo-kernel/nvme-shell/os.efi target/kaguya/os.efi
cp target/kaguya/os.efi esp/EFI/BOOT/BOOTX64.EFI
printf '%s\n' 'Kernel EFI: target/kaguya/os.efi'
