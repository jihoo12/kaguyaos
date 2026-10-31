# Complete jihoo migration

The target is a kernel, userspace, and ordinary OS build/run workflow that
do not require Rust. Existing Rust implementations remain comparison
references during migration; a component is removed from the build only
after its replacement passes the applicable host and QEMU checks. Assembly
continues to provide CPU entry, interrupt, syscall, and context-switch ABI
boundaries.

## Current status (2026-10-31)

The supported kernel, all seven programs, host image tool, ordinary build/run scripts and both hardware images now use Jihoo and assembly without Rust tools. Four-CPU NVMe/virtio/network and polled diskless QEMU checks pass. The final checkpoint at the end of this document records verification and remaining hardware limits. Earlier checkpoints below are historical; their statements about unfinished components describe the state at that time.

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

## Historical checkpoints

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

## NVMe and native image-tool checkpoint

`nvme_queue.jh` supplies submission encoding, bounded PRP-list generation,
doorbell strides and completion phase/status handling. `nvme.jh` discovers
and enables one controller, maps its MMIO before process creation, creates
admin/I/O queues, identifies namespace 1, requires 512-byte LBAs without
metadata, and supplies serialized synchronous transfers and shutdown.
DMA uses identity-mapped kernel buffers and a retained PRP-list page.
Queue/register waits are bounded; a completion timeout disables further
I/O and retains DMA storage.

The `nvme-shell` EFI mounts the existing disk volume without seeding,
formatting or replacing its programs. The `shell` and diagnostic paths
still avoid disk initialization. `tools/kef-tool/main.jh` replaces the
Rust image-tool implementation for this checkpoint by using the same
`filesystem.jh` and `fat.jh` as the kernel. It supports explicit format,
recognition checks, listing, insertion, extraction and deletion. It opens
an existing image; it does not create or implicitly format one.

Verified on compiler `8a7aecfcf3b4e72ac40890f739deb96cca3019f6`:

- Native C queue/PRP tests at `-O0` and `-O2`: submission bytes, nondefault
  doorbell strides, phase wrap, foreign command IDs, status errors, unaligned
  buffers, list capacity and overflow rejection.
- Native Jihoo host-tool round trip: a 17,000-byte file, empty overwrite,
  list/extract/delete, and unchanged unrecognized image after rejected insert.
- QEMU NVMe boot from a newly prepared disposable 16 MiB image with all
  seven Jihoo programs: file create/read/overwrite/delete, eight-stage pipes,
  redirection/append, exec arguments, syntax errors, forty sequential loads
  with constant frame counts and guest-initiated NVMe/UEFI shutdown.

```sh
CC=clang bash tools/test-jihoo-nvme.sh
CC=clang bash tools/build-jihoo-kernel.sh nvme-shell
bash tools/build-kef-tool.sh
python3 tools/gpu-smoke.py --jihoo-nvme --disk-image /tmp/kaguya-jihoo-port-nvme.img --shell-io --shutdown --output /tmp/kaguya-jihoo-port-nvme
```

The verified NVMe checkpoint is commit `3e08841`.
The verified artifacts are `target/jihoo-kernel/nvme-shell/os.efi` and
`target/jihoo-tools/kef-tool`; QEMU logs/captures are in
`/tmp/kaguya-jihoo-port-nvme`. The test image was initialized explicitly by
the host tool before boot, and QEMU used a disposable snapshot. No physical
disk or hardware was tested. Ordinary scripts still use Cargo/rustc; the
complete migration is unfinished.

## Graphics checkpoint

Compiler issue #24 is fixed in `ea3c2e0b87f8177058b4a8dcfa97588960cf6d96`.
Native direct-`None` and control cases print `true` at both optimization
levels; the UEFI cases check and emit COFF. No initializer workaround is
connected. The graphics draft's borrowed error strings were separately
changed to owned `String` errors to follow Jihoo's view-return rules.

