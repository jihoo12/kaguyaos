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
- **SimpleFS Filesystem**: Custom flat-layout filesystem with block storage allocation, mounted automatically at startup.
- **Tiny C JIT Compiler (`cc`)**: A built-in userspace C-to-machine-code compiler and JIT execution engine.
- **TinyASM (JIT Assembler)**: A custom-built assembler and JIT engine allowing dynamic execution of x86_64 assembly directly from the shell.
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
./run.sh
```

This script will:
1. Build the kernel for `x86_64-unknown-uefi`.
2. Create the necessary EFI directory structure in `esp/`.
3. Create a raw 1GB NVMe disk image (`nvme.img`) if it doesn't exist.
4. Launch QEMU with the OS, USB keyboard, and NVMe drive attached.

### 2. Interactive Shell Commands

Once the system boots, you will be dropped into an interactive shell (`kaguya>`). The following commands are registered:

| Command | Usage | Description |
|:---|:---|:---|
| **`help`** | `help` | Show the available command menu. |
| **`echo`** | `echo [args...]` | Print the arguments to the screen. |
| **`history`** | `history` | Show the shell command history (supports arrow keys). |
| **`clear`** | `clear` | Clear the screen. |
| **`shutdown`** | `shutdown` | Cleanly shut down the system and power off QEMU. |
| **`fsformat`** | `fsformat` | Format the NVMe drive with SimpleFS. |
| **`fsls`** | `fsls` | List all active files in the filesystem. |
| **`fswrite`** | `fswrite <filename> [content]` | Write text to a file (enters multi-line write mode if content is omitted). |
| **`fsread`** | `fsread <filename>` | Read and display the contents of a file. |
| **`fsrm`** | `fsrm <filename>` | Delete a file from the filesystem. |
| **`compile`** | `compile <src_file> <dest_file>` | Compile a `.c` file or assemble a `.asm`/`.s` file to machine code. |
| **`load`** | `load <file...>` | Run one or more files as processes (compiles C/assembly on the fly). |

---

## 💾 SimpleFS Filesystem

**SimpleFS** is a custom flat filesystem implemented directly over the NVMe driver. It uses a contiguous block allocation strategy:

* **Block 0 (Superblock)**: Stores the filesystem magic number (`0x5349_4d50_4c45_4653`), next free block allocator index, and the current active file count.
* **Blocks 1–16 (Directory entries)**: Supports up to 128 flat files. Each directory entry is 64 bytes and holds the filename, file size, and starting block index.
* **Blocks 17+ (Data Blocks)**: Contiguously stores actual file contents.

### Operations
SimpleFS supports full CRUD operations (`fsformat`, `fsls`, `fswrite`, `fsread`, `fsrm`) directly from the shell or via system calls.

---

## 🛠️ Tiny C JIT Compiler (`cc`)

The JIT compiler allows you to write C-like source files (normally `.c` extension) and compile/run them as Ring 3 userspace processes. It parses tokens, builds function mappings, and generates raw x86_64 machine instructions.

### 📝 Language Syntax & Limitations
- **Types**: Supports `uint64_t` and `char` (both compile as 64-bit unsigned integers). No pointer syntax or structs are supported yet.
- **Parameters**: Supports functions with up to 6 parameters (mapped to register registers `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9` per the **System V AMD64 ABI**).
- **Operators**: No direct arithmetic operators (e.g. `+`, `-`). Computations must be done via inline assembly or helper functions.
- **Control Flow**: No loops or conditional statements (no `if`, `while`, `for`, etc.).
- **Inline Assembly**: Embed raw instructions via `asm("assembly")` or `__asm__("assembly")`. Statements inside the string can be separated by `;` or newlines.
- **Comments**: Comments (`//` or `/* */`) are currently not parsed.

### 🚀 Compilation and Execution Example
To write, compile, and run a C application inside kaguyaos:

1. Create a C file in SimpleFS:
   ```bash
   fswrite my_prog.c
   ```
