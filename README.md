# kaguyaOS

A hobby OS written in Rust, targeting x86_64 UEFI.

![Rust](https://img.shields.io/badge/language-Rust-orange)
![Platform](https://img.shields.io/badge/platform-x86__64--UEFI-blue)

---

## Features

- UEFI boot with GOP framebuffer (1280×800 text console)
- Ring 0/3 isolation with `syscall`/`sysret` and 25 system calls
- Cooperative round-robin scheduler with per-CPU kernel stacks
- SMP via INIT-SIPI-SIPI with per-CPU data (GS-base MSR)
- Custom KEF executable format (16-byte header, user code pages, 16 KB stack)
- xHCI USB 3.0 driver (keyboard input via interrupt IN endpoint)
- NVMe SSD driver with MMIO BAR mapping
- FAT16 filesystem (create, read, write, delete, list, format)
- E1000 Ethernet driver with ARP and ICMP ping
- User heap (512 KB, separate from kernel, no-execute mapped)

---

## Quick Start

```bash
# Nix
nix-shell

# Manual
export OVMF_BIOS="/usr/share/ovmf/OVMF.fd"
rustup target add x86_64-unknown-none
```

```bash
./build_insert_run.sh    # Build everything and launch QEMU
```

Or step by step:

```bash
./user/build.sh          # Compile user programs → *.kef
./user/insert.sh         # Insert programs into nvme.img
./run.sh                 # Build kernel + launch QEMU
```

---

## Shell Commands

| Command | Description |
|---|---|
| `help` | Show available commands |
| `ls` | List files |
| `cat <file>` | Display file contents |
| `write <file> <msg>` | Write content to a file |
| `rm <file>` | Delete a file |
| `exec <file> [args]` | Execute a KEF binary |
| `clear` | Clear screen |
| `shutdown` | Power off |

---

## Project Structure

```
kaguyaos/
├── src/                    # Kernel
│   ├── main.rs             # Boot flow & scheduler loop
│   ├── sync/               # Synchronization primitives
│   │   └── mod.rs          # Interrupt-safe Spinlock
│   ├── memory/             # Memory management
│   │   ├── mod.rs          # Frame allocator, page tables
│   │   └── heap.rs         # Kernel + user heap allocators
│   ├── process/            # Task management
│   │   └── mod.rs          # Cooperative scheduler
│   ├── console/            # Display & terminal
│   │   ├── mod.rs          # VGA framebuffer writer, println!
│   │   └── term.rs         # Cell-based terminal renderer
│   ├── drivers/            # Device drivers
│   │   ├── pci.rs          # PCI bus enumeration
│   │   ├── nvme.rs         # NVMe SSD driver
│   │   ├── xhci.rs         # xHCI USB 3.0 driver
│   │   └── net/            # Networking
│   │       ├── mod.rs      # ICMP ring buffer, ARP cache
│   │       ├── arp.rs      # ARP request/reply handling
│   │       ├── e1000.rs    # E1000 Ethernet driver
│   │       ├── driver.rs   # NetworkDriver trait
│   │       ├── ipv4.rs     # IPv4 + ICMP packet types
│   │       └── helper.rs   # Checksum utilities
│   ├── loader.rs           # KEF binary loader
│   ├── syscall.rs          # Syscall dispatcher + handlers
│   ├── interrupts.rs       # IDT, exceptions, IRQ routing
│   ├── fs.rs               # FAT16 filesystem
│   ├── acpi.rs             # ACPI table parsing (MADT, etc.)
│   ├── processor.rs        # SMP AP startup, per-CPU data
│   ├── pic.rs              # 8259 PIC + PIT timer
│   ├── gdt.rs              # GDT/TSS per CPU
│   ├── io.rs               # Port I/O helpers
│   └── uefi.rs             # UEFI types & runtime services
├── user/                   # Userspace
│   ├── src/init.rs         # Shell
│   ├── src/std.rs          # Syscall wrappers
│   ├── src/ping.rs         # Ping utility
│   └── build.sh            # Compiles to flat KEF binaries
├── tools/kef-tool/         # Host tool to package KEF binaries
├── docs/
│   └── syscall.md          # System call reference (25 syscalls)
├── run.sh                  # Build + launch QEMU
└── README.md
```

---

## License

[Apache License 2.0](LICENSE)