`display.jh` and `virtio_gpu.jh` now compile and are connected to the
NVMe boot path. Modern PCI capabilities, VERSION_1 negotiation, fenced
split-queue commands, one 2D scanout, retained DMA storage, dirty rectangles,
GOP conversion and the desktop/window viewport are ported. Runtime output,
clear and cell/region syscalls serialize rendering and presentation with
the interrupt-safe screen lock; graphics error output stays serial-only.
MMIO mappings are made before the first process address space.

Native C tests pass GOP conversion, dirty clipping/stride bounds and
allocation cleanup at `-O0` and `-O2`. QEMU activates a virtio scanout at
1280x800 and passes file utilities, eight-stage pipelines, redirection,
exec arguments, forty constant-frame loads and guest shutdown. A separate
GOP run passes its shell/file/reclamation/shutdown regression. Logs and
screenshots are in `/tmp/kaguya-jihoo-port-nvme` and
`/tmp/kaguya-jihoo-display-gop`.

```sh
CC=clang bash tools/test-jihoo-display.sh
CC=clang bash tools/build-jihoo-kernel.sh nvme-shell
python3 tools/gpu-smoke.py --jihoo-nvme --disk-image /tmp/kaguya-jihoo-port-nvme.img --shell-io --shutdown --output /tmp/kaguya-jihoo-port-nvme
python3 tools/gpu-smoke.py --jihoo-nvme --backend gop --disk-image /tmp/kaguya-jihoo-port-nvme.img --shutdown --output /tmp/kaguya-jihoo-display-gop
```

The supported backbuffer limit remains 1920x1080; larger layouts retain
the direct GOP console. Physical GPU hardware and timeout/error paths
have not been exercised. ACPI/APIC/SMP, network services and ordinary build
script replacement remain unfinished; continue with network ownership and
drivers. Ordinary builds still require Rust.

## Network checkpoint

The standalone NVMe path now initializes an e1000 NIC and connects Ethernet,
ARP/IPv4/ICMP, UDP/DNS and TCP client sockets to descriptor ownership and
syscalls. Receive IRQs acknowledge the device and publish work; bounded
packet draining and retransmission run in normal scheduler/syscall context.
The current platform uses the legacy PIC route. The later APIC/SMP port
must retain that interrupt delivery through its I/O APIC routing.

`Shared` now supports atomic weak references so UDP port registration does
not keep closed sockets alive. Upgrade cannot resurrect a last-dropped
value, and storage remains until the final weak owner is gone. TCP keeps
connections alive while sending FIN, acknowledges/retransmits bounded
stream buffers and closes them after completion, reset or timeout. The
existing client-only TCP limitations are retained: no listener, out-of-order
reassembly, congestion control or TIME_WAIT. IPv4 uses the QEMU static
address/gateway configuration; DNS permits one outstanding fixed-ID query.

Native tests at both optimization levels pass concurrent weak upgrade vs.
last-owner destruction, descriptor/scheduler regressions, UDP bind/filter/
queue/timeout/lifetime/port reuse, truncated DNS rejection and compressed
A answers, checksum rejection and byte-ring capacity/wrap. QEMU confirms
actual e1000 RX interrupts, ten successful gateway pings, HTTP body reads,
relative redirects, output redirection, a 10,200-byte body streamed through
bounded pipes, 404/incomplete-body errors, HTTPS rejection, constant-frame
execs and NVMe/UEFI shutdown. Logs are in `/tmp/kaguya-jihoo-network-port`.

```sh
CC=clang bash tools/test-jihoo-shared.sh
CC=clang bash tools/test-jihoo-udp.sh
CC=clang bash tools/build-jihoo-kernel.sh nvme-shell
python3 tools/gpu-smoke.py --jihoo-nvme --disk-image /tmp/kaguya-jihoo-port-nvme.img --network --shutdown --output /tmp/kaguya-jihoo-network-port
```

Physical NIC hardware, loss/reordering and multi-CPU protocol execution
have not been exercised here. ACPI/APIC/SMP and ordinary build-script
replacement remain unfinished. The ordinary build still requires Rust.

## Completed SMP and supported-build checkpoint (2026-10-31)

