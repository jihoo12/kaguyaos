# kaguyaos - Project Summary

## Project Structure

```
└── kaguyaos/
    ├── .gitignore
    ├── Cargo.lock
    ├── Cargo.toml
    ├── LICENSE
    ├── README.md
    ├── build.log
    ├── build.sh
    ├── build_insert_run.sh
    ├── run.sh
    ├── scratch/
    │   ├── test_asm.o
    │   └── test_asm.rs
    ├── shell.nix
    ├── src/
    │   ├── acpi.rs
    │   ├── allocator.rs
    │   ├── fs.rs
    │   ├── gdt.rs
    │   ├── interrupts.rs
    │   ├── io.rs
    │   ├── kef.rs
    │   ├── main.rs
    │   ├── memory.rs
    │   ├── network/
    │   │   ├── arp.rs
    │   │   ├── driver.rs
    │   │   ├── e1000.rs
    │   │   ├── helper.rs
    │   │   ├── ipv4.rs
    │   │   └── mod.rs
    │   ├── nvme.rs
    │   ├── pci.rs
    │   ├── pic.rs
    │   ├── processor.rs
    │   ├── scheduler.rs
    │   ├── syscall.rs
    │   ├── term.rs
    │   ├── uefi.rs
    │   ├── writer.rs
    │   └── xhci.rs
    ├── tools/
    │   └── kef-tool/
    │       ├── .gitignore
    │       ├── Cargo.lock
    │       ├── Cargo.toml
    │       └── src/
    │           └── main.rs
    ├── tools.md
    └── user/
        ├── build.sh
        ├── init.kef
        ├── insert.sh
        ├── linker.ld
        ├── output
        └── src/
            ├── init.rs
            └── std.rs
```

## Documentation

### README.md

# kaguyaos

A custom Operating System written in Rust, targeting the x86_64 UEFI architecture. This project demonstrates key OS concepts including UEFI booting, graphical framebuffer, user-mode execution, system calls, multi-tasking scheduler, device driver support (NVMe & xHCI), a custom flat filesystem, and built-in JIT compilation for both C and assembly.

