# System Calls

kaguyaOS uses the AMD64 fast `syscall`/`sysret` interface. Syscall number is passed in `RAX`, arguments in `RDI`–`R8`. Return value in `RAX`. Unknown syscalls return `usize::MAX`.

Every process has its own address space. All pointer arguments must be mapped user-accessible in the caller's address space; kernel identity-mapped memory is rejected.

---

## I/O

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 0 | `print` | `RDI=ptr, RSI=len` | — | Write UTF-8 string to console |
| 6 | `xhci_poll` | — | — | Poll xHCI for USB events. Not needed with `read` on the console, which polls on the caller's behalf |
| 8 | `read_key` | — | `u8` | Read one key from keyboard buffer (0 if empty, never blocks). Prefer `read` on fd 0 |
| 9 | `clear` | — | — | Clear the screen |

## Memory

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 28 | `heap_grow` | `RDI=bytes` | `old_end` | Map `bytes` (rounded up to pages) of zeroed memory at the end of the process heap (`0x7000_0000_0000`, up to 256 MiB). Returns the previous end, or 0 on failure. `heap_grow(0)` returns the current end. |
| 1, 2, 10 | *(retired)* | — | `0` | Former kernel-side `alloc`/`free`/`realloc`. `alloc`, `free` and `realloc` in `user/src/std.rs` are now a userspace allocator built on `heap_grow`; the kernel never reads heap contents. |

