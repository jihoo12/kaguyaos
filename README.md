# kaguyaOS

A hobby operating system written in Rust for x86_64 UEFI.

kaguyaOS currently boots into userspace on an SMP system, runs a preemptive scheduler, mounts a FAT16 filesystem from NVMe, handles USB keyboard input through xHCI, and provides IRQ-driven IPv4 networking with DNS resolution. The current development milestone is a framebuffer-based graphical desktop/window with the shell rendered inside the window client area.

![Rust](https://img.shields.io/badge/language-Rust-orange)
![Platform](https://img.shields.io/badge/platform-x86__64--UEFI-blue)

---

## Features

### Kernel and scheduling

- Ring 0/3 isolation using AMD64 `syscall` / `sysret`
- 28 syscall IDs currently dispatched (`0..=27`)
- SMP startup using INIT-SIPI-SIPI
- Per-CPU data through GS-base MSRs with `swapgs` handling
- Preemptive round-robin scheduling with per-CPU run queues
- LAPIC timer preemption and scheduler wake IPIs
- Task sleep, blocking wait, zombie/reaping support, and idle work stealing
- Interrupt-safe spinlocks and scheduler synchronization
- Event-based blocking/wakeup used by network waits

### Graphics and console

- UEFI GOP framebuffer, currently exercised at 1280×800
- Shared software framebuffer with clipped pixel/rectangle drawing and explicit presentation
- Modern PCI virtio-gpu 2D driver: resource backing, scanout, transfer and flush
- GOP output backend when no virtio-gpu is present
- 8×8 text rendering
- Primitive desktop/window frame
- Shell output rendered inside the window client area
- Viewport-aware console clearing

### Storage and userspace

- Custom KEF executable format
- Separate user address space resources and 512 KiB user heap
- NX-enabled user mappings
- NVMe controller driver using MMIO
- FAT16 filesystem with create/read/write/delete/list/format
- Userspace shell and utilities including `ls`, `cat`, `write`, `rm`, and `ping`

### USB

- xHCI USB 3.0 host controller driver
- USB keyboard through an interrupt IN endpoint

### Networking

- Intel E1000 (82540EM) driver
- PCI INTx routed through the I/O APIC
- IRQ-driven receive processing with bounded RX draining
- ARP cache and blocking ARP resolution
- Shared IPv4 transmit/routing path
- ICMP Echo Request/Reply
- UDP receive path used by DNS
- DNS A-record resolution with scheduler-based blocking timeout
- QEMU user-networking gateway/DNS routing

The networking milestone currently resolves and pings hostnames such as `google.com`. TCP, HTTP, and TLS are not implemented yet.

---

## Quick Start

### Requirements

- Rust toolchain with the `x86_64-unknown-uefi` target
- QEMU with x86_64 system emulation
- OVMF UEFI firmware
- `qemu-img`
- A shell environment capable of running the build scripts

With Nix:

```bash
nix develop
```

Or configure the UEFI target/firmware manually:

```bash
rustup target add x86_64-unknown-uefi
export OVMF_BIOS="/usr/share/ovmf/OVMF.fd"
```

Build userspace, insert KEF programs into the disk image, and launch QEMU:

```bash
./build_insert_run.sh
```

Or run each step separately:

```bash
./user/build.sh
./user/insert.sh
./run.sh
```

The default QEMU configuration uses virtio-vga with a 2D virtio-gpu driver, 2 virtual CPUs, NVMe storage, an xHCI USB keyboard, and an E1000 NIC with QEMU user-mode networking.

---

## Shell Commands

| Command | Description |
|---|---|
| `help` | Show available commands |
| `ls` | List files |
| `cat <file>` | Display file contents |
| `write <file> <msg>` | Write content to a file |
| `rm <file>` | Delete a file |
| `exec <file> [args]` | Execute a KEF binary |
| `ping <IPv4-or-hostname>` | Send ICMP echo requests; hostnames are resolved through DNS |
| `clear` | Clear the active terminal client area |
| `shutdown` | Power off |

Examples:

```text
kaguya> ping 8.8.8.8
PING 8.8.8.8: 56 data bytes
...
10 packets transmitted, 10 received, 0% packet loss

kaguya> ping google.com
Resolving google.com...
PING <resolved IPv4>: 56 data bytes
...
```

---

## Architecture

```text
                 +----------------------+
                 |     UEFI / OVMF      |
                 +----------+-----------+
                            |
                 +----------v-----------+
                 |    kaguyaOS kernel   |
                 +----------+-----------+
                            |
       +--------------------+--------------------+
       |                    |                    |
+------v------+      +------v------+      +------v------+
| Scheduler   |      |  Drivers    |      | Graphics /  |
| SMP / Tasks |      | NVMe/xHCI   |      | Console     |
+------+------+      | E1000       |      +------+------+
       |             +------+------+             |
       |                    |                    |
+------v--------------------v--------------------v------+
|                  Ring 3 userspace                    |
|          shell / ls / cat / write / rm / ping        |
+------------------------------------------------------+
```

---

## Project Structure

```text
kaguyaos/
├── src/
│   ├── main.rs                 # Boot flow and scheduler startup
│   ├── process/
│   │   └── mod.rs              # SMP scheduler and task lifecycle
│   ├── sync/
│   │   └── mod.rs              # Interrupt-safe Spinlock
│   ├── memory/
│   │   ├── mod.rs              # Frame allocator and page tables
│   │   └── heap.rs             # Kernel/user heap allocators
│   ├── console/
│   │   ├── mod.rs              # Console writer and text viewport
│   │   ├── framebuffer.rs      # Pixel/rectangle graphics primitives
│   │   └── term.rs             # Cell-based terminal renderer
│   ├── drivers/
│   │   ├── pci.rs              # PCI enumeration and interrupt metadata
│   │   ├── nvme.rs             # NVMe driver
│   │   ├── xhci.rs             # xHCI USB driver
│   │   └── net/
│   │       ├── mod.rs           # NIC state, ARP cache, RX work
│   │       ├── driver.rs        # NetworkDriver abstraction
│   │       ├── e1000.rs         # Intel E1000 driver
│   │       ├── arp.rs           # ARP and incoming frame dispatch
│   │       ├── ipv4.rs          # Shared IPv4 transmit path
│   │       ├── dns.rs           # DNS A-record resolver
│   │       └── helper.rs        # Network checksum helpers
│   ├── loader.rs               # KEF loader
│   ├── syscall.rs              # Syscall dispatcher
│   ├── interrupts.rs           # IDT, exceptions and IRQ handling
│   ├── fs.rs                   # FAT16 filesystem
│   ├── acpi.rs                 # ACPI/MADT discovery
│   ├── processor.rs            # AP startup, LAPIC/I/O APIC, per-CPU data
│   ├── pic.rs                  # Legacy PIC/PIT support
│   ├── gdt.rs                  # GDT/TSS
│   ├── io.rs                   # Port I/O
│   └── uefi.rs                 # UEFI definitions/runtime services
├── user/
│   ├── src/init.rs             # Shell
│   ├── src/std.rs              # Userspace runtime/syscall wrappers
│   ├── src/ping.rs             # IPv4/hostname ping utility
│   └── build.sh
├── tools/kef-tool/             # KEF packaging tool
├── docs/
│   └── syscall.md              # Syscall reference
├── run.sh
└── README.md
```

---

## Development Direction

The first networking goal — resolving and successfully pinging a hostname — is complete. Current work is focused on the graphics path:

```text
framebuffer primitives
        ↓
desktop + window
        ↓
terminal inside window
        ↓
mouse cursor
        ↓
window movement / composition
```

Networking remains an active subsystem. A later milestone is to add TCP and HTTP so a userspace program can perform a `curl`-style request.

---

## Known Limitations

- Graphics use a fixed software backbuffer (up to 1920×1080 pixels); there is no compositor.
- virtio-gpu currently uses one scanout and synchronous polling with full-frame transfers. No 3D acceleration, hotplug, mode switching, or AMD hardware support.
- The GOP backend supports RGB/BGR 32-bit modes. GPU runtime failures stop presentation and report through serial; live recovery is not implemented.
- The window is static and there is no mouse support yet.
- The E1000 path targets the QEMU 82540EM device.
- IPv4 configuration currently assumes the QEMU user-networking environment.
- DNS is a minimal single-query A-record resolver.
- TCP, HTTP, and TLS are not implemented.
- User stacks are not yet reclaimed after task exit.

---

## License

[Apache License 2.0](LICENSE)

## Graphics development

The renderer writes `0x00RRGGBB` pixels into a shared backbuffer. `console::display::present()`
then copies changed regions to GOP (including RGB/BGR conversion) or submits virtio-gpu transfer/flush
commands. The console and cell syscalls present after each output batch. Serial diagnostics
have an independent path, including kernel panic and CPU exception output.

The driver follows the [VIRTIO 1.2 specification](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html),
using modern PCI capabilities, VERSION_1 negotiation, a split control queue and fenced
2D commands. The backbuffer and queue storage live in the identity-mapped kernel image;
they are reserved for the kernel lifetime, including after device timeouts.

```bash
nix develop
./run.sh                       # virtio-vga (default)
GPU=gop ./run.sh               # standard VGA + GOP output

# After building the kernel and preparing nvme.img:
python3 tools/gpu-smoke.py
python3 tools/gpu-smoke.py --backend gop --output /tmp/kaguya-gop-smoke
```

The smoke runner uses a headless QEMU with disposable disk snapshots, waits for the shell,
sends `clear` and `help` through the virtual USB keyboard, and saves serial logs and
`desktop.ppm` / `help.ppm` screenshots. It checks GPU activation (virtio mode), command
completion and changing screen contents. Output defaults to `/tmp/kaguya-gpu-smoke`.
Mouse input and window movement are outside this milestone.

## First physical hardware test

Build the dedicated diagnostic EFI, **not the normal kernel**, for the first hardware boot:

```bash
nix develop
./tools/build-hardware-test.sh
python3 tools/gpu-smoke.py --hardware-test --output /tmp/kaguya-hardware-test
```

The artifact is `target/hardware-test/esp/EFI/BOOT/BOOTX64.EFI`. The build uses an
isolated target directory so it does not replace the normal QEMU kernel. The script
only creates files inside the repository; it does not format or install onto a USB drive.

Copy this artifact to `EFI/BOOT/BOOTX64.EFI` on a prepared FAT32 UEFI boot USB.
Use the firmware boot menu to select that USB. This EFI is unsigned; firmware
Secure Boot must permit it (for example, Secure Boot disabled for the test).
Do not use the normal `esp/EFI/BOOT/BOOTX64.EFI` for this milestone.

Expected result: a black screen with white diagnostic text, ending in
`HARDWARE TEST READY`. Take a photo of the screen, including the resolution and
memory-map entries. The kernel intentionally halts there; keyboard input and the
shell are not started. Reset or power off to leave the test.

This mode uses the firmware's existing page tables and GOP framebuffer after
ExitBootServices. It does not enumerate PCI or start NVMe, USB, network, virtio-gpu,
interrupt timers, application processors, a heap, or userspace. NVMe writes are
also rejected at the driver entry point in this build. It therefore validates only
UEFI handoff, kernel entry, firmware memory-map access and basic GOP output.
It does not establish that the normal kernel or any hardware drivers work on the PC.
Only linear 32-bit RGB/BGR GOP modes are accepted. The firmware memory-map buffer
is now aligned static storage with a 256 KiB capacity; larger maps still fail boot.

Normal boots no longer automatically format an unrecognized NVMe volume or create
a placeholder `init.kef`. Prepare the QEMU image with the host tools. Normal builds
still support disk writes and are not the physical-hardware diagnostic mode.

## Diskless physical-hardware shell with USB keyboard

After the diagnostic screen works, build the separate diskless shell:

```bash
nix develop
# Install once if this target is missing:
rustup target add x86_64-unknown-none
./tools/build-hardware-shell.sh
python3 tools/gpu-smoke.py --hardware-shell --resolution 2560x1440 --output /tmp/kaguya-usb-shell
```

Copy `target/hardware-shell/esp/EFI/BOOT/BOOTX64.EFI` to the USB's
`EFI/BOOT/BOOTX64.EFI`. Keep the hardware-test artifact as a fallback. Connect a
wired USB keyboard directly to a motherboard port **before boot**, preferably a
USB 2 port for the first test. Hubs, hotplug/reconnect, NKRO-only report protocols,
key repeat and recovery from stalled endpoints are not implemented.

The script builds `user/src/init.rs` into an isolated KEF file and embeds it in the
kernel, supplying `KAGUYA_INIT_KEF`. It does not modify `nvme.img`, normal userspace
binaries or physical disks. `hardware-shell` and `hardware-test` are mutually exclusive.

Expected screen: numbered initialization stages, `xHCI: boot keyboard ready`, a USB
controller summary, then the real ring-3 shell's `kaguya>` prompt. Try `help`, `clear`,
Shift and Backspace. External programs (`ls`, `cat`, `write`, etc.) are not embedded
and cannot run without a filesystem. `shutdown` uses the UEFI runtime service;
reset or power off manually if the firmware does not shut down.

The driver selects one xHCI controller, defaulting to index 0 in PCI scan order.
Only that controller is enabled; other PCI devices are not initialized. If the
keyboard is attached to another controller, build with a different index:

```bash
KAGUYA_XHCI_INDEX=1 ./tools/build-hardware-shell.sh
```

The selected PCI address and keyboard count are logged. If there are zero keyboards,
try another direct motherboard port or another controller index, then reboot. A
USB 3-labelled port may work with a USB 2 keyboard through its USB 2 root port, but
an intermediate hub is currently unsupported. Photograph any timeout/error and the
controller summary when reporting hardware failures.

In hardware-shell mode, port enumeration stops after the first boot keyboard is
ready. This avoids a later unrelated USB device timing out and stopping the shared
controller. Devices encountered before the keyboard can still cause a timeout.

This mode installs GDT/IDT, page tables, syscalls, heaps and the BSP scheduler. The
GOP backbuffer follows the firmware resolution (up to 128 MiB), including 2560×1440.
USB events are polled from the shell, with IF kept clear; timer/AP startup, network
and NVMe initialization remain disabled, and NVMe writes are rejected. Polling may
keep one CPU core busy while waiting for input. Boot logs remain on screen.

xHCI bring-up includes firmware ownership handoff, stop/reset waits, 32/64-byte
contexts, up to 256 scratchpad pages, bounded polling, USB 2 port reset and boot-HID
interface selection. Configuration/interface numbers come from descriptors. Controller
registers must fit the mapped 64 KiB window. Port reset uses the controller microframe
counter for a one-second deadline and a 10 ms recovery delay, with an iteration cap
if the counter stops. Other poll limits are iteration budgets. Controllers requiring additional quirks may fail
with diagnostics. See the [Linux xHCI register definitions](https://github.com/torvalds/linux/blob/master/drivers/usb/host/xhci.h)
and [endpoint setup](https://github.com/torvalds/linux/blob/master/drivers/usb/host/xhci-mem.c)
for reference on context fields and polling intervals.

Additional QEMU checks:

```bash
python3 tools/gpu-smoke.py --hardware-shell --usb-mouse --keyboard-stress --output /tmp/kaguya-usb-stress
python3 tools/gpu-smoke.py --hardware-shell --no-keyboard --output /tmp/kaguya-usb-empty
python3 tools/gpu-smoke.py --hardware-shell --shutdown --output /tmp/kaguya-shutdown
```

The stress check exercises Shift/Backspace and 30 `help` commands to wrap the transfer
and event rings. A mouse preceding the keyboard tests HID filtering; an additional
mouse after the keyboard checks that enumeration stops. The empty-controller
check verifies that a missing keyboard does not prevent the shell from appearing.
The shutdown check types the shell command and requires QEMU's guest-shutdown event
and a clean process exit. Shutdown logs device stopping and the UEFI power-off request;
if physical power-off stalls, photograph the last message.

On 2026-09-27, the earlier diskless preview reached the ring-3 prompt on physical
hardware at 2560×1440. USB keyboard input, `clear` and `help` were subsequently confirmed
on the same physical machine. UEFI shutdown passes QEMU testing and still needs physical
validation. QEMU does not exercise every real controller's ownership,
scratchpad or 64-byte-context behavior.

The first physical USB test reached controller startup but all connected ports timed
out during reset, leaving zero keyboards. The reset write mask was found to omit
PORTSC's port-power bit; reset and acknowledgement writes now preserve it. Reset
completion checks current connection/enable/reset state instead of requiring the PRC
change notification. Failed resets log raw PORTSC and decoded state for the next
physical test. Stopping enumeration after the first keyboard also avoids a later
device's timeout disabling the controller; physical keyboard input then succeeded.

The PORTSC write regression checks can be run inside `nix develop`:

```bash
rustc --test src/drivers/xhci_port.rs -o /tmp/kaguya-xhci-port-test
/tmp/kaguya-xhci-port-test
```