![Rust](https://img.shields.io/badge/language-Rust-orange)
![Platform](https://img.shields.io/badge/platform-x86__64--UEFI-blue)

---

## ✨ Features

- **UEFI Booting**: Fully compliant with the Unified Extensible Firmware Interface standard.
- **Graphical Framebuffer**: High-resolution screen rendering.
- **User Mode (Ring 3)**: Secure transition from Kernel to User mode with Ring 3 privilege isolation.
- **Multi-tasking Scheduler**: Preemptive task scheduling supporting task yielding, termination, and exit statuses.
- **USB 3.0 Support**: Custom **xHCI Driver** supporting keyboard input with cursor/arrow-key navigation.
- **NVMe Support**: Native PCI driver for generic NVMe SSDs.
- **SimpleFS FAT Filesystem**: need update content
- **need update content**: need update content
- **need update content**: need update content
- **System Calls**: Robust 20-syscall interface for user-kernel communication.

---

## 🛠️ Prerequisites

To build and run this OS, you need the following tools installed:

- **QEMU**: For system emulation (`qemu-system-x86_64`).
- **OVMF**: UEFI firmware for QEMU.

---

## 🚀 Getting Started

### 1. Build and Run

Use the provided helper script to compile the kernel, create the disk image, and launch QEMU:

```bash
nix-shell # if you use nix
export OVMF_BIOS="/usr/share/ovmf/OVMF.fd" # if you don't use nix
./user/build.sh
./user/insert.sh
./run.sh
```

This script will:
1. Build the kernel for `x86_64-unknown-uefi`.
2. Create the necessary EFI directory structure in `esp/`.
3. Create a raw 1GB NVMe disk image (`nvme.img`) if it doesn't exist.
4. build init.kef
5. insert init.kef to nvme.img
6. Launch QEMU with the OS, USB keyboard, and NVMe drive attached.

### 2. Interactive Shell Commands

need update content

---

## 💾 FAT Filesystem

need update content

---

## 🛠️ Tiny C Compiler (`cc`)

The JIT compiler allows you to write C-like source files (normally `.c` extension) and compile/run them as Ring 3 userspace processes. It parses tokens, builds function mappings, and generates raw x86_64 machine instructions.

### 📝 Language Syntax & Limitations
- **Types**: Supports `uint64_t` and `char` (both compile as 64-bit unsigned integers). No pointer syntax or structs are supported yet.
- **Parameters**: Supports functions with up to 6 parameters (mapped to register registers `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9` per the **System V AMD64 ABI**).
- **Operators**: No direct arithmetic operators (e.g. `+`, `-`). Computations must be done via inline assembly or helper functions.
- **Control Flow**: No loops or conditional statements (no `if`, `while`, `for`, etc.).
- **Inline Assembly**: Embed raw instructions via `asm("assembly")` or `__asm__("assembly")`. Statements inside the string can be separated by `;` or newlines.
- **Comments**: Comments (`//` or `/* */`) are currently not parsed.

### 🚀 Compilation and Execution Example
need update content

---

## 🔌 System Calls

need update content

### README.md has not been updated yet.

### tools.md

# Walkthrough - KEF Host Insertion Tools and Linker Script

We have implemented a complete toolchain and host tool to compile and insert KEF (Kaguya Executable Format) user space binaries directly into the custom FAT16 `nvme.img` virtual disk image from the host.

## What Was Built

### 1. Custom KEF Linker Script
- **File**: [user/linker.ld](file:///home/jihoo/kaguyaos/user/linker.ld)
- Automatically structures the output binary to have a valid 16-byte `KefHeader` at the very beginning (offset 0).
- Computes `entry_offset`, `code_offset`, and `code_size` using linker symbol arithmetic at build-time.
- Merges the `.bss` section directly into `.data` so that uninitialized global variables are correctly zero-filled inside the binary (since the kernel's KEF loader does not zero-initialize unallocated memory).

### 2. User Space Rust App & Build Script
- **App**: [user/init.rs](file:///home/jihoo/kaguyaos/user/init.rs)
  - A clean `#![no_std]`, `#![no_main]` Rust program that enters user mode, prints a banner using a wrapper around the `sys_print` syscall (Syscall 1), polls the keyboard status, yields, and shuts down QEMU.
  - Implements complete clobber registers for `asm!` calls to prevent the Rust compiler from placing variables in caller-saved registers that get modified by the kernel.
- **Build Script**: [user/build.sh](file:///home/jihoo/kaguyaos/user/build.sh)
  - Automatically installs the `x86_64-unknown-none` target if needed and compiles `init.rs` into a flat `init.kef` binary using the `linker.ld` script and `rust-lld`.

### 3. Host Disk Management Tool
- **Tool Directory**: [tools/kef-tool](file:///home/jihoo/kaguyaos/tools/kef-tool)
  - Formats, lists, and inserts files into a `nvme.img` image.
  - Mirrored the exact custom FAT16 layout parameters from [src/fs.rs](file:///home/jihoo/kaguyaos/src/fs.rs) using alignment-safe, little-endian serialization/deserialization.
  - Supports:
    - `format <img_path>`: formats the image with a new KAGFAT16 layout.
    - `list <img_path>`: prints all active files and their size/cluster info.
    - `insert <img_path> <src_path> <dest_name>`: inserts/overwrites the file.

---

## Verification & Execution Log

### 1. Building and Inserting `init.kef`
We compiled the user space application and inserted it using our tool:
```bash
$ ./user/build.sh
🔨 Installing target x86_64-unknown-none...
🔨 Compiling user/init.rs to user/init.kef...
✅ Successfully built user/init.kef!
-rwxr-xr-x 1 jihoo users 640 Jun 12 22:37 user/init.kef

$ cargo run --manifest-path tools/kef-tool/Cargo.toml -- insert nvme.img user/init.kef init.kef
Successfully inserted 'user/init.kef' into disk image as 'init.kef' (640 bytes)
```

Listing files in `nvme.img` on the host:
```bash
$ cargo run --manifest-path tools/kef-tool/Cargo.toml -- list nvme.img
Filename               Size (bytes) First Cluster
------------------------------------------------
init.kef               640          2

Total files: 1
```

### 2. Booting in QEMU
We booted `kaguyaos` in QEMU. The kernel successfully mounted the NVMe disk, located the new `init.kef`, mapped it to dynamic physical memory, and executed it in user mode (Ring 3) successfully:
```text
[SMP] AP APIC ID=1 came online
[SMP] All APs started. Online AP count: 1
Online APs: 1
Loader: Successfully loaded init.kef. Entry=0x1b5000, RSP=0x1ba000
Kernel stack base=0x62d8200 top=0x62dc200
TSS rsp0=0x62e0200
Starting scheduler loop on BSP...

=========================================
🦀 Hello from Rust User Mode (Ring 3)! 🦀
=========================================
init.kef loaded and executed successfully.
Press any key to trigger shutdown...
```
No unknown syscalls or registration crashes occurred.

