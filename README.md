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
├── src/                  # Kernel
│   ├── main.rs           # Boot flow
│   ├── syscall.rs        # Syscall dispatcher + handlers
│   ├── scheduler.rs      # Cooperative scheduler
│   ├── interrupts.rs     # IDT, exceptions
│   ├── kef.rs            # KEF binary loader
│   ├── fs.rs             # FAT16 filesystem
│   ├── nvme.rs           # NVMe driver
│   ├── xhci.rs           # xHCI USB driver
│   └── network/          # E1000, ARP, ICMP
├── user/                 # Userspace
│   ├── src/init.rs       # Shell
│   ├── src/std.rs        # Syscall wrappers
│   ├── src/ping.rs       # Ping utility
│   └── build.sh          # Compiles to flat KEF binaries
├── tools/kef-tool/       # Host tool to package KEF binaries
├── run.sh                # Build + launch QEMU
└── docs/                 # Documentation
    └── syscall.md        # System call reference
```

---

## License

[Apache License 2.0](LICENSE)
