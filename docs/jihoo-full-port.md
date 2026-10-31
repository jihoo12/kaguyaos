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

Continue with CPU-local/syscall state, address-space ownership, KEF loading,
and scheduling, followed by device work. The standalone image does not yet run
userspace or drivers. The ordinary kernel remains Rust and complete Rust
removal has not been achieved. No physical hardware has been tested.
