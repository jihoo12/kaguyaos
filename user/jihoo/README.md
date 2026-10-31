# Jihoo userspace

All seven normal programs are Jihoo: `init`, `ls`, `cat`, `write`, `rm`, `ping`, and `wget`. They use the KEF executable format and syscall IDs `0..41`. `kaguya.jh` supplies syscall wrappers and panic handling; `runtime.S` supplies entry, syscall, timestamp and memory helpers.

```sh
./user/build.sh
./user/insert.sh
./run.sh
```

The compiler comes from `../jihoo` through Nix, or `JIHOO_BIN`. Manual builds need Clang and LLVM `ld.lld`; `CC` and `JIHOO_LD` select them. Outputs are in `target/jihoo-user/` by default, overridable with `KAGUYA_USER_OUTPUT`. `KAGUYA_DISK_IMAGE` selects the image used by insertion and QEMU. Existing images must pass recognition; only a new host image is automatically initialized.

`./user/build.sh verify` builds the optional syscall/thread/preemption probe. `bash user/build-jihoo.sh PROGRAM...` remains available for direct program selection. Code uses `-mno-red-zone -mgeneral-regs-only`, since the kernel does not save task SIMD state. The old `user/src/*.rs` and checked-in `user/*.kef` are reference artifacts and are excluded from normal builds and image insertion.

The shell supports eight-stage pipelines, input/output/append redirection, inherited standard descriptors, explicit `exec` arguments and syntax-error handling. Native tests cover descriptor and pipe ownership. QEMU verifies file creation/read/overwrite/delete, constant frame use across 40 executions, shared-address-space threads, legacy ABI calls, timer preemption, gateway ping and local HTTP fixtures. HTTP tests include relative redirects, streaming a 10,200-byte body through bounded pipes, file output, errors and truncated responses. HTTPS is rejected explicitly.

The separate hardware shell embeds the Jihoo shell and four file utilities in a temporary 8 MiB RAM filesystem, with a polled USB keyboard and storage/network/timers/APs off:

```sh
./tools/build-hardware-shell.sh
python3 tools/gpu-smoke.py --hardware-shell --shell-io --shutdown
```

See the [root README](../../README.md) for normal NVMe/network checks and physical-hardware limitations, and the [migration history](../../docs/jihoo-full-port.md) for the compiler fixes used during the port.
