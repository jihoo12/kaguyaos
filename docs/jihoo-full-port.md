# Complete jihoo migration

The target is a kernel, userspace, and ordinary OS build/run workflow that
do not require Rust. Existing Rust implementations remain comparison
references during migration; a component is removed from the build only
after its replacement passes the applicable host and QEMU checks. Assembly
continues to provide CPU entry, interrupt, syscall, and context-switch ABI
boundaries.

## Work order

1. Supply a kernel `std.sys` implementation so jihoo collections, ownership,
   formatting, and destructors allocate from the kernel heap and use kernel
   output after UEFI boot services have ended.
2. Build a standalone jihoo UEFI image and carry over GOP/memory-map checks,
   ExitBootServices retry, page tables, diagnostics, and runtime shutdown.
3. Move console/font/terminal/display state and interrupt-safe synchronization.
4. Move GDT/IDT, CPU-local state, interrupt/syscall assembly boundaries, and
   the scheduler, preserving preemption, SMP handoffs, wait generations, and
   stack/frame reclamation.
5. Move ownership of address spaces, pipes, descriptor tables, and loaded KEF
   programs, then exercise the diskless ring-3 shell without Rust.
6. Finish filesystem metadata/format/open/create/delete/list logic and block
   backends; retain explicit formatting policy.
7. Move PCI registration, NVMe, xHCI/keyboard, virtio GPU, ACPI/APIC, and SMP
   startup with device access and DMA mapping checks.
8. Move Ethernet, ARP/IPv4/ICMP, UDP/TCP, DNS, socket state, and network waits.
9. Switch the ordinary kernel/userspace build scripts to jihoo, replace the
   Rust disk-image host tool, and remove Cargo/font-crate requirements from
   the supported build. Replace Rust-only verification dependencies where
   they would otherwise be needed to validate the finished OS.

The optional `jihoo-kernel` build already selects checksum, PIC, port I/O,
graphics primitives, locks, KEF validation, memory allocation/page tables,
PCI scanning/access, pipe buffering/counts, FAT helpers, and file-data
reads/writes. Those implementations and their tests are reused rather than
rewritten. `src/jihoo/README.md` records their verified scope.

## Completion checks

- Build the OS and all seven user programs without invoking Cargo or rustc.
- Pass isolated diagnostic and diskless USB shell checks, filesystem I/O,
  eight-stage pipelines/redirection, repeated exec/frame reclamation, and
  guest shutdown.
- Pass normal NVMe/virtio boot and local network fixtures, including large
  HTTP output through bounded pipes, redirects, errors, and incomplete bodies.
- Exercise the preserved scheduler/SMP/device behaviors and document any
  hardware paths that have not been physically tested.
- Keep changes reviewable with intermediate commits and English README updates.

All disk verification uses disposable copies and QEMU snapshots. An
unrecognized physical or virtual disk is never automatically formatted.
If a new jihoo language/compiler problem appears, confirm a minimal case
and controls, report it to `jihoolang/jihoo` with `gh`, and stop migration
until it is fixed. Record the exact compiler commit and preserve completed
builds when doing so.

## Current checkpoint

The kernel `std.sys` adapter is connected to the mixed kernel and passes
host collection-lifetime tests plus diskless and normal QEMU regressions.
That verified checkpoint is commit `0112a08`.
Compiler issue #21 is fixed in `c8014f4b1d3bbc22bfc4c267193b47737268bf1a`.
The matching extern/export reproductions now emit native and UEFI objects;
combined, separate-object, and ordinary-call controls run successfully.
Signature mismatches and duplicate export definitions are rejected.

The independent jihoo UEFI diagnostic builds without Cargo or rustc:

```sh
CC=clang bash tools/build-jihoo-kernel.sh
python3 tools/gpu-smoke.py --hardware-test --kernel-image target/jihoo-kernel/hardware-test/os.efi --output /tmp/kaguya-standalone-diagnostic
```

QEMU reaches `HARDWARE TEST READY` after ExitBootServices, GOP/memory-map
validation, and collection/allocation-cleanup checks on the jihoo heap.
The original serial-only artifacts are in `/tmp/kaguya-standalone-diagnostic`.

