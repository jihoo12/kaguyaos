#!/usr/bin/env python3
"""Boot a disposable QEMU snapshot, save serial output and QMP screenshots.
Run inside nix develop after building the kernel and preparing nvme.img.
"""
import argparse
from contextlib import ExitStack
import http.server
import json
import os
import re
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import time

parser = argparse.ArgumentParser()
parser.add_argument('--shutdown', action='store_true', help='Type shutdown and require guest-initiated QEMU power off')
parser.add_argument('--keyboard-stress', action='store_true', help='Exercise Shift, Backspace and more than one USB ring wrap')
parser.add_argument('--usb-mouse', action='store_true', help='Add a mouse before the keyboard to test HID filtering')
parser.add_argument('--no-keyboard', action='store_true', help='Hardware-shell boot with an empty xHCI controller')
parser.add_argument('--resolution', help='Preferred GOP resolution, e.g. 2560x1440 (GOP only)')
parser.add_argument('--hardware-shell', action='store_true', help='Test diskless ring-3 shell with a USB keyboard')
parser.add_argument('--shell-io', action='store_true', help='Check diskless shell pipes, redirection and exec arguments')
parser.add_argument('--network', action='store_true', help='Check ping and wget against a local HTTP fixture')
parser.add_argument('--disk-image', type=Path, help='Use a prepared NVMe image instead of nvme.img')
parser.add_argument('--kernel-image', type=Path, help='Use an alternate EFI artifact for this boot mode')
parser.add_argument('--hardware-test', action='store_true', help='Test the isolated diagnostic EFI without NVMe, USB or network devices')
parser.add_argument('--cpu-test', action='store_true', help='Test the standalone jihoo CPU/page-table EFI without disks or external devices')
parser.add_argument('--ring3-test', action='store_true', help='Test standalone jihoo KEF execution and syscall round trips without disks')
parser.add_argument('--filesystem-test', action='store_true', help='Test standalone jihoo RAM filesystem and ring-3 checks without disks')
parser.add_argument('--backend', choices=['virtio', 'gop'], default='virtio')
parser.add_argument('--output', type=Path, default=Path('/tmp/kaguya-gpu-smoke'))
args = parser.parse_args()
if args.filesystem_test:
    if args.ring3_test or args.cpu_test or args.hardware_test or args.hardware_shell:
        parser.error('choose filesystem-test, ring3-test, cpu-test, hardware-test or hardware-shell')
    args.ring3_test = True
if args.ring3_test:
    if args.cpu_test or args.hardware_test or args.hardware_shell:
        parser.error('choose ring3-test, cpu-test, hardware-test or hardware-shell')
    args.cpu_test = True
if args.cpu_test:
    if args.hardware_test or args.hardware_shell:
        parser.error('choose cpu-test, hardware-test or hardware-shell')
    args.hardware_test = True
if args.hardware_test and args.hardware_shell:
    parser.error('choose hardware-test or hardware-shell')
if args.shutdown and (args.hardware_test or args.no_keyboard):
    parser.error('--shutdown requires a shell with a keyboard')
if args.no_keyboard and (not args.hardware_shell or args.keyboard_stress):
    parser.error('--no-keyboard requires hardware-shell and cannot be combined with stress')
if args.shell_io and (not args.hardware_shell or args.no_keyboard):
    parser.error('--shell-io requires hardware-shell with a keyboard')
isolated = args.hardware_test or args.hardware_shell
if args.network and isolated:
    parser.error('--network requires the normal kernel')
if args.disk_image and isolated:
    parser.error('--disk-image requires the normal kernel')
