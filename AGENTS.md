# AGENTS.md — kaguyaOS

## Project Overview

kaguyaOS is a hobby x86_64 UEFI operating system kernel written in Rust (`#![no_std]`). It boots under QEMU with real drivers (NVMe, USB, Ethernet), a FAT16 filesystem, and a userspace shell.

## Environment Setup

```bash
# Option 1: Nix (recommended)
nix-shell

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

No automated test suite. Verification is manual:

1. Run `./build_insert_run.sh`
2. In QEMU shell, test commands: `ls`, `cat`, `write`, `rm`, `exec`, `ping`, `shutdown`
3. Check serial output for kernel panics or errors

## Code Style

- **Rust edition 2024**, `#![no_std]`, `#![no_main]`
- `#![allow(dead_code, unused_unsafe)]` — dead code and unused unsafe are permitted
- No `unsafe` blocks where avoidable, but heavy use is expected (this is an OS kernel)
- Prefer `core::arch::asm!` / `naked_asm!` for inline assembly
- Interrupt-safe spinlocks: all locks disable interrupts (`cli`) on acquire
- User pointers validated against address-space boundary (`< 0x8000_0000_0000`) and page-table mapped before kernel access
- No external dependencies except `font8x8` (v0.3, no default features, unicode)
- Comments are sparse — code is self-documenting where possible

## Architecture Cheat Sheet

| Layer | Key Files | Notes |
|-------|-----------|-------|
| Boot | `src/main.rs:efi_main()` → `kernel_main()` | UEFI entry, init sequence, scheduler loop |
| Memory | `src/memory/{mod,heap}.rs` | Frame allocator, 4-level page tables, segregated free-list heap |
| Tasks | `src/process/mod.rs` | Cooperative round-robin, 100 Hz preemptive tick, SMP |
| Syscalls | `src/syscall.rs` | 25 syscalls via AMD64 `syscall`/`sysret` |
| Drivers | `src/drivers/{pci,nvme,xhci}.rs`, `src/drivers/net/` | PCI enumeration, NVMe, xHCI USB, E1000 |
| Filesystem | `src/fs.rs` | FAT16, global spinlock |
| Console | `src/console/{mod,term}.rs` | VGA framebuffer, font8x8, cell-based terminal |
| Userspace | `user/src/{init,std,ls,cat,write,rm,ping}.rs` | Shell, syscall wrappers, utilities |

## Key Files

- `src/main.rs` — Boot flow, kernel init, scheduler loop (619 lines)
- `src/syscall.rs` — All 25 syscall handlers
- `src/memory/heap.rs` — Kernel + user heap allocators
- `src/process/mod.rs` — Task struct, context switch, scheduler
- `src/drivers/net/mod.rs` — Network stack (ARP, ICMP, ring buffer)
- `user/src/init.rs` — Shell (PID 1)
- `user/src/std.rs` — Userspace syscall wrappers (libkaguya)
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
