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
