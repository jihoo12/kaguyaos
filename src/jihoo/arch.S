.text
.globl jihoo_outb
jihoo_outb:
    movb %dl, %al
    movw %cx, %dx
    outb %al, %dx
    ret
.globl jihoo_inb
jihoo_inb:
    movw %cx, %dx
    inb %dx, %al
    ret
.globl jihoo_outw
jihoo_outw:
    movw %dx, %ax
    movw %cx, %dx
    outw %ax, %dx
    ret
.globl jihoo_inw
jihoo_inw:
    movw %cx, %dx
    inw %dx, %ax
    ret
.globl jihoo_outl
jihoo_outl:
    movl %edx, %eax
    movw %cx, %dx
    outl %eax, %dx
    ret
.globl jihoo_inl
jihoo_inl:
    movw %cx, %dx
    inl %dx, %eax
    ret

.globl jihoo_irq_save
jihoo_irq_save:
    pushfq
    popq %rax
    cli
    ret
.globl jihoo_irq_enable
jihoo_irq_enable:
    sti
    ret
.globl jihoo_pause
jihoo_pause:
    pause
    ret
