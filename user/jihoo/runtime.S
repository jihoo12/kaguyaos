.text
.globl _start
.type _start,@function
_start:
    andq $-16, %rsp
    call kaguya_main
    ud2
.size _start, .-_start

.globl kaguya_syscall
.type kaguya_syscall,@function
kaguya_syscall:
    movq %rdi, %rax
    movq %rsi, %rdi
    movq %rdx, %rsi
    movq %rcx, %rdx
    movq %r8, %r10
    movq %r9, %r8
    syscall
    ret
.size kaguya_syscall, .-kaguya_syscall

.globl kaguya_rdtsc
.type kaguya_rdtsc,@function
kaguya_rdtsc:
    rdtsc
    shlq $32, %rdx
    orq %rdx, %rax
    ret
.size kaguya_rdtsc, .-kaguya_rdtsc

.globl memcpy
.type memcpy,@function
memcpy:
    movq %rdi, %rax
    movq %rdx, %rcx
    rep movsb
    ret
.size memcpy, .-memcpy

.globl memset
.type memset,@function
memset:
    movq %rdi, %r8
    movl %esi, %eax
    movq %rdx, %rcx
    rep stosb
    movq %r8, %rax
    ret
.size memset, .-memset

.globl memmove
.type memmove,@function
memmove:
    movq %rdi, %rax
    movq %rdx, %rcx
    cmpq %rsi, %rdi
    jbe 1f
    leaq -1(%rdi,%rdx), %rdi
    leaq -1(%rsi,%rdx), %rsi
    std
    rep movsb
    cld
    ret
1:
    rep movsb
    ret
.size memmove, .-memmove

.section .note.GNU-stack,"",@progbits
