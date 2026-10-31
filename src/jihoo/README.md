# Jihoo kernel

The normal build now compiles this kernel directly to UEFI with Jihoo and assembly. It does not compile or link the Rust kernel:

```sh
./build.sh
```

The public artifact is `target/kaguya/os.efi`; `esp/EFI/BOOT/BOOTX64.EFI` is the boot copy. The adjacent `../jihoo` compiler is selected through Nix unless `JIHOO_BIN` is set. See the [root README](../../README.md) for prerequisites, disk preparation and QEMU commands.

`standalone/platform.jh` owns the firmware handoff and initialization. Its normal path connects owned collections, memory/page tables, per-CPU interrupt/syscall entry, ACPI/APIC/SMP, task queues, file tables, NVMe/KAGFAT16, xHCI keyboard input, display/virtio-gpu and E1000/IPv4/UDP/DNS/TCP. Assembly provides instruction and ABI boundaries; there are no Rust ownership or runtime bridges in this build.

`task.jh` uses stable boxed task records, per-CPU queues and an assembly handoff acknowledgement. A task cannot run elsewhere before its outgoing stack pointer is saved. Waiters observe termination after the retired stack and address-space reference are reclaimed. Shared/weak owners protect descriptor and socket lifetimes; locks disable interrupts. New kernel top-level mappings are established before creating user address spaces.

The kernel `std.sys` adapter selects the Jihoo heap and console after ExitBootServices. Its unsupported host-style file/process interfaces return ENOSYS; actual OS files and task syscalls use the kernel modules. Both kernel and userspace use `-mno-red-zone -mgeneral-regs-only`; task SIMD state is not saved.

Separate build modes preserve restricted diagnostics:

```sh
bash tools/build-jihoo-kernel.sh hardware-test
bash tools/build-jihoo-kernel.sh cpu-test
bash tools/build-jihoo-kernel.sh ring3-test
bash tools/build-jihoo-kernel.sh filesystem-test
bash tools/build-jihoo-kernel.sh scheduler-test
bash tools/build-jihoo-kernel.sh shell
bash tools/build-jihoo-kernel.sh hardware-shell
bash tools/build-jihoo-kernel.sh nvme-shell
```

`hardware-test` stops before device initialization. `hardware-shell` is the polled diskless BSP image with external interrupts, timers, APs, network and storage off. `shell` is a separate diskless timer/scheduler checkpoint. `nvme-shell` supplies the complete normal boot and is selected by `build.sh`. Internal artifacts are under `target/jihoo-kernel/MODE/`; public hardware wrappers copy their EFI to `target/hardware-{test,shell}/esp/EFI/BOOT/BOOTX64.EFI`.

The former `jihoo-kernel` Cargo feature remains only for optional comparison against the old Rust bridges. It is excluded from the supported workflow. The [migration record](../../docs/jihoo-full-port.md) retains the earlier staged checkpoints and compiler fixes.

Verified with compiler `ea3c2e0b87f8177058b4a8dcfa97588960cf6d96`: native tests at O0/O2, Rust-tool-blocked builds, QEMU graphics/GOP, diskless shell, filesystem/pipes/constant-frame execution, four-CPU AP scheduling, networking and guest shutdown. See the root README for precise limitations and physical-hardware status.
