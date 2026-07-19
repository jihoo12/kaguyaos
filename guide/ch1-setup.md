# Chapter 1: Setting Up Your UEFI Development Environment

Before writing any kernel code, you need a toolchain that can produce UEFI binaries and an emulator to test them.

## What You'll Need

| Tool | Purpose |
|------|---------|
| Rust | Language toolchain (no_std support) |
| `rustup target add x86_64-unknown-uefi` | Cross-compilation target for UEFI PE binaries |
| QEMU | x86_64 hardware emulator with UEFI support |
| OVMF | Open-source UEFI firmware for QEMU |

## Step 1: Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then add the UEFI target:

```bash
rustup target add x86_64-unknown-uefi
```

Verify:

```bash
rustup target list --installed
# Should show: x86_64-unknown-uefi
```

## Step 2: Install QEMU

On NixOS/Nix:
```bash
nix-shell -p qemu
```

On Ubuntu/Debian:
```bash
sudo apt install qemu-system-x86
```

On Arch:
```bash
sudo pacman -S qemu-full
```

## Step 3: Get OVMF Firmware

OVMF is the UEFI firmware QEMU uses to boot. You need the combined firmware image.

On NixOS/Nix:
```bash
# The path will be something like:
# /nix/store/...-OVMF-<version>-fd/FV/OVMF.fd
find /nix/store -name "OVMF.fd" 2>/dev/null
```

On Ubuntu/Debian:
```bash
sudo apt install ovmf
# Usually at: /usr/share/OVMF/OVMF.fd
```

On Arch:
```bash
sudo pacman -S edk2-ovmf
# Usually at: /usr/share/edk2/ovmf/OVMF.fd
```

Set an environment variable so scripts can find it:

```bash
export OVMF_BIOS="/path/to/OVMF.fd"
```

## Step 4: Create the Project

```bash
cargo init --name my_os --edition 2024 my_os
cd my_os
```

Set up `Cargo.toml` for a no_std kernel:

```toml
[package]
name = "my_os"
version = "0.1.0"
edition = "2024"

[profile.dev]
panic = "abort"

[profile.release]
panic = "abort"
```

The key settings:
- **`panic = "abort"`** — no stack unwinding in a kernel; panics should halt

Add the minimal crate-level attributes in `src/main.rs`:

```rust
#![no_std]
#![no_main]
```

- **`#![no_std]`** — don't link the Rust standard library (it needs an OS)
- **`#![no_main]`** — we provide our own entry point, not `fn main()`

## Step 5: Build and Test

```bash
cargo build --target x86_64-unknown-uefi
```

This produces `target/x86_64-unknown-uefi/debug/my_os.efi` — a PE executable that UEFI firmware can load.

## Step 6: Create an ESP Directory

UEFI firmware boots from an EFI System Partition (ESP). For QEMU, we can use a directory:

```bash
mkdir -p esp/EFI/BOOT
cp target/x86_64-unknown-uefi/debug/my_os.efi esp/EFI/BOOT/BOOTX64.EFI
```

The firmware looks for `EFI/BOOT/BOOTX64.EFI` on every FAT-formatted partition.

## Step 7: Launch QEMU

```bash
qemu-system-x86_64 \
    -bios $OVMF_BIOS \
    -drive format=raw,file=fat:rw:esp \
    -serial stdio \
    -display none
```

| Flag | Meaning |
|------|---------|
| `-bios $OVMF_BIOS` | Use OVMF as UEFI firmware |
| `-drive format=raw,file=fat:rw:esp` | Mount the `esp/` directory as a FAT drive |
| `-serial stdio` | Redirect serial port to terminal |
| `-display none` | No graphical window (output goes to serial) |

At this point, QEMU will boot, OVMF will find your EFI binary, and... nothing visible will happen. That's expected — your program has no code yet!

## What kaguyaOS Does Differently

kaguyaOS's `run.sh` does exactly the above, but also adds hardware devices:

```bash
qemu-system-x86_64 \
    -smp 2 \
    -bios $OVMF_BIOS \
    -drive format=raw,file=fat:rw:esp \
    -drive file=nvme.img,if=none,id=nvm,format=raw \
    -device nvme,serial=deadbeef,drive=nvm \
    -device qemu-xhci,id=xhci,msi=off,msix=off \
    -device usb-kbd,bus=xhci.0 \
    -device e1000,netdev=net0 \
    -netdev user,id=net0,hostfwd=udp::5555-:5555 \
    -serial stdio \
    -d int,cpu_reset -no-reboot -D qemu.log
```

The NVMe drive, USB keyboard, and Ethernet card are for later chapters. For now, just the ESP drive and serial port are enough.

---

**Next:** [Chapter 2 — UEFI Hello World](ch2-uefi-hello.md) — We'll make something actually appear on screen.