## Tasks

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 3 | `add_task` | `RDI=entry, RSI=user_rsp` | `task_id` | Spawn a user-mode thread sharing the caller's address space |
| 4 | `yield` | — | — | Yield (cooperative context switch) |
| 5 | `terminate` | `RDI=exit_code` | — | Terminate current task |
| 16 | `get_task_status` | `RDI=task_id` | `usize` | Query task state (0=Ready, 1=Running, 2=Terminated) |
| 17 | `get_task_exit_code` | `RDI=task_id` | `usize` | Get terminated task's exit code |
| 21 | `exec` | `RDI=name_ptr, RSI=name_len` | `task_id` | Execute a KEF binary (no arguments) |
| 22 | `exec2` | `RDI=name_ptr, RSI=name_len, RDX=args_ptr, R10=args_len` | `task_id` | Execute a KEF binary with arguments (≤ 2 KiB, copied to the top of the child's stack) |
| 26 | `wait_task` | `RDI=task_id` | `exit_code` | Block current task until the target task exits |

## File descriptors

Each process has up to 32 descriptors. fds 0, 1 and 2 start as the console; `exec`/`exec2` give the child a fresh table, `spawn` copies three chosen descriptors into the child's fds 0–2, and `add_task` threads share their creator's table. A process's descriptors are closed as soon as it terminates. `print` in `user/src/std.rs` writes to fd 1, so program output follows redirection. Errors are negative codes: `-2` invalid argument, `-5` disk or 4 GiB size limit reached, `-6` file not found (including a descriptor whose file was deleted or replaced), `-7` bad descriptor or wrong access mode, `-8` table full, `-9` broken pipe, `-10` address in use, `-11` timed out, `-12` network unreachable (no NIC, no address, or ARP failed), `-13` not connected/bound, `-14` connection refused, `-15` connection reset.

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 29 | `open` | `RDI=name_ptr, RSI=name_len, RDX=flags` | `fd` | Open a file. Flags: `READ=1`, `WRITE=2`, `CREATE=4`, `TRUNC=8`, `APPEND=16`; at least one of READ/WRITE, and CREATE/TRUNC/APPEND need WRITE |
| 30 | `read` | `RDI=fd, RSI=buf_ptr, RDX=len` | `bytes` | Read from the current offset; 0 at end of file. On the console it sleeps until at least one key arrives (the console has no end of input); while a reader waits, the BSP timer tick polls xHCI, or the idle loop in the hardware shell |
| 31 | `write` | `RDI=fd, RSI=buf_ptr, RDX=len` | `bytes` | Write at the current offset (end of file with APPEND); goes straight to disk. Writing past the end zero-fills the gap. Short only when the disk fills up |
| 32 | `close` | `RDI=fd` | `0` | Release a descriptor |
| 33 | `seek` | `RDI=fd, RSI=offset (signed), RDX=whence` | `offset` | Set the offset relative to `0`=start, `1`=current, `2`=end. Only valid on files |
| 34 | `pipe` | `RDI=fds_ptr` (`[usize; 2]`, 8-byte aligned) | `0` | Create a 4 KiB pipe; stores the read end, then the write end. Reads block while it is empty and return 0 once every write end is closed; writes block while it is full and return `-9` once every read end is closed |
| 35 | `spawn` | `RDI=name_ptr, RSI=name_len, RDX=args_ptr, R10=args_len, R8=stdio_ptr` (`[usize; 3]`, 8-byte aligned) | `task_id` | Like `exec2`, but the child's fds 0, 1 and 2 copy the caller's descriptors listed at `stdio_ptr`. Returns `usize::MAX` on failure. A copied file keeps its own offset |
| 36 | `fd_kind` | `RDI=fd` | `kind` | `0` console, `1` file, `2` pipe, `3` socket, or `-7` if unused |

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

Sockets are descriptors. IPv4 addresses are packed as for `net_send_ping` (`a.b.c.d` → `a | b<<8 | c<<16 | d<<24`); ports are host byte order. A UDP socket is bound on first use if `bind` was not called (ephemeral ports start at 50000; 49152 belongs to the kernel DNS client). A connected UDP socket accepts datagrams only from its peer and supports `read`/`write`, so it works with pipes and redirection. Each socket queues at most 16 datagrams; more are dropped. Payloads are limited to 1472 bytes (no IP fragmentation).

A TCP socket is a client stream: `connect` blocks until the handshake completes, `read` blocks until data arrives and returns 0 after the peer closes, and `write` blocks until everything is queued (8 KiB send buffer, 16 KiB receive window). Closing the last descriptor sends a FIN after queued data; the kernel keeps the connection until it is acknowledged. Lost segments are retransmitted with a doubling timeout starting at 1 s; after 6 retries the connection fails with `-11`. Out-of-order segments are dropped and re-acknowledged. There are no listening TCP sockets yet, `bind`/`sendto`/`recvfrom` reject TCP sockets, and `read` has no timeout.

| # | Name | Arguments | Return | Description |
|---|------|-----------|--------|-------------|
| 37 | `socket` | `RDI=kind` | `fd` | `1` = UDP, `2` = TCP |
| 38 | `bind` | `RDI=fd, RSI=port` | `port` | Bind a local port; `0` picks an ephemeral one. Returns the bound port |
| 39 | `connect` | `RDI=fd, RSI=ip_packed, RDX=port` | `0` | UDP: set the default peer and filter incoming datagrams to it. TCP: open the connection (blocks; `-14` if refused, `-11` if unanswered) |
| 40 | `sendto` | `RDI=fd, RSI=buf_ptr, RDX=len, R10=ip_packed, R8=port` | `bytes` | Send one datagram; ip and port both `0` mean the connected peer |
| 41 | `recvfrom` | `RDI=fd, RSI=buf_ptr, RDX=len, R10=addr_ptr, R8=timeout_ms` | `bytes` | Block for one datagram, truncated to `len`. `addr_ptr` (optional, 4-byte aligned) receives `{ ip: [u8; 4], port: u16, _reserved: u16 }`. `timeout_ms` of `0` waits forever, otherwise `-11` on timeout |
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
- **Clobbers**: `RCX`, `R11` and all argument registers (`RDI`, `RSI`, `RDX`, `R10`, `R8`, `R9`) are clobbered; inline-asm wrappers must declare them as outputs.
- **String args**: Pass a pointer and length (e.g., `RDI=name_ptr, RSI=name_len`).
- **Packed IP**: For `net_send_ping`, pack 4 bytes of IPv4 into a single `u32` (e.g., `0x0A000202` for `10.0.2.2`).
- **Task IDs**: Returned by `add_task`, `exec`, `exec2`. Use with `wait_task`, `get_task_status`, or `get_task_exit_code`.
- **ICMP replies**: `net_recv_ping` is non-blocking. Call `yield` in a loop to poll for replies.
