# Jihoo kernel migration

`CC=clang cargo build --target x86_64-unknown-uefi --features jihoo-kernel` compiles
`kernel.jh` to COFF and links it into the Rust UEFI kernel. `build.rs`
selects `JIHOO_BIN`, the adjacent `../jihoo` checkout through Nix, or
`jihoo` on PATH. Clang assembles `arch.S`.

The feature currently selects jihoo implementations of:

- Internet and IPv4 transport checksums.
- PIC initialization, interrupt acknowledgement, and IRQ unmasking.
- Pixel and clipped rectangle writes.
- Interrupt-safe spinlock acquisition, try-lock, and release.
- KEF header parsing and validation.
- Process page-table creation, user-page mapping, heap mapping, and teardown.
- Kernel heap allocation, free lists, large-block splitting/coalescing, and reallocation.
- Boot-time single and contiguous frame allocation from the UEFI memory map.

The bridge uses the UEFI calling convention. `arch.S` implements port I/O,
interrupt flag operations, and pause instructions. Frame allocation and
kernel panic reporting call into the remaining Rust kernel. The global frame
pool and its lock remain Rust; its boot-map allocation now calls jihoo. Existing
display locks and dirty-region tracking surround drawing operations.
Without the feature, the Rust reference implementations remain active.

The port uses `-mno-red-zone -mgeneral-regs-only`, a custom panic handler,
and volatile device accesses. It does not use Linux syscalls or UEFI boot
services after boot. This is a partial kernel migration: scheduling,
storage, USB, the filesystem, and most boot/device code remain Rust.

Use Clang for `CC`: GCC cannot assemble the COFF architecture helpers. The
assembly build disables PIC explicitly so Nix's Linux Clang wrapper can be
used for the Windows/UEFI object target.

## Verification

With compiler commit `84d4a54a0de4a8c12f7584ee390e64083ac3a8b8`:

```sh
nix build path:../jihoo#jihoo --no-link --print-out-paths
bash tools/test-jihoo-heap.sh
bash tools/test-jihoo-frames.sh
```

The scripts build the current adjacent compiler unless `JIHOO_BIN` is set.
Do not assume `../jihoo/out/jihoo` matches the checkout. Both host tests run
the actual jihoo allocator code through native C ABI objects at `-O0` and
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

Host tests check allocator edge cases; QEMU checks the UEFI ABI, boot,
locking, and integration with real kernel allocations. Physical hardware,
concurrent allocator stress on multiple CPUs, and the ring-3 compatibility
syscall stubs in the Rust heap bridge have not been separately exercised.
Heap initialization requires the existing page-aligned region and page-multiple
size supplied by the kernel. Firmware descriptors must satisfy the existing
UEFI memory-map validity requirements.

## Issue #19 and remaining work

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

Next memory work includes the global recycled-frame pool, kernel page-table
walking/mapping and user-pointer permission checks. Scheduling, syscall
dispatch, files/pipes, FAT16, PCI discovery and storage/USB/network driver
state machines still require migration. Rust ABI bridges, boot assembly,
and UEFI setup also remain. This is not a completed kernel port.

If a new language/compiler problem appears, reduce it, compare control
cases, file an issue in `jihoolang/jihoo` with `gh`, and stop migration.
