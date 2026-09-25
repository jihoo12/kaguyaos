# kaguyaOS

A hobby operating system written in Rust for x86_64 UEFI.

kaguyaOS currently boots into userspace on an SMP system, runs a preemptive scheduler, mounts a FAT16 filesystem from NVMe, handles USB keyboard input through xHCI, and provides IRQ-driven IPv4 networking with DNS resolution. The current development milestone is a framebuffer-based graphical desktop/window with the shell rendered inside the window client area.

![Rust](https://img.shields.io/badge/language-Rust-orange)
![Platform](https://img.shields.io/badge/platform-x86__64--UEFI-blue)

---

## Current Milestones

- [x] Boot a Rust kernel through x86_64 UEFI
- [x] Ring 3 userspace and KEF executable loading
- [x] SMP startup and preemptive per-CPU scheduling
- [x] NVMe + FAT16 filesystem
- [x] xHCI USB keyboard input
- [x] E1000 interrupt-driven networking
- [x] ARP, IPv4, ICMP, UDP, and DNS A-record resolution
- [x] `ping 8.8.8.8`
- [x] `ping google.com`
- [x] Framebuffer drawing primitives and first window
- [x] Shell console constrained to the window client area
- [ ] Mouse input and cursor
- [ ] Movable windows / compositor
- [ ] TCP and HTTP (`curl`-style milestone)

---

## Features

### Kernel and scheduling

- Ring 0/3 isolation using AMD64 `syscall` / `sysret`
- 28 syscall IDs currently dispatched (`0..=27`)
- SMP startup using INIT-SIPI-SIPI
- Per-CPU data through GS-base MSRs with `swapgs` handling
- Preemptive round-robin scheduling with per-CPU run queues
- LAPIC timer preemption and scheduler wake IPIs
- Task sleep, blocking wait, zombie/reaping support, and idle work stealing
- Interrupt-safe spinlocks and scheduler synchronization
- Event-based blocking/wakeup used by network waits

### Graphics and console

- UEFI GOP framebuffer, currently exercised at 1280×800
- Shared framebuffer abstraction with clipped pixel/rectangle drawing
- 8×8 text rendering
- Primitive desktop/window frame
- Shell output rendered inside the window client area
- Viewport-aware console clearing

### Storage and userspace

- Custom KEF executable format
- Separate user address space resources and 512 KiB user heap
- NX-enabled user mappings
- NVMe controller driver using MMIO
- FAT16 filesystem with create/read/write/delete/list/format
- Userspace shell and utilities including `ls`, `cat`, `write`, `rm`, and `ping`

### USB

- xHCI USB 3.0 host controller driver
- USB keyboard through an interrupt IN endpoint

### Networking

- Intel E1000 (82540EM) driver
- PCI INTx routed through the I/O APIC
- IRQ-driven receive processing with bounded RX draining
- ARP cache and blocking ARP resolution
- Shared IPv4 transmit/routing path
- ICMP Echo Request/Reply
- UDP receive path used by DNS
- DNS A-record resolution with scheduler-based blocking timeout
- QEMU user-networking gateway/DNS routing

The networking milestone currently resolves and pings hostnames such as `google.com`. TCP, HTTP, and TLS are not implemented yet.

---

## Quick Start

### Requirements

- Rust toolchain with the `x86_64-unknown-uefi` target
- QEMU with x86_64 system emulation
- OVMF UEFI firmware
- `qemu-img`
- A shell environment capable of running the build scripts

With Nix:

```bash
nix-shell
```

Or configure the UEFI target/firmware manually:

```bash
rustup target add x86_64-unknown-uefi
export OVMF_BIOS="/usr/share/ovmf/OVMF.fd"
```

Build userspace, insert KEF programs into the disk image, and launch QEMU:

```bash
./build_insert_run.sh
```

Or run each step separately:

```bash
./user/build.sh
./user/insert.sh
./run.sh
```

The default QEMU configuration uses 2 virtual CPUs, NVMe storage, an xHCI USB keyboard, and an E1000 NIC with QEMU user-mode networking.

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
| `ping <IPv4-or-hostname>` | Send ICMP echo requests; hostnames are resolved through DNS |
| `clear` | Clear the active terminal client area |
| `shutdown` | Power off |

Examples:

```text
kaguya> ping 8.8.8.8
PING 8.8.8.8: 56 data bytes
...
10 packets transmitted, 10 received, 0% packet loss

kaguya> ping google.com
Resolving google.com...
PING <resolved IPv4>: 56 data bytes
...
```

---

## Architecture

```text
                 +----------------------+
                 |     UEFI / OVMF      |
                 +----------+-----------+
                            |
                 +----------v-----------+
                 |    kaguyaOS kernel   |
                 +----------+-----------+
                            |
       +--------------------+--------------------+
       |                    |                    |
+------v------+      +------v------+      +------v------+
| Scheduler   |      |  Drivers    |      | Graphics /  |
| SMP / Tasks |      | NVMe/xHCI   |      | Console     |
+------+------+      | E1000       |      +------+------+
       |             +------+------+             |
       |                    |                    |
+------v--------------------v--------------------v------+
|                  Ring 3 userspace                    |
|          shell / ls / cat / write / rm / ping        |
+------------------------------------------------------+
```

---

## Project Structure

```text
kaguyaos/
├── src/
│   ├── main.rs                 # Boot flow and scheduler startup
│   ├── process/
│   │   └── mod.rs              # SMP scheduler and task lifecycle
│   ├── sync/
│   │   └── mod.rs              # Interrupt-safe Spinlock
│   ├── memory/
│   │   ├── mod.rs              # Frame allocator and page tables
│   │   └── heap.rs             # Kernel/user heap allocators
│   ├── console/
│   │   ├── mod.rs              # Console writer and text viewport
│   │   ├── framebuffer.rs      # Pixel/rectangle graphics primitives
│   │   └── term.rs             # Cell-based terminal renderer
│   ├── drivers/
│   │   ├── pci.rs              # PCI enumeration and interrupt metadata
│   │   ├── nvme.rs             # NVMe driver
│   │   ├── xhci.rs             # xHCI USB driver
│   │   └── net/
│   │       ├── mod.rs           # NIC state, ARP cache, RX work
│   │       ├── driver.rs        # NetworkDriver abstraction
│   │       ├── e1000.rs         # Intel E1000 driver
│   │       ├── arp.rs           # ARP and incoming frame dispatch
│   │       ├── ipv4.rs          # Shared IPv4 transmit path
│   │       ├── dns.rs           # DNS A-record resolver
│   │       └── helper.rs        # Network checksum helpers
│   ├── loader.rs               # KEF loader
│   ├── syscall.rs              # Syscall dispatcher
│   ├── interrupts.rs           # IDT, exceptions and IRQ handling
│   ├── fs.rs                   # FAT16 filesystem
│   ├── acpi.rs                 # ACPI/MADT discovery
│   ├── processor.rs            # AP startup, LAPIC/I/O APIC, per-CPU data
│   ├── pic.rs                  # Legacy PIC/PIT support
│   ├── gdt.rs                  # GDT/TSS
│   ├── io.rs                   # Port I/O
│   └── uefi.rs                 # UEFI definitions/runtime services
├── user/
│   ├── src/init.rs             # Shell
│   ├── src/std.rs              # Userspace runtime/syscall wrappers
│   ├── src/ping.rs             # IPv4/hostname ping utility
│   └── build.sh
├── tools/kef-tool/             # KEF packaging tool
├── docs/
│   └── syscall.md              # Syscall reference
├── run.sh
└── README.md
```

---

## Development Direction

The first networking goal — resolving and successfully pinging a hostname — is complete. Current work is focused on the graphics path:

```text
framebuffer primitives
        ↓
desktop + window
        ↓
terminal inside window
        ↓
mouse cursor
        ↓
window movement / composition
```

Networking remains an active subsystem. A later milestone is to add TCP and HTTP so a userspace program can perform a `curl`-style request.

---

## Known Limitations

- Graphics currently draw directly to the GOP framebuffer; there is no compositor/backbuffer yet.
- The window is static and there is no mouse support yet.
- The E1000 path targets the QEMU 82540EM device.
- IPv4 configuration currently assumes the QEMU user-networking environment.
- DNS is a minimal single-query A-record resolver.
- TCP, HTTP, and TLS are not implemented.
- User stacks are not yet reclaimed after task exit.

---

## License

[Apache License 2.0](LICENSE)
