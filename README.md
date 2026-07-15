# kaguyaos

A custom operating system written in Rust, targeting x86_64 UEFI. Demonstrates UEFI booting, graphical framebuffer, user-mode execution, system calls, cooperative multi-tasking, SMP multi-CPU support, NVMe & xHCI drivers, a custom FAT16 filesystem.

![Rust](https://img.shields.io/badge/language-Rust-orange)
![Platform](https://img.shields.io/badge/platform-x86__64--UEFI-blue)

---

## Features

- **UEFI Booting** — Full UEFI compliance: GOP framebuffer, ACPI table discovery, `ExitBootServices` handoff.
- **Graphical Framebuffer** — 1280x800 text console via `font8x8` rendering, plus a cell-based terminal API for user programs.
- **Ring 0/3 Isolation** — Full privilege separation: user-mode processes run in Ring 3 via `IRETQ`, with dedicated kernel/user stacks.
- **Cooperative Scheduler** — Round-robin task scheduling with per-CPU kernel stacks, task yield, termination, and exit status queries.
- **SMP Multi-CPU** — APIC-based startup via INIT-SIPI-SIPI, per-CPU data via GS-base MSR.
- **21 System Calls** — Fast `syscall`/`sysret` interface covering I/O, memory, tasks, filesystem, and terminal operations.
- **Custom KEF Executable Format** — 16-byte header (`KEF\0`), user code pages, 16KB user stack with guard page.
- **xHCI USB 3.0** — Full xHCI host controller driver with keyboard input (interrupt IN endpoint).
- **NVMe SSD** — PCI-based NVMe driver with MMIO BAR mapping and IO queue setup.
- **FAT16 Filesystem** — Custom implementation on NVMe: create, read, write, delete, list, and format.
- **User Heap** — Separate segregated free-list allocator (512 KiB), accessed via syscalls, no-execute mapped.
- **E1000 Networking** — Intel e1000 Ethernet driver with ARP and ICMP ping support (no TCP/UDP yet).

---

## Prerequisites

- **QEMU** (`qemu-system-x86_64`)
- **OVMF** UEFI firmware (`OVMF.fd`)
- **Rust nightly** (edition 2024)
- **`rust-src`** component (for `x86_64-unknown-none` target)
- **KEF tool** (bundled in `tools/kef-tool/`)

For Nix users: `nix-shell` sets up the environment automatically.

---

## Getting Started

### Quick Start

```bash
# Option 1: Nix
nix-shell

# Option 2: Manual
export OVMF_BIOS="/usr/share/ovmf/OVMF.fd"
rustup target add x86_64-unknown-none
```

### Build and Run

```bash
./build_insert_run.sh
```

Or step by step:

```bash
./user/build.sh      # Compile the user shell (init.kef)
./user/insert.sh     # Insert init.kef into the NVMe disk image
./run.sh             # Build kernel + launch QEMU
```

### What the Build Scripts Do

1. `user/build.sh` — Compiles `user/src/init.rs` to a flat KEF binary (`user/init.kef`, ~5.6 KB)
2. `user/insert.sh` — Packages `init.kef` into `nvme.img` via the `kef-tool`
3. `run.sh` — Builds the kernel (`cargo build --target x86_64-unknown-uefi`), copies it to `esp/EFI/BOOT/BOOTX64.EFI`, and launches QEMU with USB keyboard, NVMe drive, and E1000 NIC attached

---

## Shell Commands

The init process provides an interactive shell (`kaguya>`):

| Command | Description |
|---|---|
| `help` | Show available commands |
| `ls` | List files with names and sizes |
| `cat <file>` | Display file contents (text files only) |
| `write <file> <msg>` | Write content to a file (creates or overwrites) |
| `rm <file>` | Delete a file |
| `clear` | Clear the screen |
| `shutdown` | Power off the machine |

---

## FAT16 Filesystem

A custom FAT16 implementation running on an NVMe-backed 1 GB disk image.

| Property | Value |
|---|---|
| Sector size | 512 bytes |
| Cluster size | 4 KB (8 sectors) |
| Max files | 256 (flat root directory, no subdirectories) |
| Max filename | 21 characters |
| Max file size | ~64 MB (limited by cluster chain) |

**Supported operations**: format, create/overwrite, read, delete, list.

---

## System Calls

21 syscalls via AMD64 fast `syscall`/`sysret`:

| # | Name | Args | Description |
|---|---|---|---|
| 0 | `print` | `ptr, len` | Print UTF-8 string to console |
| 1 | `alloc` | `size, align` | Allocate from user heap |
| 2 | `free` | `ptr` | Free user heap memory |
| 3 | `add_task` | `entry, user_rsp` | Spawn a user-mode task |
| 4 | `switch_task` | — | Yield (cooperative context switch) |
| 5 | `terminate_task` | `exit_code` | Terminate current task |
| 6 | `xhci_poll` | — | Poll xHCI for USB events |
| 7 | `shutdown` | — | Power off |
| 8 | `read_key` | — | Read one key (returns `u8`) |
| 9 | `clear` | — | Clear screen |
| 10 | `realloc` | `ptr, size, align` | Reallocate user heap memory |
| 11 | `fs_format` | — | Format FAT16 filesystem |
| 12 | `fs_ls` | `buf, max_entries` | List files |
| 13 | `fs_write` | `name, name_len, data, data_len` | Write/create a file |
| 14 | `fs_read` | `name, name_len, buf, buf_len` | Read a file |
| 15 | `fs_rm` | `name, name_len` | Delete a file |
| 16 | `get_task_status` | `task_id` | Query task state (Ready/Running/Terminated) |
| 17 | `get_task_exit_code` | `task_id` | Get terminated task's exit code |
| 18 | `run_ap_scheduler` | — | Enter AP scheduler loop (no return) |
| 19 | `write_cell` | `row, col, char, fg, bg` | Write a single terminal cell |
| 20 | `write_region` | `row, col, ptr, len, width` | Write a batch of terminal cells |

All pointer arguments are validated against user address space limits and page table mappings before kernel access.

---

## Architecture

### Boot Sequence

1. UEFI loads `BOOTX64.EFI` → framebuffer, ACPI RSDP, memory map obtained
2. `ExitBootServices` → kernel takes full control
3. GDT, IDT, 4-level paging, PIC, interrupts initialized
4. MSR-based fast syscall interface configured
5. PCI enumeration → NVMe, xHCI, E1000 drivers initialized
5. Kernel heap (512 KB) + user heap (512 KB) allocated
6. SMP: Application Processors started via INIT-SIPI-SIPI
7. `init.kef` loaded from FAT16, mapped into user address space
8. BSP enters scheduler loop

### Memory Layout

| Region | Address Range | Size | Notes |
|---|---|---|---|
| Kernel code/data | UEFI-assigned | variable | Identity-mapped |
| Kernel heap | `0x132000` | 512 KB | Segregated free-list allocator |
| User code | `0x238000`+ | loaded from KEF | Ring 3, PAGE_USER |
| User stack | `0x23A000`–`0x23EFFF` | 16 KB | Ring 3, 1 guard page below |
| User heap | `0x7000_0000_0000` | 512 KB | PAGE_USER, PAGE_NO_EXECUTE |
| Framebuffer | `0x80000000` | varies | Identity-mapped |

### KEF Executable Format

16-byte header followed by flat code:

```
Offset  Field
0x00    Magic: "KEF\0" (4 bytes)
0x04    Entry offset (4 bytes, LE)
0x08    Code offset (4 bytes, LE)
0x0C    Code size (4 bytes, LE)
```

---

## Project Structure

```
kaguyaos/
├── src/                  # Kernel source
│   ├── main.rs           # Boot flow, initialization
│   ├── memory.rs         # Physical/virtual memory, page tables
│   ├── allocator.rs      # Kernel heap (segregated free-list)
│   ├── syscall.rs        # Syscall dispatcher + handlers
│   ├── scheduler.rs      # Cooperative round-robin scheduler
│   ├── interrupts.rs     # IDT, exception handlers, ISR stubs
│   ├── kef.rs            # KEF binary loader
│   ├── fs.rs             # FAT16 filesystem
│   ├── nvme.rs           # NVMe driver
│   ├── xhci.rs           # xHCI USB host controller driver
│   ├── network/          # E1000 NIC, ARP, ICMP
│   └── ...
├── user/                 # Userspace
│   ├── src/init.rs       # Interactive shell
│   ├── src/std.rs        # User-space syscall wrappers
│   └── build.sh          # Compiles to flat KEF binary
├── tools/kef-tool/       # Host tool to package KEF binaries
├── run.sh                # Build + launch QEMU
└── build_insert_run.sh   # Full pipeline: build user → insert → run
```

---

## License

[LICENSE](LICENSE)
