.text
.globl jihoo_disable_interrupts
jihoo_disable_interrupts:
    cli
    ret
.globl jihoo_halt
jihoo_halt:
    cli
1:
    hlt
    jmp 1b

.globl jihoo_read_msr
jihoo_read_msr:
    rdmsr
    shlq $32, %rdx
    orq %rdx, %rax
    ret
.globl jihoo_write_msr
jihoo_write_msr:
    movq %rdx, %rax
    shrq $32, %rdx
    wrmsr
    ret
.globl jihoo_syscall_address
jihoo_syscall_address:
    leaq jihoo_syscall_entry(%rip), %rax
    ret

jihoo_syscall_entry:
    swapgs
    movq %rsp, %gs:8
    movq %gs:0, %rsp
    pushq %r11
    pushq %rcx
    pushq %rbp
    pushq %rbx
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    pushq %rdi
    pushq %rsi
    cld
    subq $64, %rsp
    movq %r10, 32(%rsp)
    movq %r8, 40(%rsp)
    movq %r9, 48(%rsp)
    movq %rdx, %r9
    movq %rax, %rcx
    movq %rdi, %rdx
    movq %rsi, %r8
    call jihoo_syscall_dispatch
    addq $64, %rsp
    popq %rsi
    popq %rdi
    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %rbx
    popq %rbp
    popq %rcx
    popq %r11
    movq %gs:8, %rsp
    swapgs
    sysretq

# Isolated ring-3 test continuation, replaced by scheduling in the full OS.
.globl jihoo_run_user
jihoo_run_user:
    pushq %rbx
    pushq %rbp
    pushq %rdi
    pushq %rsi
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    movq %rsp, jihoo_probe_kernel_rsp(%rip)
    movq %r8, %rdi
    movq %r9, %rsi
    pushq $0x1b
    pushq %rdx
    pushq $0x202
    pushq $0x23
    pushq %rcx
    swapgs
    iretq

.globl jihoo_finish_user
jihoo_finish_user:
    cli
    movq %rcx, %rax
    movq jihoo_probe_kernel_rsp(%rip), %rsp
    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %rsi
    popq %rdi
    popq %rbp
    popq %rbx
    ret

.bss
.p2align 3
jihoo_probe_kernel_rsp:
    .quad 0
