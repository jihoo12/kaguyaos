.text
.globl jihoo_ap_blob
jihoo_ap_blob:
    leaq ap_start(%rip), %rax
    ret
.globl jihoo_ap_blob_size
jihoo_ap_blob_size:
    movq $(ap_end-ap_start), %rax
    ret
.globl jihoo_ap_pm_offset
jihoo_ap_pm_offset:
    movq $(ap_pm-ap_start), %rax
    ret
.globl jihoo_ap_lm_offset
jihoo_ap_lm_offset:
    movq $(ap_lm-ap_start), %rax
    ret
.globl jihoo_apic_id
jihoo_apic_id:
    pushq %rbx
    movl $1, %eax
    cpuid
    movl %ebx, %eax
    shrl $24, %eax
    popq %rbx
    ret
.globl jihoo_ap_entry_address
jihoo_ap_entry_address:
    leaq jihoo_ap_entry(%rip), %rax
    ret

.section .rdata,"dr"
.p2align 4
ap_start:
.code16
    cli
    cld
    movw %cs, %ax
    movw %ax, %ds
    movw %ax, %ss
    movw $0xee0, %sp
    movzwl %ax, %ebx
    shll $4, %ebx
    lgdtw 0x100
    movl %cr0, %eax
    orl $1, %eax
    movl %eax, %cr0
    ljmpl *0xf40
.code32
ap_pm:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    leal 0xee0(%ebx), %esp
    movl %cr4, %eax
    orl $0x20, %eax
    movl %eax, %cr4
    movl 0xf08(%ebx), %eax
    movl %eax, %cr3
    movl $0xc0000080, %ecx
    rdmsr
    orl $0x900, %eax
    wrmsr
    movl %cr0, %eax
    orl $0x80000000, %eax
    movl %eax, %cr0
    ljmpl *0xf48(%ebx)
.code64
ap_lm:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    xorw %ax, %ax
    movw %ax, %fs
    movw %ax, %gs
    leaq ap_start(%rip), %rbx
    movq 0xf10(%rbx), %rsp
    movq 0xf20(%rbx), %rcx
    movq 0xf00(%rbx), %rax
    subq $32, %rsp
    call *%rax
    ud2
.org ap_start+0x100
    .word 31
    .long 0
.org ap_start+0x110
    .quad 0
    .quad 0x00cf9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x00af9a000000ffff
ap_end:
