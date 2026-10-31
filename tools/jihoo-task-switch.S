.text
.globl kaguya_context_switch
.type kaguya_context_switch,@function
kaguya_context_switch:
    movq 8(%rsp), %rax
    pushq %r15
    pushq %r14
    pushq %r13
    pushq %r12
    pushq %rbx
    pushq %rbp
    pushq %rdi
    pushq %rsi
    movq %rsp, (%rdi)
    movq %rsi, %rsp
    testq %rax, %rax
    jz 9f
    movb $0, (%rax)
9:
    testq %rcx, %rcx
    jz 1f
    xchgq %rcx, (%rdx)
1:
    testq %r9, %r9
    jz 3f
    movq $64, %r10
2:
    xorq %rax, %rax
    lock cmpxchgq %r9, (%r8)
    jz 3f
    addq $8, %r8
    decq %r10
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
.size kaguya_context_switch, .-kaguya_context_switch
.section .note.GNU-stack,"",@progbits
