# kaguyaOS

A hobby x86_64 UEFI operating system written in [Jihoo](https://github.com/jihoolang/jihoo), with assembly at CPU and syscall boundaries.

The supported kernel, all seven userspace programs, and disk-image tool build without `rustc`, Cargo, `rustup`, or `rust-lld`. The former Rust implementation remains in the repository for comparison and is excluded from the normal build.

## Build and run

Keep the Jihoo checkout at `../jihoo`, then enter the development environment:

```sh
nix develop
./build_insert_run.sh
```

The scripts build the adjacent compiler with Nix. Set `JIHOO_BIN=/path/to/jihoo` to use an existing compiler instead. The port is verified with Jihoo commit `ea3c2e0b87f8177058b4a8dcfa97588960cf6d96`, including the fixes for issues #23 and #24.

Manual builds need Jihoo, Clang, LLVM `ld.lld`, Python 3, Bash, QEMU and OVMF. Set `OVMF_BIOS` to the firmware file; `CC` and `JIHOO_LD` can select the assembler and userspace linker. The Nix shell supplies these tools and does not install or initialize Rust.

Individual steps:

```sh
./user/build.sh       # target/jihoo-user/{init,ls,cat,write,rm,ping,wget}.kef
./user/insert.sh      # insert those programs into nvme.img
./build.sh           # target/kaguya/os.efi and esp/EFI/BOOT/BOOTX64.EFI
./run.sh             # rebuild kernel and boot QEMU
```

`KAGUYA_USER_OUTPUT` changes the userspace output directory. `KAGUYA_DISK_IMAGE` changes the image used by insertion and QEMU. `GPU=gop` selects GOP graphics, and `KAGUYA_CPUS=4` changes the virtual CPU count.

A missing host image is created as a sparse 1 GiB regular file and explicitly initialized by the Jihoo image tool. An existing image must already contain the custom KAGFAT16 layout; an unrecognized image fails without formatting. The kernel also refuses to format unrecognized NVMe volumes during boot. These scripts never install onto physical disks.

The host tool builds with `bash tools/build-kef-tool.sh`. Its executable is `target/jihoo-tools/kef-tool`, supporting `check`, `list`, `insert`, `extract`, `delete`, and explicit `format`. This custom layout is not a standard FAT16/FAT32 boot partition.

The default QEMU machine has two CPUs, virtio-vga, NVMe, an xHCI USB keyboard, and an Intel E1000 NIC with user-mode networking.

## Features

- UEFI handoff, GOP output, memory-map validation and runtime shutdown.
- GDT, per-CPU TSS and fault stacks, IDT, ring-3 isolation and syscall IDs `0..41`.
- ACPI RSDP/XSDT/RSDT/MADT parsing, LAPIC/IOAPIC setup and INIT-SIPI-SIPI AP startup.
- Per-CPU run queues, work stealing, timer preemption, wake IPIs, blocking sleep/wait and safe stack retirement.
- Interrupt-safe locks, shared/weak ownership and generation-based event waits.
- Per-process page tables, NX mappings, frame reclamation and a userspace heap of up to 256 MiB.
- NVMe I/O and KAGFAT16 create/read/write/delete/list, descriptor tables and shared file offsets.
- Bounded blocking pipes, inherited standard descriptors and shared-address-space threads.
- xHCI boot-protocol USB keyboard input.
- Software desktop/window rendering, modern PCI virtio-gpu 2D, dirty-region presentation and GOP fallback.
- E1000 IRQ receive, ARP, IPv4, ICMP, UDP sockets, DNS A resolution and client TCP sockets.
- HTTP downloads with streaming, redirects, file output and bounded-pipe backpressure.

The shell supports `help`, `clear`, `shutdown`, `ls`, `cat`, `write`, `rm`, `exec`, `ping`, and `wget`, plus pipelines of up to eight commands and `<`, `>`, `>>` redirection:

```text
kaguya> write note hello
kaguya> cat note | cat
kaguya> ls > listing
kaguya> exec cat.kef listing
kaguya> ping 10.0.2.2
kaguya> wget http://example.com > page.html
kaguya> shutdown
```

## Verification

To verify all supported build paths while trapping any Rust tool invocation:

```sh
bash tools/check-rust-free-build.sh
```

This builds the kernel, seven programs, Jihoo host tool and both hardware images, prepares an image under `target/rust-free-check/`, and verifies that an existing invalid image remains byte-for-byte unchanged.

Boot the ordinary kernel with a disposable QEMU snapshot and a local HTTP fixture:

```sh
python3 tools/gpu-smoke.py --network --cpus 4 --require-smp --shutdown
python3 tools/gpu-smoke.py --backend gop --shutdown
```

Use `--disk-image PATH` to select another prepared image. Smoke tests save screenshots and serial logs under `--output PATH` (default `/tmp/kaguya-gpu-smoke`) and do not persist guest writes.

Additional syscall/thread/preemption checks use an optional verification program:

```sh
./user/build.sh verify
target/jihoo-tools/kef-tool insert target/rust-free-check/nvme.img target/jihoo-user/verify.kef verify.kef
python3 tools/gpu-smoke.py --disk-image target/rust-free-check/nvme.img --abi --network --cpus 4 --require-smp --shutdown
```

Native C/Jihoo tests cover collections/ownership, address spaces, filesystem failures and stale handles, descriptor lifetimes, concurrent pipes, task switching, device rings, graphics, ACPI and network parsing/state. Representative commands:

```sh
bash tools/test-jihoo-shared.sh
bash tools/test-jihoo-space.sh
bash tools/test-jihoo-filesystem.sh
bash tools/test-jihoo-file-table.sh
bash tools/test-jihoo-pipe-owner.sh
bash tools/test-jihoo-task.sh
bash tools/test-jihoo-usb.sh
bash tools/test-jihoo-nvme.sh
bash tools/test-jihoo-display.sh
bash tools/test-jihoo-acpi.sh
bash tools/test-jihoo-udp.sh
```

Older Rust differential harnesses are optional reference checks. They are not required to build or validate the supported Jihoo OS. The migration history and recorded checkpoints are in [docs/jihoo-full-port.md](docs/jihoo-full-port.md).

## Physical hardware milestones

Use the isolated diagnostic image for a first hardware boot:

```sh
./tools/build-hardware-test.sh
python3 tools/gpu-smoke.py --hardware-test
```

Its artifact is `target/hardware-test/esp/EFI/BOOT/BOOTX64.EFI`. It validates GOP, UEFI ExitBootServices, the memory map and kernel collection/heap cleanup, then halts at `HARDWARE TEST READY`. It does not start PCI devices, replacement page tables, timers, APs or userspace.

After that diagnostic succeeds, build the separate diskless shell:

```sh
./tools/build-hardware-shell.sh
python3 tools/gpu-smoke.py --hardware-shell --shell-io --shutdown
python3 tools/gpu-smoke.py --hardware-shell --resolution 2560x1440
python3 tools/gpu-smoke.py --hardware-shell --usb-mouse --keyboard-stress
python3 tools/gpu-smoke.py --hardware-shell --no-keyboard
```

Its artifact is `target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI`. Copy the selected artifact to `EFI/BOOT/BOOTX64.EFI` on a prepared FAT32 UEFI boot USB, using firmware settings that permit an unsigned image. The build scripts do not write the USB.

The diskless shell installs GDT/IDT, page tables and a cooperative BSP scheduler. It embeds `init`, `ls`, `cat`, `write`, and `rm` in a fresh temporary 8 MiB RAM filesystem. External interrupts remain disabled in kernel and userspace: USB is polled, and storage, network, timers and APs remain off. Polling can keep one CPU busy. Changes disappear on shutdown or reboot.

Connect a boot-protocol wired USB keyboard directly before boot. Only the selected xHCI controller is enabled; enumeration stops after the first usable keyboard. Select another controller with `KAGUYA_XHCI_INDEX=1 ./tools/build-hardware-shell.sh`. Hubs, hotplug, NKRO-only keyboards and endpoint recovery are unsupported. xHCI supports firmware ownership handoff, 32/64-byte contexts, scratchpad pages and bounded port reset waits; real controllers may need additional quirks.

The former Rust diskless shell was physically tested at 2560×1440 in September 2026, including USB input, file utilities and shutdown. The replacement Jihoo images have been tested in QEMU; they still need physical-hardware validation. Normal NVMe/network/SMP builds are separate from these restricted hardware milestones.

## Source map

| Area | Jihoo implementation |
|---|---|
| UEFI boot and architecture glue | `src/jihoo/standalone/` |
| Scheduler and ownership | `task.jh`, `shared.jh`, `kernel_sync.jh` |
| Memory and executable loading | `heap.jh`, `frame.jh`, `paging.jh`, `space.jh`, `kef.jh` |
| Files, descriptors and pipes | `filesystem.jh`, `fat.jh`, `file_table.jh`, `pipe_owner.jh` |
| Hardware | `pci.jh`, `nvme.jh`, `xhci.jh`, `virtio_gpu.jh`, `e1000.jh`, `acpi.jh` |
| Graphics and console | `display.jh`, `console.jh`, `font/basic.jh` |
| Network protocols | `net_base.jh`, `net_wire.jh`, `net_udp.jh`, `net_dns.jh`, `net_tcp.jh` |
| Kernel standard-library adapter | `src/jihoo/runtime/std/sys.jh` |
| Userspace | `user/jihoo/` |
| Host image tool | `tools/kef-tool/main.jh` |

Paths without a directory prefix in the table are under `src/jihoo/`. See [docs/syscall.md](docs/syscall.md) for the userspace ABI.

## Limitations

This is an experimental OS. Networking uses the static QEMU IPv4 configuration; DNS has one outstanding fixed-ID query. TCP is client-only, without listening sockets, out-of-order reassembly, congestion control or TIME_WAIT. HTTP is supported; HTTPS/TLS is not. Virtio desktop output is bounded to 1920×1080, while the direct GOP diagnostic/diskless console follows the firmware resolution. Kernel/userspace code uses general registers only; per-task SIMD state is not saved. ACPI SMP supports up to 64 enabled xAPIC CPUs through the first IOAPIC. Physical NIC/NVMe/GPU and multiprocessor hardware remain unverified.