Compiler issue #22 is fixed in `d0b3d3182a33e602ef687561633d6e2be9d0472b`.
Explicit/repeated static arrays and controls pass, including native LLVM and
Clang execution, UEFI IR/COFF, and the compiler's own aggregate regression.
The C console harness passes at both optimization levels without Rust.
The standalone EFI now renders visible GOP text with a vendored licensed
font and interrupt-safe output locking. QEMU checks pass at 1280x800 and
2560x1440; captures/logs are in `/tmp/kaguya-jihoo-console` and
`/tmp/kaguya-jihoo-console-1440p`. Kernel collection/cleanup host tests also
pass on this compiler. No static-data workaround is connected.

The separate `cpu-test` EFI now loads jihoo-owned GDT/TSS/IDT tables,
switches to jihoo identity page tables using a locked frame pool, and passes
CR3 readback, kernel-only permissions, recycled-frame zeroing/accounting,
returning `int3`, and ten PIT interrupts in QEMU at both resolutions. The
packed-table C tests pass at `-O0` and `-O2`; each CPU slot reserves a separate
double-fault stack. AP startup and double faults have not been exercised.
Artifacts are in `/tmp/kaguya-jihoo-cpu` and `/tmp/kaguya-jihoo-cpu-1440p`.
The original diagnostic still stops before these initializations and passes
its regression in `/tmp/kaguya-jihoo-cpu-diagnostic`.

Atomic shared address-space ownership and full KEF mapping/argument
orchestration now live in `space.jh` and `kef.jh`. The native C tests pass
clone/last-owner lifetime, OOM cleanup, mappings, initialized data/BSS,
stack guards and argument delivery at both optimization levels. The
separate `ring3-test` EFI embeds a jihoo verification KEF and passes twenty
executions, syscall entry/return, user-pointer checks, heap growth,
contained kernel-page and NX faults, user-mode PIT return, and constant
frame/heap counts in QEMU at 1280x800 and 2560x1440. Artifacts are in
`/tmp/kaguya-jihoo-ring3` and `/tmp/kaguya-jihoo-ring3-1440p`.

This isolated harness uses a bootstrap continuation, not the scheduler.
Only exit, console-write, heap-growth and retired-allocation syscalls are
supplied. Continue with shared kernel ownership, scheduler handoffs/waits,
and file/pipe ownership, then shell/device integration. The standalone
images do not yet provide the full shell or drivers. The ordinary kernel remains Rust and complete Rust
removal has not been achieved. No physical hardware has been tested.

Generic atomic shared ownership and interrupt-safe view guards now pass
native lifetime/nested-lock/try-lock tests and a four-thread 40,000-increment
test at both optimization levels. Guard escape is rejected. Standalone
QEMU checks use the real heap and both IF states, then pass ring-3 regression.
The owned jihoo pipe module passes native clones/EOF/streaming/blocking/
partial-close/generation-race tests with complete allocation/wait-key
cleanup. Its scheduler callbacks are still host fixtures; it is not yet
connected to kernel scheduling or descriptor ownership. Next finish
filesystem/descriptor ownership and the real scheduler/device integration.

FAT metadata and buffer ownership are now implemented by `filesystem.jh`.
C memory-disk tests pass at both optimization levels for formatting policy,
metadata/slot generations, stale handles, full directories/disks, UTF-8,
append/sparse/partial I/O and injected failures. The separate
`filesystem-test` EFI creates only a new temporary kernel-only NX RAM disk
before process creation, passes its real-heap filesystem checks, then passes
all ring-3 regressions. Artifacts are in `/tmp/kaguya-jihoo-filesystem`.
No physical/virtual disk is connected or automatically formatted. The
standalone file table and scheduler still need integration; NVMe, USB,
network, virtio GPU and AP startup remain to be ported/connected.

`file_table.jh` now owns console/disk/pipe descriptors in jihoo. Native C
tests pass flags, offsets/seeks, sharing versus stdio inheritance, lifetime,
FD limits/rollback, and closure during a blocked 6,000-byte write at both
optimization levels, with full owner/wait-key cleanup. This module is still
excluded from the kernel; its console and scheduling callbacks are native
test fixtures. Connect real scheduler handoffs/events and the syscall table
next. Socket variants remain part of the later network port.

