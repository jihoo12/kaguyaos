# Jihoo kernel migration

`CC=clang cargo build --target x86_64-unknown-uefi --features jihoo-kernel` compiles
`kernel.jh` to COFF and links it into the Rust UEFI kernel. `build.rs`
selects `JIHOO_BIN`, the adjacent `../jihoo` checkout through Nix, or
`jihoo` on PATH. Clang assembles `arch.S`.

The complete Rust-free migration is tracked in
[docs/jihoo-full-port.md](../../docs/jihoo-full-port.md). The current ordinary
kernel still uses Rust bridges. A separate Rust-free UEFI diagnostic now
builds and boots in QEMU; its limited scope is described below.

The feature currently selects jihoo implementations of:

- Internet and IPv4 transport checksums.
- PIC initialization, interrupt acknowledgement, and IRQ unmasking.
- Pixel and clipped rectangle writes.
- Interrupt-safe spinlock acquisition, try-lock, and release.
- KEF header parsing and validation.
- Process page-table creation, user-page mapping, heap mapping, and teardown.
- Kernel heap allocation, free lists, large-block splitting/coalescing, and reallocation.
- Boot-time single and contiguous frame allocation from the UEFI memory map.
- Global frame recycling, allocation counts, and full-page zeroing.
- Boot identity-map construction, kernel page-table mapping, and mapped/user
  range permission checks.
- PCI configuration reads/writes, multifunction bus scanning, MMIO BAR0
  decoding, and indexed read-only xHCI discovery.
- Pipe ring-buffer reads/writes, bounded-capacity transitions, and counted
  reader/writer ends.
- FAT entry reads/writes, bounded cluster allocation, chain walking/freeing,
  root-directory entry reads/writes, filename lookup, and free-slot search.
- Whole-file and offset-based data reads, cluster data writes, sparse-gap
  zeroing, and partial-write accounting.

The bridge uses the UEFI calling convention. `arch.S` implements port I/O,
interrupt flag operations, and pause instructions. Frame allocation and
kernel panic reporting call through Rust ABI bridges. Frame-pool state and
recycling are jihoo; the Rust bridge holds the interrupt-safe lock and
releases it before zeroing an allocated frame. Kernel mapping passes its
frame source through a synchronous C callback; CR3 and TLB instructions
remain Rust assembly. Existing
display locks and dirty-region tracking surround drawing operations.
Without the feature, the Rust reference implementations remain active.

Pipe buffers and end counts are protected by the existing interrupt-safe
lock. Rust owns their backing memory and `Arc` handles, snapshots the event
generation before inspecting the ring, and releases the pipe lock before
waiting or waking tasks. FAT helpers use synchronous block-I/O callbacks
under the existing filesystem lock. Packed directory records cross the ABI
as 32 raw bytes; no packed jihoo struct layout is assumed. File data uses a
small C-compatible metadata record, callbacks for one-sector and eight-sector
I/O, and a Rust-owned 4 KiB workspace. Rust validates handles and updates the
directory entry after writes, preserving partial-write and error behavior.
Slot generations and create/open/delete/list/format orchestration remain Rust.

The mixed build supplies `-I src/jihoo/runtime`, selecting a kernel `std.sys`
adapter for the standard jihoo `Vec`, `Box`, `String`, `Map`, and formatting
implementations. Their allocation/output callbacks currently use Rust ABI
bridges into the kernel heap and console. They do not use firmware allocation
or Linux services. File/process functions in this kernel-only adapter report
`ENOSYS` until the corresponding native OS interfaces are connected.

The port uses `-mno-red-zone -mgeneral-regs-only`, a custom panic handler,
and volatile device accesses. It does not use Linux syscalls or UEFI boot
services after boot. This is a partial kernel migration: scheduling,
storage, USB, the filesystem, and most boot/device code remain Rust.

Use Clang for `CC`: GCC cannot assemble the COFF architecture helpers. The
assembly build disables PIC explicitly so Nix's Linux Clang wrapper can be
used for the Windows/UEFI object target.

