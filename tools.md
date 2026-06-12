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
