#!/usr/bin/env python3
"""Boot a disposable QEMU snapshot, save serial output and QMP screenshots.
Run inside nix develop after building the kernel and preparing nvme.img.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--hardware-test', action='store_true', help='Test the isolated diagnostic EFI without NVMe, USB or network devices')
parser.add_argument('--backend', choices=['virtio', 'gop'], default='virtio')
parser.add_argument('--output', type=Path, default=Path('/tmp/kaguya-gpu-smoke'))
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='kaguya-qmp-') as tmp:
    tmp = Path(tmp)
    boot = tmp / 'esp/EFI/BOOT'
    boot.mkdir(parents=True)
    kernel = 'target/hardware-test/build/x86_64-unknown-uefi/debug/os.efi' if args.hardware_test else 'target/x86_64-unknown-uefi/debug/os.efi'
    shutil.copyfile(root / kernel, boot / 'BOOTX64.EFI')
    qmp = tmp / 'qmp.sock'
    if args.hardware_test:
        args.backend = 'gop'
    gpu = ['-vga', 'none', '-device', 'virtio-vga'] if args.backend == 'virtio' else ['-vga', 'std']
    command = ['qemu-system-x86_64', '-smp', '2', '-m', '256', '-bios', os.environ['OVMF_BIOS'],
               '-drive', f'format=raw,file=fat:ro:{tmp / "esp"}',
               '-snapshot', '-display', 'none', '-serial', f'file:{out / "serial.log"}',
               '-qmp', f'unix:{qmp},server=on,wait=off', '-no-reboot', *gpu]
    if not args.hardware_test:
        command += [
               '-drive', f'file={root / "nvme.img"},if=none,id=nvm,format=raw',
               '-device', 'nvme,serial=deadbeef,drive=nvm',
               '-device', 'qemu-xhci,id=xhci,msi=off,msix=off', '-device', 'usb-kbd,bus=xhci.0',
               '-device', 'e1000,netdev=net0', '-netdev', 'user,id=net0'
        ]
    else:
        command += ['-nic', 'none']
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
                    if ('HARDWARE TEST READY' if args.hardware_test else 'kaguya>') in serial:
                        break
                    if time.monotonic() > deadline or process.poll() is not None:
                        raise RuntimeError('Shell did not start; see serial.log')
                    time.sleep(.2)
                if args.backend == 'virtio' and 'virtio-gpu: scanout' not in serial:
                    raise RuntimeError('Shell booted without virtio-gpu')
                time.sleep(1)
                execute('screendump', {'filename': str(out / 'desktop.ppm')})
                if not args.hardware_test:
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
                else:
                    forbidden = ['PCI: Checking', 'NVMe: Init', 'xHCI:', 'Starting scheduler', 'Formatting...']
                    if any(marker in serial for marker in forbidden):
                        raise RuntimeError('Hardware-test unexpectedly initialized a subsystem')
                    execute('screendump', {'filename': str(out / 'hardware-test.png'), 'format': 'png'})
                execute('quit')
                process.wait(timeout=10)
                label = 'hardware-test: diagnostic boot passed' if args.hardware_test else f'{args.backend}: boot, GPU/console and shell help passed'
                print(f'{label}; artifacts: {out}')
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