## Verification

With compiler commit `12f3827bd2574255934b7c730c1746ace14e5197`:

```sh
nix build path:../jihoo#jihoo --no-link --print-out-paths
bash tools/test-jihoo-heap.sh
bash tools/test-jihoo-frames.sh
bash tools/test-jihoo-paging.sh
bash tools/test-jihoo-pci.sh
bash tools/test-jihoo-pipe.sh
bash tools/test-jihoo-fat.sh
bash tools/test-jihoo-file-data.sh
bash tools/test-jihoo-runtime.sh
```

The scripts build the current adjacent compiler unless `JIHOO_BIN` is set.
Do not assume `../jihoo/out/jihoo` matches the checkout. Host tests run
the actual jihoo implementations through native C ABI objects at `-O0` and
`-O2`, with the kernel's register and red-zone options:

- Heap: all ten size classes, live-data preservation, free-list reuse,
  alignments from 1 through 4096, large-block coalescing in different free
  orders, slab fallback after arena exhaustion, invalid alignments, size
  overflow, allocation/reallocation failure, and 20,000 mixed operations.
  Failed reallocation preserves the original allocation. Boundary canaries
  check for writes outside the backing heap.
- Frames: 40-, 48-, and 64-byte firmware descriptor strides, reserved and
  empty descriptors, physical address zero, exhaustion, zero-count requests,
  and contiguous runs across descriptors and after gaps. The map is read-only.
  Pool tests also cover pre-commit failure, handoff of a partially used boot
  descriptor, LIFO reuse, preserved live-page contents, whole-page zeroing,
  and unchanged allocation counts after OOM.
- Paging: callback-based table allocation and zeroing, table reuse across
  mapping boundaries, leaf flags, permission failure at each level, 1 GiB
  and 2 MiB leaves, NX address masking, invalid PML4 huge-page flags,
  noncanonical addresses, range overflow, the highest canonical page, and
  retry after partial table-allocation failure. Range results are compared
  with the Rust reference as well as explicit expectations. Boot fixtures
  check UEFI memory-type filtering, empty descriptors, framebuffer tails,
  read-only firmware maps, and OOM during identity-map construction.
- PCI: mocked configuration ports check byte/word/dword extraction, maximum
  bus/device/function/offset addressing, writes, 32/64-bit BAR decoding,
  multifunction scanning and callbacks, indexed xHCI discovery through bus
  255, C device-record layout, absent-device results, and no configuration
  data writes during discovery.
- Pipes: both the Rust reference and the actual Rust/jihoo bridge pass the
  same host contract for short/empty transfers, clone/close counts, EOF,
  BrokenPipe, partial writes after reader closure, full/empty blocking,
  streaming across ring wraps, concurrent writers, and a close between the
  state check and sleep. A condition-variable scheduler substitute tests
  generation-based waiting and wait-key cleanup. Another 20,000 operations
  compare the jihoo ring with a `VecDeque` model and check boundary canaries.
- FAT: both filesystem variants run against an 8 MiB RAM disk at each
  optimization level, covering offset reads, append, sparse zero-filled
  gaps, truncate, stale handles, 21-byte UTF-8 names, a full root directory,
  full-disk failures and partial writes, and unchanged unrecognized volumes.
  Separate callback fixtures check FAT/sector boundaries, allocation at the
  final addressable entry, chain traversal and cyclic-chain release, injected
  read/write errors, exact packed directory bytes, and unchanged output on
  failed reads. These tests do not access host disks.
- File data: the actual Rust and jihoo filesystem variants run against the
  same mock block device. At both optimization levels, 289 cases compare
  returned results, file sizes, output-buffer and disk-image hashes, and
  exact I/O traces. They include 259 injected failure positions across
  reads/writes of empty and populated files, cluster/sector boundaries,
  append, sparse gaps, empty/overflowing writes, and malformed chains.
  Another 500 rounds compare whole-file and offset reads/writes with a byte
  vector model, checking untouched output tails. The existing RAM-disk tests
  additionally cover partial writes when the FAT is full. No host disk is
  involved.
