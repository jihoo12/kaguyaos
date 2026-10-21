# AGENTS.md — kaguyaOS

## Project Overview

kaguyaOS is a hobby x86_64 UEFI operating system kernel written in Rust (`#![no_std]`). It boots under QEMU with real drivers (NVMe, USB, Ethernet), a FAT16 filesystem, and a userspace shell.

## Environment Setup

```bash
# Option 1: Nix (recommended)
nix develop

# Option 2: Manual
export OVMF_BIOS="/usr/share/ovmf/OVMF.fd"
rustup target add x86_64-unknown-uefi x86_64-unknown-none
```

Required: `rustup`, `cargo`, `rust-lld`, `qemu-system-x86_64`, `OVMF firmware`.

## Build Commands

```bash
# Build kernel only (produces target/x86_64-unknown-uefi/debug/os.efi)
cargo build --target x86_64-unknown-uefi

# Build userspace programs (produces user/*.kef)
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
The build script supplies `KAGUYA_INIT_KEF` and `KAGUYA_PROGRAM_DIR`; do not combine the two hardware features.
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

- **Rust edition 2024**, `#![no_std]`, `#![no_main]`
- `#![allow(dead_code, unused_unsafe)]` — dead code and unused unsafe are permitted
- No `unsafe` blocks where avoidable, but heavy use is expected (this is an OS kernel)
- Prefer `core::arch::asm!` / `naked_asm!` for inline assembly
- Interrupt-safe spinlocks: all locks disable interrupts (`cli`) on acquire
- Each user task has its own PML4: slots 128–255 (`0x4000_0000_0000`–`0x8000_0000_0000`) are private, all other slots are copied from the kernel PML4 when the space is created, so add new kernel top-level mappings during boot only
- User pointers must be mapped with `PAGE_USER` in the current address space (`memory::is_user_range_mapped`) before kernel access; the identity map is below `0x8000_0000_0000` but kernel-only
- No external dependencies except `font8x8` (v0.3, no default features, unicode)
- Comments are sparse — code is self-documenting where possible

## Architecture Cheat Sheet

| Layer | Key Files | Notes |
|-------|-----------|-------|
| Boot | `src/main.rs:efi_main()` → `kernel_main()` | UEFI entry, init sequence, scheduler loop |
| Memory | `src/memory/{mod,heap,address_space}.rs` | Frame pool, 4-level page tables, kernel heap, per-process address spaces |
| Tasks | `src/process/mod.rs` | Cooperative round-robin, 100 Hz preemptive tick, SMP |
| Syscalls | `src/syscall.rs` | 25 syscalls via AMD64 `syscall`/`sysret` |
| Drivers | `src/drivers/{pci,nvme,xhci,keyboard}.rs`, `src/drivers/net/` | PCI enumeration, NVMe, xHCI USB, E1000, UDP and TCP client sockets (`net/udp.rs`, `net/tcp.rs`) |
| Filesystem | `src/fs.rs`, `src/file.rs`, `src/pipe.rs` | FAT16, global spinlock; per-process fd tables; blocking pipes |
| Console | `src/console/{mod,term}.rs` | VGA framebuffer, font8x8, cell-based terminal |
| Userspace | `user/src/{init,std,ls,cat,write,rm,ping,wget}.rs` | Shell, syscall wrappers, utilities |

## Key Files

- `src/main.rs` — Boot flow, kernel init, scheduler loop (619 lines)
- `src/syscall.rs` — All 25 syscall handlers
- `src/memory/heap.rs` — Kernel heap allocator
- `src/memory/address_space.rs` — Per-process page tables, `heap_grow`
- `user/linker.ld` — Links programs at `USER_CODE_BASE` (`0x4000_0000_0000`); build with `--apply-dynamic-relocs`
- `src/process/mod.rs` — Task struct, context switch, scheduler
- `src/drivers/net/mod.rs` — Network stack (ARP, ICMP, ring buffer)
- `user/src/init.rs` — Shell (PID 1)
- `user/src/std.rs` — Userspace syscall wrappers and heap allocator (libkaguya)
- `docs/syscall.md` — Syscall reference table
- `tools/kef-tool/` — Host-side FAT16 image manipulation

## Common Tasks

**Add a syscall:**
1. Add handler in `src/syscall.rs` match statement
2. Add number constant
3. Add wrapper in `user/src/std.rs`
4. Document in `docs/syscall.md`

**Add a user program:**
1. Create `user/src/name.rs` with `#![no_std]` + `#![no_main]`
2. Use `#[no_mangle] extern "C" fn _start()` entry point
3. Add to `PROGRAMS` list in `user/build.sh`
4. Run `./user/build.sh && ./user/insert.sh`

**Add a driver:**
1. Create module in `src/drivers/`
2. Implement PCI BAR mapping via `src/drivers/pci.rs`
3. Initialize in `kernel_main()` after PCI enumeration
4. For network drivers: implement `NetworkDriver` trait in `src/drivers/net/driver.rs`

## QEMU Debug

```bash
# Serial output is on stdio
# QEMU logs to qemu.log (int,cpu_reset events)
# Add -d in trace flags to run.sh for more verbose logging
```
