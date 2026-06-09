# kaguyaos

A custom Operating System written in Rust, targeting the x86_64 UEFI architecture. This project demonstrates key OS concepts including UEFI booting, graphical user interface, user-mode execution, system calls, and device driver support (NVMe & xHCI).

![Rust](https://img.shields.io/badge/language-Rust-orange)
![Platform](https://img.shields.io/badge/platform-x86__64--UEFI-blue)

## ✨ Features

- **UEFI Booting**: Fully compliant with the Unified Extensible Firmware Interface standard.
- **Graphical Framebuffer**: High-resolution graphics.
- **Interactive Shell**: A built-in userspace shell (Ring 3) for command execution.
- **USB 3.0 Support**: Custom **xHCI Driver** supporting keyboard input.
- **NVMe Support**: Native driver for generic NVMe SSDs.
- **User Mode**: Secure transition from Kernel to User mode with Ring 3 privilege isolation.
- **System Calls**: Robust syscall interface for user-kernel communication (print, keyboard, shutdown, etc.).
- **TinyASM (JIT Assembler)**: A custom-built assembler and JIT engine allowing dynamic execution of x86_64 assembly directly from the shell.
## 🛠️ Prerequisites

To build and run this OS, you need the following tools installed:

- **Rust Nightly**: Required for experimental OS features (inline assembly, naked functions, etc.).
- **QEMU**: For system emulation (`qemu-system-x86_64`).
- **OVMF**: UEFI firmware for QEMU.

## 🚀 Getting Started

### 1. Build and Run

Use the provided helper script to compile the kernel, create the disk image, and launch QEMU.

```bash
nix-shell # if you use nix
export OVMF_BIOS="/usr/share/ovmf/OVMF.fd" # if you don't use nix
./run.sh
```

This script will:
1. Build the kernel for `x86_64-unknown-uefi`.
2. Create the necessary EFI directory structure.
3. Create a raw NVMe disk image (`nvme.img`) if it doesn't exist.
4. Launch QEMU with the OS, USB keyboard, and NVMe drive attached.

### 2. Usage

Once the system boots, you will be dropped into an interactive shell.
**Available Commands:**
- `help`: Show available commands.
- `echo <args>`: Print arguments to the screen.
- `history`: Show command history.
- `clear`: Clear the screen.
- `shutdown`: Cleanly shut down the system (powers off QEMU).
- `asm <instruction>`: Execute x86_64 assembly instructions dynamically.

### ⌨️ TinyASM (Inline Assembly)

The OS includes a lightweight JIT assembler called **TinyASM**. It allows you to write and execute x86_64 assembly instructions directly in the shell.

**Key Features:**
- **Dynamic JIT**: Parses, encodes, and executes machine code in real-time.
- **Support for Multiple Instructions**: Use `;` to separate multiple instructions.
- **Return Value**: The shell prints the value of the `rax` register after execution.

**Usage Examples:**

```bash
# Basic arithmetic
asm mov rax, 10; add rax, 20;ret
# Result: 30

# System call example (Shutdown)
asm mov rax, 10; syscall;ret

# bit manipulation
asm mov rax, 0xFF; and rax, 0x0F; ret
# result: 15
```

