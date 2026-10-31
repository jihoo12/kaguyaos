# AGENTS.md — kaguyaOS

## Project Overview

kaguyaOS is a hobby x86_64 UEFI operating system kernel written in Jihoo with assembly ABI boundaries. It boots under QEMU with real drivers (NVMe, USB, Ethernet), a FAT16 filesystem, and a userspace shell.

## Environment Setup

```bash
# Option 1: Nix (recommended)
nix develop

# Option 2: Manual
export OVMF_BIOS="/usr/share/ovmf/OVMF.fd"
export JIHOO_BIN="/path/to/jihoo"
```

Required: Jihoo (adjacent checkout `../jihoo` or `JIHOO_BIN`), Clang, LLVM `ld.lld`, Python 3, `qemu-system-x86_64`, and OVMF. The supported build must not invoke Rust tools.

## Build Commands

```bash
# Build kernel and user KEFs (produces target/kaguya/os.efi)
./build.sh

# Build userspace programs (produces target/jihoo-user/*.kef)
./user/build.sh

# Insert user programs into nvme.img
./user/insert.sh

# Build everything + launch QEMU
./build_insert_run.sh

# Build kernel + launch QEMU (without rebuilding userspace)
./run.sh
```

## Testing

Graphics smoke checks (inside `nix develop`, after building the kernel and preparing `nvme.img`):

```bash
python3 tools/gpu-smoke.py
python3 tools/gpu-smoke.py --backend gop --output /tmp/kaguya-gop-smoke
```

These boot disposable QEMU snapshots, exercise shell input and capture screenshots/serial logs.
Other verification is manual:

1. Run `./build_insert_run.sh`
2. In QEMU shell, test commands: `ls`, `cat`, `write`, `rm`, `exec`, `ping`, `shutdown`, plus pipes and redirection (`ls | cat`, `ls > f`, `cat < f`, `>>`) and `wget example.com > page.html`
3. Check serial output for kernel panics or errors

First physical-hardware milestone uses a separate diagnostic build:

```bash
./tools/build-hardware-test.sh
python3 tools/gpu-smoke.py --hardware-test --output /tmp/kaguya-hardware-test
```

Use only `target/hardware-test/esp/EFI/BOOT/BOOTX64.EFI` for this milestone.
It displays GOP/memory-map diagnostics and halts before device initialization.
The next milestone is a diskless ring-3 shell with a polled USB boot keyboard:

```bash
./tools/build-hardware-shell.sh
python3 tools/gpu-smoke.py --hardware-shell --resolution 2560x1440 --output /tmp/kaguya-hardware-shell-1440p
```

Its separate artifact is `target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI`.
The build script embeds the Jihoo shell and four file utilities; hardware-test and hardware-shell are separate images.
Connect a boot-protocol USB keyboard directly before boot; hubs/hotplug are unsupported.
The shell has an 8 MiB temporary RAM filesystem with embedded ls/cat/write/rm.
Only this new RAM filesystem is initialized automatically; no physical disk is formatted.
Only the selected xHCI controller is enabled; storage, network, timers and APs remain off.
Select another controller with `KAGUYA_XHCI_INDEX=1 ./tools/build-hardware-shell.sh`.
Additional checks: `tools/gpu-smoke.py --hardware-shell --usb-mouse --keyboard-stress`
and `tools/gpu-smoke.py --hardware-shell --no-keyboard` (run with python3).
Use `python3 tools/gpu-smoke.py --hardware-shell --shutdown` to verify guest power-off.

Never restore automatic formatting of unrecognized disks during boot.

## Code Style

- Normal source: Jihoo under `src/jihoo/` and `user/jihoo/`; keep README.md in English.
- Rust sources and harnesses are optional comparison references (edition 2024, `no_std`).
- `#![allow(dead_code, unused_unsafe)]` — dead code and unused unsafe are permitted
- No `unsafe` blocks where avoidable, but heavy use is expected (this is an OS kernel)
- Prefer `core::arch::asm!` / `naked_asm!` for inline assembly
- Interrupt-safe spinlocks: all locks disable interrupts (`cli`) on acquire
- Each user task has its own PML4: slots 128–255 (`0x4000_0000_0000`–`0x8000_0000_0000`) are private, all other slots are copied from the kernel PML4 when the space is created, so add new kernel top-level mappings during boot only
- User pointers must be mapped with `PAGE_USER` in the current address space (`memory::is_user_range_mapped`) before kernel access; the identity map is below `0x8000_0000_0000` but kernel-only
- Normal builds use the Jihoo standard library with the kernel adapter and the embedded font; no Cargo/font-crate dependency.
- Comments are sparse — code is self-documenting where possible

## Architecture Cheat Sheet

| Layer | Files under `src/jihoo/` | Notes |
|-------|-------------------------|-------|
| Boot | `standalone/platform.jh` | UEFI entry and device init; mode 6 is ordinary NVMe, mode 7 is polled hardware shell |
| Memory | `heap.jh`, `frame.jh`, `paging.jh`, `space.jh`, `standalone/memory.jh` | Kernel heap/frame pool and owned per-process page tables |
| CPUs | `acpi.jh`, `standalone/{cpu,processor,smp}.jh`, `standalone/ap.S` | GDT/TSS/IDT, GS state, LAPIC/IOAPIC, AP startup |
| Tasks | `task.jh`, `standalone/task.S` | Per-CPU queues, cooperative/preemptive scheduling, retirement acknowledgement |
| Syscalls | `standalone/{ring3,scheduler}.jh`, `standalone/entry.S` | IDs 0..41 via syscall/sysret |
| Drivers | `pci.jh`, `nvme.jh`, `xhci.jh`, `e1000.jh`, `virtio_gpu.jh` | PCI devices and DMA |
| Network | `net_{base,wire,udp,dns,tcp}.jh` | IRQ receive, IPv4, ICMP, UDP/DNS and client TCP |
| Files | `filesystem.jh`, `fat.jh`, `file_table.jh`, `pipe_owner.jh` | KAGFAT16, descriptor/shared ownership, blocking pipes |
| Console | `console.jh`, `font/basic.jh`, `display.jh` | GOP/virtio desktop and text |
| Userspace | `user/jihoo/` | Seven programs, syscall wrappers, `runtime.S` |
| Host tool | `tools/kef-tool/main.jh` | Jihoo image manipulation, excluded Rust reference in same directory |

## Common Tasks

**Add a syscall:** update `src/jihoo/standalone/scheduler.jh`, add a wrapper in `user/jihoo/kaguya.jh`, and document it in `docs/syscall.md`.

**Add a program:** create `user/jihoo/name.jh` with `#![freestanding]` and exported `kaguya_main`, then add it to the build/insertion lists. Programs are linked by `user/linker.ld` at `0x4000_0000_0000` with `--apply-dynamic-relocs`.

**Validate changes:** use relevant native C/Jihoo harnesses under `tools/test-jihoo-*.sh`, `tools/check-rust-free-build.sh`, and disposable QEMU snapshots. Older Rust differential harnesses are optional references. Keep diagnostic/hardware shell restrictions intact; new kernel top-level mappings must exist before user spaces are created.

## QEMU Debug

```bash
# Serial output is on stdio
# QEMU logs to qemu.log (int,cpu_reset events)
# Add -d in trace flags to run.sh for more verbose logging
```