- Kernel runtime: at `-O0` and `-O2`, twenty repeated collection checks grow,
  clone and sort a vector, own `Box` values in a string-keyed map, reallocate
  a string, and confirm every allocation is released. Object inspection
  rejects Linux syscall instructions and vector registers. The same check
  runs on the real kernel heap immediately after initialization; diskless
  and normal QEMU boots both print
  `jihoo kernel runtime: collections and cleanup passed`. The existing shell,
  eight-stage pipes, exec reclamation, local network fixtures and shutdown
  regressions passed in `/tmp/kaguya-jihoo-runtime-shell` and
  `/tmp/kaguya-jihoo-runtime-network`, using a snapshot of the disposable
  `/tmp/kaguya-jihoo-runtime-nvme.img` copy for normal boot.

See [the userspace README](../../user/jihoo/README.md) for smoke commands.
QEMU boot, framebuffer updates, shell input, gateway ping, and HTTP
fixtures passed with the jihoo checksum, PIC, port-I/O, drawing, and lock
implementations. The KEF parser and page-table creation/mapping/teardown
additionally passed diskless shell I/O and forty sequential execution and
reclamation checks. The heap growth entry point compiles, but these
allocation-free userspace programs do not directly exercise it.

The heap and boot-frame implementations additionally passed the diskless
shell I/O regression (including eight-stage pipes and forty sequential
executions with constant frame usage), normal NVMe boot, virtio display,
gateway ping, local HTTP streaming/redirect/error fixtures, and guest shutdown.
Disk checks used `/tmp/kaguya-jihoo-nvme.img`, a copy of `nvme.img`, with QEMU
`-snapshot`. No unrecognized disk is formatted automatically.

The frame-pool, paging/boot-map, and PCI ports are also covered by the same
diskless and normal-network regressions. PCI device selection, logging,
registration and enabling remain Rust; the diskless discovery function
only reads configuration registers.
The default Rust kernel also passed the normal NVMe/virtio/network/shutdown
regression with the same jihoo userspace disk. Current artifacts are in
`/tmp/kaguya-jihoo-memory-shell`, `/tmp/kaguya-jihoo-memory-network`, and
`/tmp/kaguya-rust-memory-network`.

An alternate EFI can be selected without replacing the normal artifact:

```sh
cargo build --target x86_64-unknown-uefi --target-dir target/rust-reference-check
python3 tools/gpu-smoke.py --kernel-image target/rust-reference-check/x86_64-unknown-uefi/debug/os.efi --network --disk-image /tmp/kaguya-jihoo-nvme.img --shutdown --output /tmp/kaguya-rust-memory-network
```

Prepare the disposable disk using the userspace instructions first. The
recorded comparison used a separate copy, `/tmp/kaguya-jihoo-memory-nvme.img`.

The pipe/FAT stage passed diskless shell I/O, eight-stage pipelines, forty
sequential exec/frame-reclamation checks, normal NVMe boot, ping, HTTP and
shutdown checks. The network fixture now also streams a 10,200-byte body
through `wget | cat | cat > piped`, exceeding each pipe's 4 KiB capacity,
then reads the file back and compares the full repeated body. Saving the
output avoids interleaved diagnostics from concurrent processes in the
serial log. Both the jihoo and default Rust kernels pass this fixture.
Current artifacts are in
`/tmp/kaguya-jihoo-fat-shell`, `/tmp/kaguya-jihoo-fat-network`, and
`/tmp/kaguya-rust-fat-network`; normal boots use a snapshot of the disposable
copy `/tmp/kaguya-jihoo-fat-nvme.img`.

