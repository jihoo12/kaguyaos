#![no_std]
#![no_main]

use core::arch::global_asm;

global_asm!(
    "
    .global _start
    _start:
    .code64
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    xor ax, ax
    mov fs, ax
    mov gs, ax
    mov rsp, qword ptr [0x8F10]
    mov rax, qword ptr [0x8F00]
    jmp rax
    "
);

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
