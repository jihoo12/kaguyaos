.text
.globl kaguya_context_switch
kaguya_context_switch:
    movq 40(%rsp), %r10
    movq 48(%rsp), %r11
    pushq %r15
    pushq %r14
    pushq %r13
    pushq %r12
    pushq %rbx
    pushq %rbp
    pushq %rdi
    pushq %rsi
    movq %rsp, (%rcx)
    movq %rdx, %rsp
    testq %r9, %r9
    jz 1f
    xchgq %r9, (%r8)
1:
    testq %r11, %r11
    jz 3f
    movq $64, %rcx
2:
    xorq %rax, %rax
    lock cmpxchgq %r11, (%r10)
    jz 3f
    addq $8, %r10
    decq %rcx
    jnz 2b
3:
    popq %rsi
    popq %rdi
    popq %rbp
    popq %rbx
    popq %r12
    popq %r13
    popq %r14
    popq %r15
    ret

.globl jihoo_user_trampoline_address
jihoo_user_trampoline_address:
    leaq jihoo_user_trampoline(%rip), %rax
    ret
jihoo_user_trampoline:
    swapgs
    iretq

.globl jihoo_kernel_return_address
jihoo_kernel_return_address:
    leaq jihoo_kernel_return(%rip), %rax
    ret
jihoo_kernel_return:
    subq $32, %rsp
    call jihoo_kernel_task_return
    ud2
