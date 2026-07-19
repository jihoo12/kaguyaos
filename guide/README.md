# Building an OS in Rust — A Step-by-Step Guide

A hands-on guide to building a x86_64 operating system from scratch, using [kaguyaOS](https://github.com/jihoo12/kaguyaos) as a working reference. Each chapter builds on the previous one, progressing from a UEFI "hello world" to a multitasking OS with networking.

> **Who is this for?** programmers familiar with Rust (or C) who want to understand how operating systems work by building one.

---

## Table of Contents

| Ch | Title | What you learn | kaguyaOS reference |
|----|-------|---------------|-------------------|
| 1 | [Setting Up Your UEFI Dev Environment](ch1-setup.md) | Toolchain, QEMU, OVMF, first build | `Cargo.toml`, `run.sh` |
| 2 | [UEFI Hello World](ch2-uefi-hello.md) | UEFI boot protocol, `#[no_std]`, GOP framebuffer | `src/uefi.rs`, `src/main.rs` (efi_main) |
| 3 | [Booting Into Your Kernel](ch3-boot.md) | Memory map, ExitBootServices, passing data to kernel | `src/main.rs` (BootInfo) |
| 4 | [Kernel Hello World](ch4-kernel-hello.md) | Bare-metal println, framebuffer console | `src/console/mod.rs` |
| 5 | [GDT, TSS, and IDT](ch5-gdt-idt.md) | Segment descriptors, interrupt stack frames, exception handlers | `src/gdt.rs`, `src/interrupts.rs` |
| 6 | [Hardware Interrupts & Timer](ch6-interrupts.md) | 8259 PIC, 8254 PIT, IRQ routing, EOI | `src/pic.rs` |
| 7 | [Physical Memory & Paging](ch7-memory.md) | Frame allocator, 4-level page tables, identity mapping | `src/memory/mod.rs` |
| 8 | [The Kernel Heap](ch8-heap.md) | Segregated free-list allocator, `#[global_allocator]` | `src/memory/heap.rs` |
| 9 | [Ring 0/3 Isolation & Syscalls](ch9-syscalls.md) | `syscall`/`sysret`, MSR configuration, user-mode entry | `src/syscall.rs` |
| 10 | [User Programs & KEF Format](ch10-user-programs.md) | Custom executable format, loading binaries, user stacks | `src/loader.rs`, `user/` |
| 11 | [Building a Shell](ch11-shell.md) | Keyboard input, command parsing, terminal rendering | `src/console/term.rs`, `user/src/init.rs` |
| 12 | [Device Drivers](ch12-drivers.md) | PCI enumeration, NVMe, xHCI USB | `src/drivers/` |
| 13 | [FAT16 Filesystem](ch13-filesystem.md) | BPB, FAT table, directory entries, file I/O | `src/fs.rs` |
| 14 | [Networking](ch14-networking.md) | E1000 NIC, ARP, ICMP ping | `src/drivers/net/` |
| 15 | [Symmetric Multi-Processing](ch15-smp.md) | AP bring-up, INIT-SIPI-SIPI, per-CPU data | `src/processor.rs` |
| 16 | [Cooperative Scheduling](ch16-scheduling.md) | Task switching, context save/restore, timer preemption | `src/process/mod.rs` |

---

## Repository Structure (after completing all chapters)

```
kaguyaos/
├── src/
│   ├── main.rs              # Ch 2-4: UEFI entry + kernel init
│   ├── uefi.rs              # Ch 2: UEFI type bindings
│   ├── console/
│   │   ├── mod.rs           # Ch 4: Framebuffer writer + println!
│   │   └── term.rs          # Ch 11: Cell-based terminal
│   ├── sync/
│   │   └── mod.rs           # Ch 5+: Interrupt-safe Spinlock
│   ├── gdt.rs               # Ch 5: GDT/TSS per CPU
│   ├── interrupts.rs        # Ch 5-6: IDT, exception/IRQ handlers
│   ├── pic.rs               # Ch 6: 8259 PIC + 8254 PIT
│   ├── memory/
│   │   ├── mod.rs           # Ch 7: Frame allocator, page tables
│   │   └── heap.rs          # Ch 8: Kernel + user heap
│   ├── syscall.rs           # Ch 9: MSR setup + dispatch
│   ├── loader.rs            # Ch 10: KEF binary loader
│   ├── process/
│   │   └── mod.rs           # Ch 16: Scheduler + context switch
│   ├── fs.rs                # Ch 13: FAT16
│   ├── acpi.rs              # Ch 15: ACPI/RSDP/MADT parsing
│   ├── processor.rs         # Ch 15: SMP AP bring-up
│   └── drivers/
│       ├── pci.rs           # Ch 12: PCI bus enumeration
│       ├── nvme.rs          # Ch 12: NVMe block device
│       ├── xhci.rs          # Ch 12: USB 3.0 controller
│       └── net/             # Ch 14: Networking stack
├── user/
│   ├── src/
│   │   ├── init.rs          # Ch 11: Shell
│   │   ├── std.rs           # Ch 9-10: Syscall wrappers
│   │   └── ping.rs          # Ch 14: Ping utility
│   └── build.sh            # Ch 10: Build user programs
└── run.sh                   # Ch 1: Build + launch QEMU
```

---

## Prerequisites

- **Rust** (stable, with `x86_64-unknown-uefi` target)
- **QEMU** (with x86_64 UEFI/OVMF support)
- **OVMF** firmware (open-source UEFI implementation for QEMU)
- Basic understanding of Rust (ownership, unsafe, FFI)
- Curiosity about how computers boot and run

Let's start building.
