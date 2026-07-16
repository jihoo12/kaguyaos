# System Calls

kaguyaOS uses the AMD64 fast `syscall`/`sysret` interface. Syscall number is passed in `RAX`, arguments in `RDI`–`R8`. Return value in `RAX`. Unknown syscalls return `usize::MAX`.

All pointer arguments are validated against user address-space limits and page table mappings before kernel access.

---

## I/O

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 0 | `print` | `RDI=ptr, RSI=len` | — | Write UTF-8 string to console |
| 6 | `xhci_poll` | — | — | Poll xHCI for USB events (keyboard input) |
| 8 | `read_key` | — | `u8` | Read one key from keyboard buffer (0 if empty) |
| 9 | `clear` | — | — | Clear the screen |

## Memory

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 1 | `alloc` | `RDI=size, RSI=align` | `ptr` | Allocate from user heap |
| 2 | `free` | `RDI=ptr` | — | Free user heap memory |
| 10 | `realloc` | `RDI=ptr, RSI=size, RDX=align` | `ptr` | Reallocate user heap memory |

## Tasks

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 3 | `add_task` | `RDI=entry, RSI=user_rsp` | `task_id` | Spawn a new user-mode task |
| 4 | `yield` | — | — | Yield (cooperative context switch) |
| 5 | `terminate` | `RDI=exit_code` | — | Terminate current task |
| 16 | `get_task_status` | `RDI=task_id` | `usize` | Query task state (0=Ready, 1=Running, 2=Terminated) |
| 17 | `get_task_exit_code` | `RDI=task_id` | `usize` | Get terminated task's exit code |
| 21 | `exec` | `RDI=name_ptr, RSI=name_len` | `task_id` | Execute a KEF binary (no arguments) |
| 22 | `exec2` | `RDI=name_ptr, RSI=name_len, RDX=args_ptr, R10=args_len` | `task_id` | Execute a KEF binary with arguments |

## Filesystem (FAT16 on NVMe)

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 11 | `fs_format` | — | `i32` | Format the FAT16 filesystem |
| 12 | `fs_ls` | `RDI=buf_ptr, RSI=max_entries` | `count` | List files into buffer |
| 13 | `fs_write` | `RDI=name_ptr, RSI=name_len, RDX=data_ptr, R10=data_len` | `i32` | Create or overwrite a file |
| 14 | `fs_read` | `RDI=name_ptr, RSI=name_len, RDX=buf_ptr, R10=buf_len` | `bytes_read` | Read a file into buffer |
| 15 | `fs_rm` | `RDI=name_ptr, RSI=name_len` | `i32` | Delete a file |

## Terminal

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 19 | `write_cell` | `RDI=row, RSI=col, RDX=char, R10=fg, R8=bg` | — | Write a single terminal cell |
| 20 | `write_region` | `RDI=row, RSI=col, RDX=ptr, R10=len, R8=width` | — | Write a batch of terminal cells |

## Network

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 23 | `net_send_ping` | `RDI=dst_ip_packed` | `seq` | Send ICMP Echo Request (returns sequence number, 0 on failure) |
| 24 | `net_recv_ping` | `RDI=buf_ptr, RSI=buf_len` | `bytes` | Receive ICMP Echo Reply (non-blocking, returns bytes copied) |

## System

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 7 | `shutdown` | — | — | Power off the machine |
| 18 | `run_ap_scheduler` | — | — | Enter AP scheduler loop (never returns) |

---

## Notes

- **Argument passing**: Syscall number in `RAX`, up to 6 args in `RDI`, `RSI`, `RDX`, `R10`, `R8`, `R9`.
- **String args**: Pass a pointer and length (e.g., `RDI=name_ptr, RSI=name_len`).
- **Packed IP**: For `net_send_ping`, pack 4 bytes of IPv4 into a single `u32` (e.g., `0x0A000202` for `10.0.2.2`).
- **Task IDs**: Returned by `add_task`, `exec`, `exec2`. Use with `get_task_status` / `get_task_exit_code`.
- **ICMP replies**: `net_recv_ping` is non-blocking. Call `yield` in a loop to poll for replies.