2. Enter the C source code (type `esc` when finished):
   ```c
   uint64_t print_char(uint64_t c) {
       // Push character to the stack and pass stack pointer to sys_print (Syscall 1)
       asm("push rdi; mov rdi, rsp; mov rsi, 1; mov rax, 1; syscall; pop rdi");
       return c;
   }

   uint64_t add(uint64_t a, uint64_t b) {
       // Parameters are located at [rbp-8] (a) and [rbp-16] (b)
       asm("mov rax, [rbp-8]; add rax, [rbp-16]");
       return a;
   }

   uint64_t main() {
       uint64_t x = 10;
       uint64_t y = 20;
       uint64_t sum = add(x, y); // sum is 30
       
       // Calculate character 'A' (ASCII 65) by adding 35 to the sum
       uint64_t letter = add(sum, 35);
       print_char(letter); // Output: A
       
       return sum; // Process returns exit code 30
   }
   ```
3. Load and execute the code:
   ```bash
   load my_prog.c
   ```
   **Output:**
   ```text
   Spawned process PID=1
   A
   Process finished with exit code: 30
   ```

---

## 🔌 System Calls

kaguyaos implements a robust system call interface over the standard AMD64 `syscall`/`sysretq` mechanism.

| ID | Name | Arguments | Description |
|:---|:---|:---|:---|
| **1** | `sys_print` | `ptr: usize, len: usize` | Prints a UTF-8 string to the graphical console. |
| **2** | `sys_alloc` | `size: usize, align: usize` | Allocates memory from the heap with the specified alignment. |
| **3** | `sys_free` | `ptr: usize` | Frees an allocated memory pointer. |
| **4** | `sys_add_task` | `entry: usize, user_rsp: usize` | Spawns a new user mode task/process. Returns process ID (PID). |
| **5** | `sys_switch_task` | None | Yields the processor to the next task in the scheduler queue. |
| **6** | `sys_terminate_task`| `exit_code: usize` | Terminates the current task with an exit code. |
| **7** | `sys_nvme_read` | `lba: usize, ptr: usize, count: usize` | Reads `count` blocks from the NVMe disk starting at logical block address `lba` into buffer `ptr`. |
| **8** | `sys_nvme_write` | `lba: usize, ptr: usize, count: usize` | Writes `count` blocks from buffer `ptr` to the NVMe disk starting at logical block address `lba`. |
| **9** | `sys_xhci_poll` | None | Polls the USB xHCI controller for keyboard events. |
| **10**| `sys_shutdown` | None | Powers off the system (emulates shutdown in QEMU via UEFI). |
| **11**| `sys_read_key` | None | Reads a character from the keyboard event queue. Returns ASCII value or 0 if empty. |
| **12**| `sys_clear` | None | Clears the graphical console screen. |
| **13**| `sys_realloc` | `ptr: usize, size: usize, align: usize` | Reallocates memory at `ptr` with a new `size` and `align`. |
| **14**| `sys_fsformat` | None | Formats the NVMe drive with the SimpleFS filesystem layout. |
| **15**| `sys_fsls` | `buffer_ptr: usize, max_entries: usize` | Lists up to `max_entries` files into a `SyscallFileEntry` buffer. Returns file count. |
| **16**| `sys_fswrite` | `filename_ptr: usize, filename_len: usize, content_ptr: usize, content_len: usize` | Creates or overwrites a file with the given name and content. |
| **17**| `sys_fsread` | `filename_ptr: usize, filename_len: usize, buffer_ptr: usize, buffer_len: usize` | Reads the contents of a file into a buffer. Returns the file size or copied length. |
| **18**| `sys_fsrm` | `filename_ptr: usize, filename_len: usize` | Deletes a file from SimpleFS. |
| **19**| `sys_get_task_status`| `task_id: usize` | Gets the status of the task (e.g. 2 for Terminated, 3 for Not Found). |
| **20**| `sys_get_task_exit_code`| `task_id: usize` | Gets the exit code of a completed process. |
