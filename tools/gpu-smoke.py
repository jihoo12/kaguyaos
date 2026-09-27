#!/usr/bin/env python3
"""Boot a disposable QEMU snapshot, save serial output and QMP screenshots.
Run inside nix develop after building the kernel and preparing nvme.img.
"""
import argparse
import json
import os
import re
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--shutdown', action='store_true', help='Type shutdown and require guest-initiated QEMU power off')
parser.add_argument('--keyboard-stress', action='store_true', help='Exercise Shift, Backspace and more than one USB ring wrap')
parser.add_argument('--usb-mouse', action='store_true', help='Add a mouse before the keyboard to test HID filtering')
parser.add_argument('--no-keyboard', action='store_true', help='Hardware-shell boot with an empty xHCI controller')
parser.add_argument('--resolution', help='Preferred GOP resolution, e.g. 2560x1440 (GOP only)')
parser.add_argument('--hardware-shell', action='store_true', help='Test diskless ring-3 shell with a USB keyboard')
parser.add_argument('--hardware-test', action='store_true', help='Test the isolated diagnostic EFI without NVMe, USB or network devices')
parser.add_argument('--backend', choices=['virtio', 'gop'], default='virtio')
parser.add_argument('--output', type=Path, default=Path('/tmp/kaguya-gpu-smoke'))
args = parser.parse_args()
if args.hardware_test and args.hardware_shell:
    parser.error('choose hardware-test or hardware-shell')
if args.shutdown and (args.hardware_test or args.no_keyboard):
    parser.error('--shutdown requires a shell with a keyboard')
if args.no_keyboard and (not args.hardware_shell or args.keyboard_stress):
    parser.error('--no-keyboard requires hardware-shell and cannot be combined with stress')