Compiler `ea3c2e0b87f8177058b4a8dcfa97588960cf6d96` fixes issue #24. No new compiler problem was found while completing this checkpoint.

`acpi.jh` validates RSDP/root/child checksums and lengths, bounds firmware mapping and parses enabled MADT CPUs, IOAPICs, interrupt overrides and LAPIC overrides. FADT, HPET and MCFG helpers retain the relevant firmware queries. Native fixtures pass checksums, invalid pointers, malformed records, duplicate/disabled CPUs, FADT extended addresses and ECAM bounds at O0/O2.

The normal EFI now starts APs through a firmware-allocated low LoaderData page, rather than overwriting a fixed physical address. Each AP has its own GDT/TSS, GS state and stack, loads the shared IDT/root, enables its LAPIC timer and joins the scheduler. E1000 INTx uses the IOAPIC. A bounded startup handshake prevents reuse of a trampoline that an unacknowledged AP might still access. Shutdown parks all other CPUs before stopping devices and requesting UEFI power-off.

The scheduler's outgoing task remains unavailable to other CPUs until assembly has saved its stack and acknowledged release. Event wakeups cannot resume the outgoing context early. Task-exit waiters wake after retirement acknowledgement and frame/stack reclamation. Native stack-switch tests pass; four-CPU QEMU tests show actual AP context switches, constant frames across 40 sequential loads, eight-stage pipelines, sleep/waits and network protocol progress.

All syscall IDs `0..41` are implemented, including the legacy print, file, exec and shared-address-space-thread paths. The `verify.kef compat` probe checks legacy file calls, invalid user pointers, exec/exec2, shared address-space memory and shared descriptor offsets; separate busy children exercise user timer return. Formatting remains an explicit syscall/tool request only.

The supported scripts now select Jihoo directly: `build.sh`, `run.sh`, `build_insert_run.sh`, `user/build.sh`, `user/insert.sh`, and both hardware wrappers. Nix provides Clang/LLVM/QEMU/OVMF without installing or initializing Rust. The image tool is Jihoo. New regular host images are initialized explicitly; existing unrecognized images fail without modification. Default artifacts are `target/kaguya/os.efi` and `esp/EFI/BOOT/BOOTX64.EFI`.

The hardware diagnostic remains isolated. A separate `hardware-shell` mode keeps IF clear in kernel and userspace, polls USB on the BSP and never initializes storage, network, timers or APs. The public hardware artifact paths are preserved. The timer-driven diskless `shell` checkpoint remains separate.

`tools/check-rust-free-build.sh` traps `rustc`, Cargo, `rustup` and `rust-lld`, then builds the kernel, seven programs, Jihoo image tool and both hardware images. It also compares an existing invalid image before/after rejection. This check passes with zero trapped calls. `nix flake check --no-build` and the same trapped build inside `nix develop` pass. Rust sources and differential harnesses remain optional historical references, excluded from the supported OS build.

Recorded QEMU artifacts:

- `/tmp/kaguya-jihoo-smp-port`: two CPUs, USB/files, 40 constant-frame loads and shutdown.
- `/tmp/kaguya-jihoo-smp-network`: four CPUs, AP scheduling, eight-stage pipelines, IRQ receive, gateway ping, large HTTP bodies, redirects/errors, bounded-pipe output and shutdown parking.
- `/tmp/kaguya-jihoo-hardware-polled`: restricted diskless USB shell, pipelines, constant frames and shutdown with timers/APs off.
- `/tmp/kaguya-jihoo-default-final`: the ordinary Rust-tool-blocked build, legacy ABI/shared-thread checks, four CPUs and local network fixtures.
- `/tmp/kaguya-jihoo-abi-gop-final`: single-CPU GOP, shared-thread/legacy ABI checks, terminal region pixel placement and user timer preemption.

The ordinary build no longer requires Rust. Replacement Jihoo images still need physical-hardware validation; the older physical successes belong to the Rust reference. Protocol loss/reordering and physical NIC/NVMe/GPU/SMP behavior have not been validated here. See the root README for remaining runtime limits.