## Scheduler checkpoint

Compiler issue #23 is fixed in `8a7aecfcf3b4e72ac40890f739deb96cca3019f6`.
The rebuilt compiler passes boxed/vector field assignment checks, both native
executions and all four controls, UEFI checking, and scheduler object emission.
No workaround is connected.

`task.jh` now has real native and UEFI stack-switch bridges. Host C tests
at `-O0` and `-O2` exercise cooperative switching, wait/exit, timed sleep,
event generations, timer quanta and acknowledged stack retirement. The
standalone processor uses the shared `cpu_state.jh` record. PIT scheduling
runs after interrupt acknowledgement; kernel execution is not preempted.

The separate `scheduler-test` EFI connects address-space and descriptor
ownership to syscall dispatch, generic KEF loading from the temporary RAM
volume, stdio inheritance, and real blocking pipe waits. QEMU passes twenty
ring-3 runs with constant frame counts, filesystem I/O, child spawn/wait,
timed sleep, a 6,000-byte stream through a bounded 4 KiB pipe, EOF, concurrent
user timer preemption, and contained page/NX faults. It first runs the
existing CPU, RAM-filesystem and ring-3 checks too.

```sh
CC=clang bash tools/test-jihoo-task.sh
CC=clang bash tools/build-jihoo-kernel.sh scheduler-test
python3 tools/gpu-smoke.py --scheduler-test --output /tmp/kaguya-jihoo-scheduler
```

This is a single-CPU integration checkpoint; AP startup, hardware drivers,
full shell input and the remaining syscall services are still unfinished.
The original diagnostic still stops before CPU/device initialization.
No physical disk is connected or formatted. Ordinary OS builds still use
Rust; complete Rust removal has not been achieved.

## Standalone diskless USB shell checkpoint

The separate `shell` mode now builds the kernel and all seven user programs
without Cargo or rustc. It embeds their KEFs in the EFI, copies them only to
the newly initialized temporary RAM volume, then boots the existing Jihoo
ring-3 shell. `xhci.jh`, `xhci_ring.jh` and `hid_keyboard.jh` replace the Rust
USB path for this image. Only the selected xHCI controller is enabled; its
64 KiB MMIO window is mapped before any user address spaces are created.
DMA pages are zeroed identity-mapped frames retained for the boot lifetime.
Firmware handoff, 32/64-byte contexts, up to 256 scratchpads, bounded waits,
port-power-preserving reset, HID boot protocol and first-keyboard stopping
are carried over. Kernel allocation exhaustion stops boot.

```sh
CC=clang bash tools/test-jihoo-usb.sh
CC=clang bash tools/build-jihoo-kernel.sh shell
python3 tools/gpu-smoke.py --jihoo-shell --shell-io --shutdown --output /tmp/kaguya-jihoo-standalone-shell
```

QEMU passes USB keyboard input, file create/read/overwrite/delete, eight-stage
pipelines, input/output/append redirection, exec arguments, syntax errors,
forty sequential executions with constant frame usage, and guest shutdown.
A second run at 2560x1440 passes mouse filtering, Shift/Backspace and thirty
help commands wrapping the USB rings. No-keyboard boot passes too. Logs and
screenshots are in `/tmp/kaguya-jihoo-standalone-shell`,
`/tmp/kaguya-jihoo-standalone-usb-stress` and
`/tmp/kaguya-jihoo-standalone-no-keyboard`. The original standalone diagnostic
and scheduler regressions still pass after USB integration.

The new EFI is `target/jihoo-kernel/shell/esp/EFI/BOOT/BOOTX64.EFI` and has not
been tested on physical hardware. It uses the BSP PIT for scheduling and USB
reader polling; APs remain off. Keep the existing physical-hardware milestone
artifacts separate. `KAGUYA_XHCI_INDEX` selects another controller at build
time. No physical disk is accessed or automatically formatted. NVMe, virtio
GPU, ACPI/APIC/SMP, network services and the ordinary Rust build replacement
remain unfinished. No new Jihoo compiler problem was found in these steps.