isolated = args.hardware_test or args.hardware_shell
root = Path(__file__).resolve().parent.parent
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='kaguya-qmp-') as tmp:
    tmp = Path(tmp)
    boot = tmp / 'esp/EFI/BOOT'
    boot.mkdir(parents=True)
    kernel = 'target/hardware-test/build/x86_64-unknown-uefi/debug/os.efi' if args.hardware_test else 'target/x86_64-unknown-uefi/debug/os.efi'
    if args.hardware_shell:
        kernel = 'target/hardware-shell/build/x86_64-unknown-uefi/debug/os.efi'
    shutil.copyfile(root / kernel, boot / 'BOOTX64.EFI')
    qmp = tmp / 'qmp.sock'
    if isolated:
        args.backend = 'gop'
    gpu = ['-vga', 'none', '-device', 'virtio-vga'] if args.backend == 'virtio' else ['-vga', 'std']
    if args.resolution:
        if args.backend != 'gop' or not re.fullmatch(r'[1-9][0-9]{2,4}x[1-9][0-9]{2,4}', args.resolution):
            parser.error('--resolution requires GOP and WIDTHxHEIGHT')
        width, height = map(int, args.resolution.split('x'))
        gpu = ['-vga', 'none', '-device', f'VGA,xres={width},yres={height},vgamem_mb=64']
    command = ['qemu-system-x86_64', '-smp', '2', '-m', '256', '-bios', os.environ['OVMF_BIOS'],
               '-drive', f'format=raw,file=fat:ro:{tmp / "esp"}',
               '-snapshot', '-display', 'none', '-serial', f'file:{out / "serial.log"}',
               '-qmp', f'unix:{qmp},server=on,wait=off', '-no-reboot', *gpu]
    if not isolated:
        command += [
               '-drive', f'file={root / "nvme.img"},if=none,id=nvm,format=raw',
               '-device', 'nvme,serial=deadbeef,drive=nvm',
               '-device', 'qemu-xhci,id=xhci,msi=off,msix=off', '-device', 'usb-kbd,bus=xhci.0',
               '-device', 'e1000,netdev=net0', '-netdev', 'user,id=net0'
        ]
    else:
        command += ['-nic', 'none']
        if args.hardware_shell:
            command += ['-device', 'qemu-xhci,id=xhci,msi=off,msix=off']
            if args.usb_mouse:
                command += ['-device', 'usb-mouse,bus=xhci.0']
            if not args.no_keyboard:
                command += ['-device', 'usb-kbd,bus=xhci.0']
                command += ['-device', 'usb-mouse,bus=xhci.0']
    with (out / 'qemu.log').open('w') as log:
        process = subprocess.Popen(command, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 60
            while not qmp.exists():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError('QEMU did not start; see qemu.log')
                time.sleep(.1)
            with socket.socket(socket.AF_UNIX) as sock:
                sock.settimeout(10)
                sock.connect(str(qmp))
                stream = sock.makefile('rwb', buffering=0)
                json.loads(stream.readline())
                def execute(name, arguments=None):
                    stream.write((json.dumps({'execute': name, 'arguments': arguments or {}}) + '\n').encode())
                    while True:
                        response = json.loads(stream.readline())
                        if 'error' in response:
                            raise RuntimeError(response['error'])
                        if 'return' in response:
                            return response['return']
                execute('qmp_capabilities')
                while True:
                    serial = (out / 'serial.log').read_text(errors='replace') if (out / 'serial.log').exists() else ''
                    marker = 'HARDWARE TEST READY' if args.hardware_test else 'kaguya>'
                    if marker in serial:
                        break
                    if time.monotonic() > deadline or process.poll() is not None:
                        raise RuntimeError('Shell did not start; see serial.log')
                    time.sleep(.2)
                if args.backend == 'virtio' and 'virtio-gpu: scanout' not in serial:
                    raise RuntimeError('Shell booted without virtio-gpu')
                time.sleep(1)
                execute('screendump', {'filename': str(out / 'desktop.ppm')})
                if args.resolution:
                    with (out / 'desktop.ppm').open('rb') as capture:
                        capture.readline()
                        actual = tuple(map(int, capture.readline().split()))
                    if actual != (width, height):
                        raise RuntimeError(f'Firmware selected {actual}, wanted {(width, height)}')
                if not args.hardware_test and not args.no_keyboard:
                    # Exercise USB keyboard -> userspace shell -> display update.
                    for key in ['c', 'l', 'e', 'a', 'r', 'ret', 'h', 'e', 'l', 'p', 'ret']:
                        execute('human-monitor-command', {'command-line': f'sendkey {key}'})
                        time.sleep(.15)
                    deadline = time.monotonic() + 30
                    while True:
                        final = (out / 'serial.log').read_text(errors='replace')
                        output = final[len(serial):]
                        if 'shutdown' in output and output.rstrip().endswith('kaguya>'):
                            break
                        if time.monotonic() > deadline:
                            raise RuntimeError('Shell help timed out')
                        time.sleep(.2)
                    time.sleep(.5)
                    execute('screendump', {'filename': str(out / 'help.ppm')})
                    if 'help' not in final[len(serial):] or 'shutdown' not in final[len(serial):]:
                        raise RuntimeError('Shell help command did not complete')
                    if 'screen updates stopped' in final or 'panicked' in final:
                        raise RuntimeError('Kernel/GPU failure; see serial.log')
                    if (out / 'desktop.ppm').read_bytes() == (out / 'help.ppm').read_bytes():
                        raise RuntimeError('Screen did not change after keyboard input')
                elif args.hardware_test:
                    forbidden = ['PCI: Checking', 'NVMe: Init', 'xHCI:', 'Starting scheduler', 'Formatting...']
                    if any(marker in serial for marker in forbidden):
                        raise RuntimeError('Hardware-test unexpectedly initialized a subsystem')
                    name = 'hardware-shell.png' if args.hardware_shell else 'hardware-test.png'
                    execute('screendump', {'filename': str(out / name), 'format': 'png'})
                    if args.hardware_shell and 'kaguya>' not in serial:
                        raise RuntimeError('Missing userspace prompt')
                if args.keyboard_stress:
                    baseline = (out / 'serial.log').read_text(errors='replace')
                    for key in ['shift-a', 'b', 'backspace', 'ret'] + ['h', 'e', 'l', 'p', 'ret'] * 30:
                        execute('human-monitor-command', {'command-line': f'sendkey {key}'})
                        time.sleep(.15)
                    deadline = time.monotonic() + 30
                    while True:
                        final = (out / 'serial.log').read_text(errors='replace')
                        added = final[len(baseline):]
                        if added.count('Commands:') == 30 and added.rstrip().endswith('kaguya>'):
                            break
                        if time.monotonic() > deadline:
                            raise RuntimeError('Keyboard ring-wrap stress timed out')
                        time.sleep(.2)
                    if 'Unknown: A' not in added:
                        raise RuntimeError('Shift/Backspace input failed')
                if args.hardware_shell:
                    final = (out / 'serial.log').read_text(errors='replace')
                    if args.no_keyboard and '0 boot keyboard(s) ready' not in serial:
                        raise RuntimeError('Expected an empty controller')
                    if not args.no_keyboard and 'xHCI: boot keyboard ready' not in serial:
                        raise RuntimeError('No USB keyboard enumerated')
                    if not args.no_keyboard:
                        stop = 'xHCI: first boot keyboard ready; stopping port enumeration'
                        if stop not in serial or 'xHCI: Addressing slot' in serial.split(stop, 1)[1]:
                            raise RuntimeError('Port enumeration did not stop after the first keyboard')
                        expected_slots = 2 if args.usb_mouse else 1
                        if serial.count('xHCI: Assigned Slot ID:') != expected_slots:
                            raise RuntimeError('Unexpected devices enumerated around the keyboard')
                    if any(marker in final for marker in ['NVMe: Init', 'Formatting...', 'EXCEPTION OCCURRED', 'panicked']):
                        raise RuntimeError('Diskless shell failure or storage initialization')
                    execute('screendump', {'filename': str(out / 'hardware-shell.png'), 'format': 'png'})
                if args.shutdown:
                    for key in ['s', 'h', 'u', 't', 'd', 'o', 'w', 'n']:
                        execute('human-monitor-command', {'command-line': f'sendkey {key}'})
                        time.sleep(.15)
                    execute('human-monitor-command', {'command-line': 'sendkey ret'})
                    shutdown_event = False
                    while True:
                        line = stream.readline()
                        if not line:
                            break
                        event = json.loads(line)
                        if event.get('event') == 'SHUTDOWN':
                            data = event.get('data', {})
                            shutdown_event = data.get('guest') is True and data.get('reason') == 'guest-shutdown'
                    process.wait(timeout=10)
                    final = (out / 'serial.log').read_text(errors='replace')
                    if not shutdown_event or process.returncode != 0 or 'Shutdown: requesting UEFI power off' not in final:
                        raise RuntimeError('Guest did not complete UEFI shutdown; see serial.log')
                else:
                    execute('quit')
                process.wait(timeout=10)
                label = 'hardware-test: diagnostic boot passed' if args.hardware_test else f'{args.backend}: boot, GPU/console and shell help passed'
                if args.hardware_shell:
                    label = 'hardware-shell: no-keyboard boot passed' if args.no_keyboard else 'hardware-shell: USB keyboard, clear and help passed'
                    if args.keyboard_stress:
                        label += '; Shift, Backspace and ring-wrap stress passed'
                print(f'{label}; artifacts: {out}')
                if args.shutdown:
                    print('Guest-initiated shutdown passed')
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
