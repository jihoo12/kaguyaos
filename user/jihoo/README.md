# Jihoo userspace port

Jihoo versions of all seven programs are available: `init`, `ls`, `cat`,
`write`, `rm`, `ping`, and `wget`. They use the existing syscall ABI and KEF
loader. `kaguya.jh` supplies platform wrappers and panic handling;
`runtime.S` supplies entry, syscall, timer-read, and memory helpers.

The normal build remains Rust. Select the jihoo programs explicitly during
this staged migration. The optional kernel port is documented in
[src/jihoo/README.md](../../src/jihoo/README.md).

## Build

Inside `nix develop`:

```sh
bash user/build-jihoo.sh init ls cat write rm ping wget
```

The script builds `../jihoo` with Nix when that checkout exists. Set
`JIHOO_BIN` to select an already built compiler. Outside Nix, provide
`cc` and `ld.lld` on PATH, or set `CC` and `JIHOO_LD`.

Outputs go to `target/jihoo-user/`, with objects in its `.jihoo` directory.
`KAGUYA_USER_OUTPUT` selects another directory. Building does not insert
programs into a disk image or replace normal Rust outputs. The compiler
uses `-mno-red-zone -mgeneral-regs-only`, since the kernel does not preserve
per-task SIMD state.

## Diskless QEMU check

```sh
CC=clang KAGUYA_USER_LANGUAGE=jihoo KAGUYA_KERNEL_LANGUAGE=jihoo bash tools/build-hardware-shell.sh
python3 tools/gpu-smoke.py --hardware-shell --shell-io --shutdown --output /tmp/kaguya-jihoo-shell
```

This boots the jihoo shell and four file utilities in a temporary RAM
filesystem. It does not write to physical disks or `nvme.img`. Omit
`KAGUYA_KERNEL_LANGUAGE=jihoo` to use the Rust kernel implementations, or
omit both language variables for the original Rust build.

## Network QEMU check

Prepare a disposable disk containing all seven jihoo binaries:

```sh
cargo build --manifest-path tools/kef-tool/Cargo.toml
cp --reflink=auto --sparse=always nvme.img /tmp/kaguya-jihoo-nvme.img
for program in init ls cat write rm ping wget; do
    tools/kef-tool/target/debug/kef-tool insert /tmp/kaguya-jihoo-nvme.img "target/jihoo-user/$program.kef" "$program.kef"
done
CC=clang cargo build --target x86_64-unknown-uefi --features jihoo-kernel
python3 tools/gpu-smoke.py --network --disk-image /tmp/kaguya-jihoo-nvme.img --shutdown --output /tmp/kaguya-jihoo-network
```

QEMU uses a snapshot of the copy. The HTTP fixture runs locally on the
host; no external HTTP service is needed.

## Verified state

Rechecked with compiler commit `12f3827bd2574255934b7c730c1746ace14e5197`
(previously verified with `84d4a54a0de4a8c12f7584ee390e64083ac3a8b8`):

- All seven programs compile and link as KEF files.
- The shell passes file creation, reading, overwrite, deletion, pipes
  through eight stages, input/output/append redirection, `exec` arguments,
  and syntax-error handling.
- Forty sequential executions use a constant number of physical frames.
- Gateway ping, HTTP bodies larger than the receive buffer, relative
  redirects with queries, file output, HTTP errors, truncated responses,
  HTTPS rejection, and guest shutdown pass. The local fixture does not
  exercise DNS hostname resolution.
- A 10,200-byte HTTP body also passes through `wget | cat | cat > piped`
  and file readback, exercising 4 KiB pipes in both kernels.

The checks exposed an existing Rust-kernel failure with eight concurrent
programs: kernel-stack allocation failure was unchecked. That path now
returns a spawn error, and the diagnostic kernel heap is 1 MiB to support
the shell's advertised eight-stage pipeline limit.

[Jihoo issue #19](https://github.com/jihoolang/jihoo/issues/19) is fixed and
its minimal reproducer and controls pass with the new compiler. The kernel
heap allocator and boot-time frame allocation are now connected through
`jihoo-kernel`. The kernel also uses jihoo frame recycling, boot/kernel page-table
mapping, user-range checks, and PCI configuration/scanning/discovery, with
host edge-case tests and QEMU regression checks. See
[the kernel README](../../src/jihoo/README.md) for tested coverage and the
remaining Rust kernel work. The default build remains Rust.

The kernel feature additionally selects jihoo pipe buffering/end counts and FAT
entry/cluster/root-directory helpers. Descriptor ownership and scheduling
waits remain in Rust bridges, as do filesystem metadata orchestration
and formatting policy. Disk regressions use disposable copies
and QEMU snapshots; unrecognized disks are not automatically formatted.

[Jihoo issue #20](https://github.com/jihoolang/jihoo/issues/20) is fixed in
`12f3827`. Its pointer-call reproducer and controls now pass. The kernel
feature selects jihoo whole-file/offset reads and file data writes, including
sparse gaps and partial-write accounting. Rust retains buffer ownership,
handle checks, and directory metadata commits. Native failure/model tests
and QEMU diskless/network regressions pass; create/open/delete/list/format
orchestration and other kernel work remain to be ported. See the kernel README
for verification coverage and remaining work.

The full Rust-free migration now has a kernel standard-library adapter for
jihoo collections and formatting. Its mixed-kernel host and QEMU checks pass.
Compiler [issue #21](https://github.com/jihoolang/jihoo/issues/21) is fixed in
`c8014f4`. A standalone jihoo UEFI diagnostic now builds without Rust tools
and boots in QEMU through ExitBootServices and heap/collection checks. This
image does not run userspace yet. The ordinary kernel remains Rust; see the
kernel README and [full-port checklist](../../docs/jihoo-full-port.md).

Compiler [issue #22](https://github.com/jihoolang/jihoo/issues/22) is fixed
in `d0b3d31`. Minimal/control cases and the native C console tests pass.
The standalone jihoo diagnostic now shows visible text after ExitBootServices
and passes QEMU at 1280x800 and 2560x1440, without Cargo, rustc, or a Rust font
crate. This image still does not run userspace; the ordinary Rust and mixed
kernel checkpoints remain separate. CPU initialization and the remaining
full-port checklist continue next.