root = Path(__file__).resolve().parent.parent
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
with ExitStack() as resources, tempfile.TemporaryDirectory(prefix='kaguya-qmp-') as tmp:
    tmp = Path(tmp)
    requests = []
    if args.network:
        class Fixture(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                requests.append(self.path)
                route = self.path.split('?', 1)[0]
                payload = b'jihoo-http-body\n'
                status = 200
                if route == '/redirect/start':
                    status = 302
                    payload = b''
                elif route == '/missing':
                    status = 404
                    payload = b'not found\n'
                elif route == '/short':
                    payload = b'xy'
                elif route == '/large':
                    payload = b'jihoo-large-body\n' * 600
                self.send_response(status)
                self.send_header('Content-Length', str(10 if route == '/short' else len(payload)))
                self.send_header('Connection', 'close')
                if status == 302:
                    self.send_header('Location', '../ok?from=redirect')
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, *unused):
                pass

        http_server = resources.enter_context(http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture))
        http_thread = threading.Thread(target=http_server.serve_forever, daemon=True)
        http_thread.start()
        resources.callback(http_thread.join, 2)
        resources.callback(http_server.shutdown)
    boot = tmp / 'esp/EFI/BOOT'
    boot.mkdir(parents=True)
    kernel = 'target/hardware-test/build/x86_64-unknown-uefi/debug/os.efi' if args.hardware_test else 'target/x86_64-unknown-uefi/debug/os.efi'
    if args.cpu_test:
        kernel = 'target/jihoo-kernel/cpu-test/os.efi'
    if args.ring3_test:
        kernel = 'target/jihoo-kernel/ring3-test/os.efi'
    if args.filesystem_test:
        kernel = 'target/jihoo-kernel/filesystem-test/os.efi'
    if args.hardware_shell:
        kernel = 'target/hardware-shell/build/x86_64-unknown-uefi/debug/os.efi'
    shutil.copyfile(args.kernel_image.resolve() if args.kernel_image else root / kernel, boot / 'BOOTX64.EFI')
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
               '-drive', f'file={args.disk_image.resolve() if args.disk_image else root / "nvme.img"},if=none,id=nvm,format=raw',
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
                def shell_command(command, timeout=15):
                    baseline = (out / 'serial.log').read_text(errors='replace')
                    keys = {' ': 'spc', '.': 'dot', '|': 'shift-backslash',
                            '>': 'shift-dot', '<': 'shift-comma', '/': 'slash',
                            ':': 'shift-semicolon', '-': 'minus', '?': 'shift-slash',
                            '=': 'equal', '#': 'shift-3'}
                    for char in command:
                        execute('human-monitor-command', {'command-line': f'sendkey {keys.get(char, char)}'})
                        time.sleep(.1)
                    execute('human-monitor-command', {'command-line': 'sendkey ret'})
                    deadline = time.monotonic() + timeout
                    while time.monotonic() < deadline:
                        added = (out / 'serial.log').read_text(errors='replace')[len(baseline):]
                        if added.rstrip().endswith('kaguya>'):
                            return added
                        time.sleep(.1)
                    raise RuntimeError(f'Shell command timed out: {command}')
                execute('qmp_capabilities')
                while True:
                    serial = (out / 'serial.log').read_text(errors='replace') if (out / 'serial.log').exists() else ''
                    marker = 'FILESYSTEM TEST READY' if args.filesystem_test else ('RING3 TEST READY' if args.ring3_test else ('CPU TEST READY' if args.cpu_test else ('HARDWARE TEST READY' if args.hardware_test else 'kaguya>')))
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
                    if args.cpu_test:
                        required = ['CPU tables: GDT, TSS, per-CPU double-fault stacks and IDT loaded',
                                    'Paging replacement: CR3, kernel permissions, frame reuse and zeroing passed',
                                    'CPU interrupts: breakpoint return and ten PIT ticks passed']
                        if any(marker not in serial for marker in required):
                            raise RuntimeError('Standalone CPU/page-table self-check missing; see serial.log')
                    if args.ring3_test:
                        required = ['Ring3: jihoo KEF, syscall return, user-pointer rejection, NX heap and twenty cleanups passed',
                                    'Ring3: user page fault contained and process frames reclaimed',
                                    'Ring3: NX execution rejected and user-mode timer return passed']
                        if any(marker not in serial for marker in required):
                            raise RuntimeError('Standalone ring-3 self-check missing; see serial.log')
                    if args.filesystem_test:
                        required = ['RAM filesystem: new temporary 8 MiB volume initialized; no disk devices accessed',
                                    'RAM filesystem: create/list/read/sparse write/delete/stale handle and cleanup passed']
                        if any(marker not in serial for marker in required):
                            raise RuntimeError('Standalone RAM filesystem check missing; see serial.log')
                    name = 'filesystem-test.png' if args.filesystem_test else ('ring3-test.png' if args.ring3_test else ('cpu-test.png' if args.cpu_test else 'hardware-test.png'))
                    execute('screendump', {'filename': str(out / name), 'format': 'png'})
                    if args.hardware_shell and 'kaguya>' not in serial:
                        raise RuntimeError('Missing userspace prompt')
                if args.hardware_shell and not args.no_keyboard:
                    listing = shell_command('ls')
                    if 'session' in listing:
                        raise RuntimeError('Previous session file survived reboot')
                    if 'welcome.txt' not in listing:
                        raise RuntimeError('RAM filesystem seed missing')
                    if 'Changes disappear on reboot.' not in shell_command('cat welcome.txt'):
                        raise RuntimeError('RAM filesystem seed read failed')
                    if 'Wrote 5 bytes' not in shell_command('write note hello'):
                        raise RuntimeError('RAM filesystem write failed')
                    if 'hello' not in shell_command('cat note').split('\n', 1)[-1]:
                        raise RuntimeError('RAM filesystem readback failed')
                    if 'Wrote 3 bytes' not in shell_command('write note bye'):
                        raise RuntimeError('RAM filesystem overwrite failed')
                    if 'bye' not in shell_command('cat note').split('\n', 1)[-1]:
                        raise RuntimeError('RAM filesystem overwrite readback failed')
                    if 'Deleted note' not in shell_command('rm note') or 'note' in shell_command('ls'):
                        raise RuntimeError('RAM filesystem deletion failed')
                    if 'Wrote 4 bytes' not in shell_command('write session temp'):
                        raise RuntimeError('Temporary session file creation failed')
                    print('RAM filesystem: seed, list, write, read, overwrite and delete passed')
                    if args.shell_io:
                        seed = 'Changes disappear on reboot.'
                        if seed not in shell_command('cat welcome.txt | cat'):
                            raise RuntimeError('Shell pipeline failed')
                        shell_command('cat welcome.txt > copied')
                        if seed not in shell_command('cat < copied'):
                            raise RuntimeError('Shell input/output redirection failed')
                        shell_command('cat welcome.txt >> copied')
                        if shell_command('cat copied').count(seed) != 2:
                            raise RuntimeError('Shell append redirection failed')
                        if 'welcome.txt' not in shell_command('ls | cat').split('\n', 1)[-1]:
                            raise RuntimeError('Shell listing pipeline failed')
                        if seed not in shell_command('exec cat.kef welcome.txt'):
                            raise RuntimeError('Shell exec arguments failed')
                        if 'missing file name after redirection' not in shell_command('cat >'):
                            raise RuntimeError('Shell missing-redirection error failed')
                        if 'missing command' not in shell_command('ls |'):
                            raise RuntimeError('Shell empty-stage error failed')
                        if 'cannot be piped or redirected' not in shell_command('help > copied'):
                            raise RuntimeError('Shell builtin redirection error failed')
                        if seed not in shell_command('cat welcome.txt' + ' | cat' * 7):
                            raise RuntimeError('Shell maximum-length pipeline failed')
                        shell_command('rm copied')
                        print('Shell I/O: pipelines, redirection, append, exec and syntax errors passed')
                    # Each program gets its own page tables; exiting must return every frame.
                    loads = [re.search(r'\[kef\] loaded .*, (\d+) frames in use', shell_command('ls'))
                             for _ in range(40)]
                    if None in loads:
                        raise RuntimeError('Program frame usage missing from exec output')
                    if len({load.group(1) for load in loads}) != 1:
                        raise RuntimeError(f'Exited program frames were not reclaimed: {[l.group(1) for l in loads]}')
                    print('Exec memory: 40 sequential programs used a constant number of frames')
                if args.network:
                    ping = shell_command('ping 10.0.2.2', timeout=30)
                    if '10 packets transmitted, 10 received, 0% packet loss' not in ping:
                        raise RuntimeError('Gateway ping failed')
                    base = f'http://10.0.2.2:{http_server.server_port}'
                    if 'jihoo-http-body' not in shell_command(f'wget {base}/ok'):
                        raise RuntimeError('HTTP body read failed')
                    if 'jihoo-http-body' not in shell_command(f'wget {base}/redirect/start'):
                        raise RuntimeError('HTTP redirect failed')
                    if '/ok?from=redirect' not in requests:
                        raise RuntimeError('HTTP relative path/query resolution failed')
                    shell_command(f'wget {base}/ok > fetched')
                    if 'jihoo-http-body' not in shell_command('cat fetched'):
                        raise RuntimeError('HTTP redirected output failed')
                    large = shell_command(f'wget {base}/large', timeout=30)
                    if large.count('jihoo-large-body') != 600:
                        raise RuntimeError('HTTP streaming body read failed')
                    shell_command(f'wget {base}/large | cat | cat > piped', timeout=30)
                    piped = shell_command('cat piped', timeout=30)
                    if piped.count('jihoo-large-body') != 600 or 'jihoo-large-body\n' * 600 not in piped:
                        raise RuntimeError('HTTP body through bounded pipes failed')
                    shell_command('rm piped')
                    if 'exit code 1' not in shell_command(f'wget {base}/missing'):
                        raise RuntimeError('HTTP error status did not fail')
                    if 'body shorter than Content-Length' not in shell_command(f'wget {base}/short'):
                        raise RuntimeError('HTTP incomplete body did not fail')
                    if 'https is not supported' not in shell_command('wget https://example.com'):
                        raise RuntimeError('HTTPS rejection failed')
                    shell_command('rm fetched')
                    final = (out / 'serial.log').read_text(errors='replace')
                    if any(marker in final for marker in ['EXCEPTION OCCURRED', 'panicked', 'panic:']):
                        raise RuntimeError('Userspace or kernel panic during network checks')
                    print('Network: gateway ping, HTTP streaming, bounded pipes, redirects, output and error cases passed')
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
                label = 'filesystem-test: RAM filesystem/KEF/syscall checks passed' if args.filesystem_test else ('ring3-test: KEF/syscall checks passed' if args.ring3_test else ('cpu-test: CPU/page-table checks passed' if args.cpu_test else ('hardware-test: diagnostic boot passed' if args.hardware_test else f'{args.backend}: boot, GPU/console and shell help passed')))
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