The file-data stage passed the same diskless and normal network checks on
the new compiler, including KEF loading through the jihoo whole-file reader,
file overwrite/append/redirection, eight-stage pipes, forty sequential execs
with constant frame use, and the 10,200-byte HTTP pipeline/file readback.
Artifacts are in `/tmp/kaguya-jihoo-file-data-shell` and
`/tmp/kaguya-jihoo-file-data-network`; normal boots use a snapshot of the copy
`/tmp/kaguya-jihoo-file-data-nvme.img`. The Rust reference build and network
regression use `target/rust-reference-check` and
`/tmp/kaguya-rust-file-data-network`.

Host tests check allocator edge cases; QEMU checks the UEFI ABI, boot,
locking, and integration with real kernel allocations. Physical hardware,
concurrent allocator stress on multiple CPUs, and the ring-3 compatibility
syscall stubs in the Rust heap bridge have not been separately exercised.
Heap initialization requires the existing page-aligned region and page-multiple
size supplied by the kernel. Firmware descriptors must satisfy the existing
UEFI memory-map validity requirements.

## Compiler issues and remaining work

[Issue #19](https://github.com/jihoolang/jihoo/issues/19) is fixed in
`84d4a54`. The minimal static-pointer-array null assignment (constant and
loop indices), local-array control, and separately typed pointer control
all pass `check --lib`; the compiler's `tests/run/static_null.jh` execution
also passes. No typed-null workaround is used in the allocator.

The allocator draft also contained ordinary porting errors: `8 +` a `u64`
value inferred the first operand as `i64`, and an untyped `if` expression
inferred its zero branch as `i64`. Using the typed operand first and an
explicit `u64` result fixes these errors. They are not new compiler bugs.
`heap.jh` and its locked Rust `GlobalAlloc` bridge are now selected only by
`jihoo-kernel`; the default allocator remains the Rust reference.

During the range-check port, existing Rust boundary problems were fixed in
both implementations: noncanonical aliases are rejected, range iteration
cannot wrap past the highest page, NX is masked before following a table
pointer, 1 GiB leaves are recognized, and a PML4 huge-page flag is rejected.
Empty ranges consistently succeed. Mapping over a huge leaf fails before
following it as a table. These are kernel correctness fixes, not compiler
problems.

The FAT allocator also had an existing capacity bug: its upper bound could
truncate to `u16` and scan beyond the actual 16,384-entry FAT on the normal
1 GiB image. Both implementations now clamp allocation to the data region
and the table's addressable entries, reporting `NoSpace` at capacity.
This does not change the on-disk layout or enable automatic formatting.

Scheduling, syscall dispatch, file descriptor tables, FAT16
create/open/delete/list/format orchestration, PCI device registration and
storage/USB/network driver state machines still require migration.
Pipe ownership/locking and scheduler waits, filesystem slot generations,
and block-I/O backends remain in Rust bridges.
Rust ABI bridges, address-space bookkeeping, frame-source objects, CR3/TLB
assembly, boot assembly, and UEFI setup also remain. This is not a completed
kernel port.

## Issue #20 resolution

[Issue #20](https://github.com/jihoolang/jihoo/issues/20) is fixed in compiler
commit `12f3827bd2574255934b7c730c1746ace14e5197`. Calling a `fnptr` field
through a raw struct pointer now checks in both native and UEFI profiles.
The pointer case and three executable controls each produce `42`; the
compiler's own regression also passes, and a pointer call outside `unsafe`
remains correctly rejected. See the
[reproduction and controls](../../tools/jihoo-regressions/fnptr-field/README.md).

The excluded file-data fragment has been incorporated into `fat.jh`, connected
through `fat.rs`, and removed from the draft directory. No explicit-dereference
or local-callee workaround was needed. The original Rust implementations
remain selected by builds without `jihoo-kernel`.

The failure tests preserve existing API semantics, including zero-filled
tails for premature chain endings in the legacy whole-file reader and an
error for the same condition in offset-based reads. Crash recovery, atomic
metadata updates, and physical-hardware error injection are not verified
by this stage.

If a new language/compiler problem appears, reduce it, compare control
cases, file an issue in `jihoolang/jihoo` with `gh`, and stop migration.

## Issue #21 resolution and standalone diagnostic

[Issue #21](https://github.com/jihoolang/jihoo/issues/21) is fixed in compiler
`c8014f4b1d3bbc22bfc4c267193b47737268bf1a`. Matching external declarations and
imported exported definitions emit native and UEFI objects. Executable
controls print `42`, and mismatching signatures and duplicate definitions
are rejected. See the [regression cases](../../tools/jihoo-regressions/extern-export/README.md).

```sh
CC=clang bash tools/build-jihoo-kernel.sh
python3 tools/gpu-smoke.py --hardware-test --kernel-image target/jihoo-kernel/hardware-test/os.efi --output /tmp/kaguya-standalone-diagnostic
```

This build invokes jihoo, Clang for assembly, and the linker, without Cargo
or rustc. It produces a separate image under `target/jihoo-kernel/` and
preserves the ordinary mixed/Rust build. QEMU passes ExitBootServices,
GOP/memory-map validation, and collections/cleanup on the jihoo heap, then
halts at `HARDWARE TEST READY`. The console checkpoint below adds visible
GOP text. Replacement page tables, interrupts, drivers, scheduling, and
userspace are not yet connected to this standalone image. No physical hardware has been tested. Continue using the
[full-port checklist](../../docs/jihoo-full-port.md).

## Issue #22 resolution and standalone text console

[Issue #22](https://github.com/jihoolang/jihoo/issues/22) is fixed in compiler
`d0b3d3182a33e602ef687561633d6e2be9d0472b`, freshly built through Nix. Explicit-list
and repeated nonzero static arrays retain their data in native execution
and UEFI IR/objects; const/local/scalar/zeroed controls and the compiler's
own aggregate initializer regression pass. See the
[reproductions](../../tools/jihoo-regressions/static-initializer/README.md).

The text console (`console.jh`, `font/basic.jh`) is connected to the separate
standalone EFI. Standard jihoo output writes to serial and an interrupt-safe
GOP console after ExitBootServices. It renders ASCII, converts GOP RGB/BGR
formats, clamps viewports, handles wrapping/backspace, and supplies terminal
cell/region operations. The existing Basic Latin font bytes are vendored
with their MIT license, so the standalone build needs no Rust font crate.
The mixed kernel continues using its existing console ownership.

```sh
CC=clang bash tools/test-jihoo-console.sh
CC=clang bash tools/build-jihoo-kernel.sh
python3 tools/gpu-smoke.py --hardware-test --kernel-image target/jihoo-kernel/hardware-test/os.efi --output /tmp/kaguya-jihoo-console
python3 tools/gpu-smoke.py --hardware-test --resolution 2560x1440 --kernel-image target/jihoo-kernel/hardware-test/os.efi --output /tmp/kaguya-jihoo-console-1440p
```

The C host harness requires no Rust and passes at `-O0` and `-O2`: exact
glyph pixels, both GOP color formats, stride/tail guards, unsupported
characters, clipped viewports, wrapping/backspace/clear, terminal cells,
zero-width regions, and coordinate overflow. Kernel-runtime collection tests
also pass at both optimization levels on the new compiler. QEMU reaches the
ready marker at 1280x800 and 2560x1440; the captured screen visibly contains
the diagnostic text. The standalone image still halts before replacement
page tables, CPU tables/interrupts, drivers, scheduling, or userspace.
No physical hardware has been tested. Continue CPU initialization using the
[full-port checklist](../../docs/jihoo-full-port.md); full Rust removal is
still incomplete.

## Standalone CPU/page-table checkpoint

With compiler `d0b3d3182a33e602ef687561633d6e2be9d0472b`:

```sh
CC=clang bash tools/test-jihoo-cpu-tables.sh
CC=clang bash tools/build-jihoo-kernel.sh cpu-test
python3 tools/gpu-smoke.py --cpu-test --output /tmp/kaguya-jihoo-cpu
python3 tools/gpu-smoke.py --cpu-test --resolution 2560x1440 --output /tmp/kaguya-jihoo-cpu-1440p
```

This separate EFI owns GDT/TSS/IDT storage in jihoo and uses assembly only
for privileged instructions and interrupt register frames. Packed table
records are written byte by byte, avoiding assumptions about jihoo packed
struct layout. The C harness passes at `-O0` and `-O2`, including unaligned
buffers, exact segment/TSS/gate bytes, full-width addresses, all IST-bit
inputs, and boundary canaries. Each of 64 CPU slots has a separate reserved
double-fault stack; the Rust reference used one shared stack. AP startup and
actual double-fault recovery are not yet exercised.

The standalone frame-pool wrapper uses the jihoo interrupt-safe lock. It
builds and switches to the existing jihoo identity page tables without Rust
callbacks. QEMU confirms CR3 readback, kernel-only mapping permissions,
recycled frame zeroing/accounting, a returning `int3`, and ten real PIT IRQs.
The common entry code now lives in `standalone/platform.jh`; the original
`hardware-test` entry still stops before CPU/page-table/PIC initialization
and its regression passes in `/tmp/kaguya-jihoo-cpu-diagnostic`.

Both CPU-test resolutions pass. Storage, USB, network, virtio GPU, APs,
scheduling, syscall entry and userspace are not started in this mode.
This is an incremental validation artifact, not the complete OS or the
physical-hardware diagnostic milestone. Continue with CPU-local/syscall
state, address-space ownership, KEF loading, and scheduling.

## Standalone address ownership, KEF and ring-3 checkpoint

```sh
CC=clang bash tools/test-jihoo-space.sh
CC=clang bash tools/build-jihoo-kernel.sh ring3-test
python3 tools/gpu-smoke.py --ring3-test --output /tmp/kaguya-jihoo-ring3
python3 tools/gpu-smoke.py --ring3-test --resolution 2560x1440 --output /tmp/kaguya-jihoo-ring3-1440p
```

On compiler `d0b3d31`, `space.jh` supplies atomic reference-counted address
space ownership, mapping/heap locks, and last-owner page reclamation.
`kef.jh` owns the formerly Rust loader orchestration: code/BSS pages,
nonexecutable stack pages, an unmapped stack guard, and aligned argument
copying. The C host tests run twenty rounds at both optimization levels,
checking shared lifetime, duplicate/unaligned/kernel mapping rejection,
heap rounding/overflow/OOM, root/node/partial-table allocation failures,
exact initialized data and BSS, page flags, argument contents and guards.
All frames and ownership allocations are released after each round.

The separate ring-3 EFI embeds `user/jihoo/verify.jh` as a KEF using Clang
assembly; neither its kernel nor user build invokes Cargo or rustc. It sets
up CPU-local GS state, TSS RSP0, EFER/STAR/LSTAR/SFMASK and a syscall assembly
boundary that translates the user SysV register ABI to the kernel UEFI ABI.
The test dispatcher supplies exit, console write, heap growth and retired
allocation stubs; it is not the full production syscall dispatcher.

QEMU passes twenty normal executions with stable frame/heap counts,
argument delivery, syscall return, exact invalid-argument errors for null,
kernel, guard-crossing and overflowing user ranges, and heap growth/data
writes. A deliberate ring-3 write to kernel-only memory faults, and an
attempt to execute the NX heap faults with error `0x15`; both executions
are contained and reclaimed. A timed user-mode loop receives PIT IRQs and
returns through the interrupt frame. Both GOP resolutions pass.

This harness uses one isolated bootstrap continuation for program exit;
it does not supply scheduling, preemption, threads, descriptor ownership,
USB input, filesystems, storage, networking or AP startup. The normal Rust
and mixed kernels are still separate. Continue by porting shared ownership,
scheduler handoffs/waits and file/pipe ownership before the full shell.
