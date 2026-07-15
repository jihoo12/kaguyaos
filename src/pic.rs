use crate::io::{inb, outb, io_wait};

// PIC1 is Master, PIC2 is Slave
const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const PIC_EOI: u8 = 0x20;

// Initialization Command Words
const ICW1_INIT: u8 = 0x10;
const ICW1_ICW4: u8 = 0x01;
const ICW4_8086: u8 = 0x01;

pub unsafe fn init() {
    unsafe {
        // Remap PIC
        // We want Master to start at 32 (0x20) and Slave at 40 (0x28)
        
        // Save masks
        let _a1 = inb(PIC1_DATA);
        let _a2 = inb(PIC2_DATA);
        
        io_wait();
        
        // ICW1: Init
        outb(PIC1_COMMAND, ICW1_INIT | ICW1_ICW4);
        io_wait();
        outb(PIC2_COMMAND, ICW1_INIT | ICW1_ICW4);
        io_wait();
        
        // ICW2: Vector offsets
        outb(PIC1_DATA, 0x20); // Master starts at 32
        io_wait();
        outb(PIC2_DATA, 0x28); // Slave starts at 40
        io_wait();
        
        // ICW3: Cascading
        outb(PIC1_DATA, 4); // Tell Master that Slave is at IRQ2 (0000 0100)
        io_wait();
        outb(PIC2_DATA, 2); // Tell Slave its cascade identity (0000 0010)
        io_wait();
        
        // ICW4: Mode (8086)
        outb(PIC1_DATA, ICW4_8086);
        io_wait();
        outb(PIC2_DATA, ICW4_8086);
        io_wait();
        
        // Restore masks (or set new ones)
        // 0 = Unmasked (Enabled), 1 = Masked (Disabled)
        outb(PIC1_DATA, 0xFC); // 1111 1100 -> Unmask IRQ 0 (Timer) and IRQ 1 (Keyboard)
        outb(PIC2_DATA, 0xFF);

        // ── Configure 8254 PIT Channel 0 for ~100 Hz periodic interrupts ──
        // UEFI disables the PIT during ExitBootServices, so we must
        // reconfigure it to generate the periodic IRQ 0 ticks that the
        // scheduler's `hlt` loop depends on.
        //
        // Base clock: 1_193_182 Hz
        // Divisor:    1_193_182 / 100 = 11_932
        const PIT_FREQ: u32 = 1_193_182;
        const TARGET_HZ: u32 = 100;
        const DIVISOR: u16 = (PIT_FREQ / TARGET_HZ) as u16; // 11932 = 0x2E9C

        const PIT_CMD: u16 = 0x43;
        const PIT_CH0: u16 = 0x40;

        // Command: Channel 0, lobyte/hibyte, mode 3 (square wave)
        outb(PIT_CMD, 0x36);
        io_wait();
        outb(PIT_CH0, (DIVISOR & 0xFF) as u8);       // low byte
        io_wait();
        outb(PIT_CH0, ((DIVISOR >> 8) & 0xFF) as u8); // high byte
    }
}

pub unsafe fn notify_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_COMMAND, PIC_EOI);
        }
        outb(PIC1_COMMAND, PIC_EOI);
    }
}
