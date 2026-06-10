#![allow(dead_code)]
#![allow(unused_variables)]

use crate::pci::PciDevice;
use crate::print;
use crate::println;
use core::ptr::{read_volatile, write_volatile};

// xHCI Capability Registers (Offset 0x00 from BAR)
#[repr(C)]
pub struct XhciCapabilityRegisters {
    pub caplength: u8, // Capability Register Length
    pub reserved1: u8,
    pub hciversion: u16, // Interface Version Number
    pub hcsparams1: u32, // Structural Parameters 1
    pub hcsparams2: u32, // Structural Parameters 2
    pub hcsparams3: u32, // Structural Parameters 3
    pub hccparams1: u32, // Capability Parameters 1
    pub dboff: u32,      // Doorbell Offset
    pub rtsoff: u32,     // Runtime Register Space Offset
    pub hccparams2: u32, // Capability Parameters 2
}

// xHCI Operational Registers (Offset CAPLENGTH from BAR)
#[repr(C)]
pub struct XhciOperationalRegisters {
    pub usbcmd: u32,   // USB Command
    pub usbsts: u32,   // USB Status
    pub pagesize: u32, // Page Size
    pub reserved1: [u32; 2],
    pub dnctrl: u32, // Device Notification Control
    pub crcr: u64,   // Command Ring Control Register
    pub reserved2: [u32; 4],
    pub dcbaap: u64, // Device Context Base Address Array Pointer
    pub config: u32, // Configure Register
}

// xHCI Runtime Registers (Offset RTSOFF from BAR)
#[repr(C)]
pub struct XhciRuntimeRegisters {
    pub mfindex: u32, // Microframe Index
    pub reserved1: [u32; 7],
    // Interrupter Register Sets start here (at least 1)
    pub ir: [XhciInterrupterRegisterSet; 1024], // Max 1024 interrupters
}

#[repr(C)]
pub struct XhciInterrupterRegisterSet {
    pub iman: u32,   // Interrupter Management
    pub imod: u32,   // Interrupter Moderation
    pub erstsz: u32, // Event Ring Segment Table Size
    pub reserved: u32,
    pub erstba: u64, // Event Ring Segment Table Base Address
    pub erdp: u64,   // Event Ring Dequeue Pointer
}

#[repr(C)]
pub struct EventRingSegmentTableEntry {
    pub base_address: u64,
    pub size: u16,
    pub reserved: [u16; 3],
}

pub const TRB_COMMAND_COMPLETION_EVENT: u8 = 33;
pub const TRB_PORT_STATUS_CHANGE_EVENT: u8 = 34;

pub const TRB_NOOP_COMMAND: u8 = 23;
pub const TRB_ENABLE_SLOT_COMMAND: u8 = 9;
pub const TRB_ADDRESS_DEVICE_COMMAND: u8 = 11;
pub const TRB_CONFIGURE_ENDPOINT_COMMAND: u8 = 12;
pub const TRB_NORMAL: u8 = 1;
pub const TRB_SETUP_STAGE: u8 = 2;
pub const TRB_DATA_STAGE: u8 = 3;
pub const TRB_STATUS_STAGE: u8 = 4;
pub const TRB_TRANSFER_EVENT: u8 = 32;
pub const TRB_LINK: u8 = 6;

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct Trb {
    pub param: u64,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    pub fn trb_type(&self) -> u8 {
        ((self.control >> 10) & 0x3F) as u8
    }

    pub fn cycle_bit(&self) -> bool {
        (self.control & 1) != 0
    }

    pub fn completion_code(&self) -> u8 {
        (self.status >> 24) as u8
    }

    pub fn slot_id(&self) -> u8 {
        (self.control >> 24) as u8
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SlotContext {
    pub field1: u32,
    pub field2: u32,
    pub field3: u32,
    pub field4: u32,
    pub reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct EndpointContext {
    pub field1: u32,
    pub field2: u32,
    pub tr_dequeue_pointer: u64,
    pub field4: u32,
    pub reserved: [u32; 3],
}

#[repr(C)]
pub struct DeviceContext {
    pub slot: SlotContext,
    pub endpoints: [EndpointContext; 31],
}

#[repr(C)]
pub struct InputContext {
    pub drop_flags: u32,
    pub add_flags: u32,
    pub reserved1: [u32; 5],
    pub field8: u32, // Input Control Context End
    pub device: DeviceContext,
}
pub struct TransferRing {
    pub base: *mut Trb,
    pub size: usize,
    pub enqueue_index: usize,
    pub cycle_bit: bool,
}

impl TransferRing {
    pub fn new(buffer: *mut Trb, size_bytes: usize) -> Self {
        let size = size_bytes / core::mem::size_of::<Trb>();
        unsafe {
            core::ptr::write_bytes(buffer, 0, size_bytes);
            // Setup Link TRB at the end
            let link_trb_ptr = buffer.add(size - 1);
            let mut link_trb = Trb::default();
            link_trb.param = buffer as u64;
            link_trb.control = (TRB_LINK as u32) << 10 | (1 << 1); // TC = 1
            write_volatile(link_trb_ptr, link_trb);
        }
        Self {
            base: buffer,
            size: size - 1, // Number of usable slots
            enqueue_index: 0,
            cycle_bit: true,
        }
    }

    pub unsafe fn enqueue(&mut self, mut trb: Trb, db: *mut u32, endpoint_id: u8) {
        let control = trb.control & !1;
        trb.control = control | (if self.cycle_bit { 1 } else { 0 });

        let trb_ptr = unsafe { self.base.add(self.enqueue_index) };
        unsafe { write_volatile(trb_ptr, trb) };

        self.enqueue_index += 1;
        if self.enqueue_index >= self.size {
            // Update Link TRB cycle bit
            let link_trb_ptr = unsafe { self.base.add(self.size) };
            let mut link_trb = unsafe { read_volatile(link_trb_ptr) };
            link_trb.control = (link_trb.control & !1) | (if self.cycle_bit { 1 } else { 0 });
            unsafe { write_volatile(link_trb_ptr, link_trb) };

            self.enqueue_index = 0;
            self.cycle_bit = !self.cycle_bit;
        }

        // Ring Doorbell
        unsafe { write_volatile(db, endpoint_id as u32) };
    }
}

// Global state for command results (simplified for single-task)
static mut LAST_COMPLETION_CODE: u8 = 0;
static mut LAST_SLOT_ID: u8 = 0;

static mut EP0_TRANSFER_RINGS: [Option<TransferRing>; 64] = [const { None }; 64];
static mut KEYBOARD_TRANSFER_RINGS: [Option<TransferRing>; 64] = [const { None }; 64];

#[repr(align(4096))]
pub struct TransferRingBuffer([Trb; 256]);
pub static mut EP0_TR_BUFFERS: [TransferRingBuffer; 64] = [const {
    TransferRingBuffer(
        [Trb {
            param: 0,
            status: 0,
            control: 0,
        }; 256],
    )
}; 64];

#[repr(align(4096))]
pub struct KeyboardTrBuffers([[Trb; 256]; 64]);
pub static mut KEYBOARD_TR_BUFFERS: KeyboardTrBuffers = KeyboardTrBuffers(
    [[Trb {
        param: 0,
        status: 0,
        control: 0,
    }; 256]; 64],
);

pub struct CommandRing {
    pub base: *mut Trb,
    pub size: usize,
    pub enqueue_index: usize,
    pub cycle_bit: bool,
}

impl CommandRing {
    pub fn new(buffer: *mut Trb, size_bytes: usize) -> Self {
        let size = size_bytes / core::mem::size_of::<Trb>();
        unsafe {
            core::ptr::write_bytes(buffer, 0, size_bytes);
            // Setup Link TRB at the end
            let link_trb_ptr = buffer.add(size - 1);
            let mut link_trb = Trb::default();
            link_trb.param = buffer as u64;
            link_trb.control = (TRB_LINK as u32) << 10 | (1 << 1); // TC = 1
            write_volatile(link_trb_ptr, link_trb);
        }
        Self {
            base: buffer,
            size: size - 1,
            enqueue_index: 0,
            cycle_bit: true,
        }
    }

    pub unsafe fn enqueue(&mut self, mut trb: Trb, db: *mut u32) {
        let control = trb.control & !1;
        trb.control = control | (if self.cycle_bit { 1 } else { 0 });

        let trb_ptr = unsafe { self.base.add(self.enqueue_index) };
        unsafe { write_volatile(trb_ptr, trb) };

        self.enqueue_index += 1;
        if self.enqueue_index >= self.size {
            // Update Link TRB cycle bit
            let link_trb_ptr = unsafe { self.base.add(self.size) };
            let mut link_trb = unsafe { read_volatile(link_trb_ptr) };
            link_trb.control = (link_trb.control & !1) | (if self.cycle_bit { 1 } else { 0 });
            unsafe { write_volatile(link_trb_ptr, link_trb) };

            self.enqueue_index = 0;
            self.cycle_bit = !self.cycle_bit;
        }

        // Ring Doorbell 0 (Host Controller)
        unsafe { write_volatile(db, 0) };
    }
}

pub struct XhciContext {
    pub cap: *const XhciCapabilityRegisters,
    pub op: *mut XhciOperationalRegisters,
    pub rt: *mut XhciRuntimeRegisters,
    pub db: *mut u32, // Doorbell Array Base
    pub cmd_ring: CommandRing,
    pub event_ring_base: *mut Trb,
    pub event_ring_size: usize,
    pub event_ring_dequeue_index: usize,
    pub event_ring_cycle_bit: bool,
    pub max_ports: u8,
}

static mut XHCI_CTX: Option<XhciContext> = None;

#[repr(align(4096))]
pub struct AlignedPage([u8; 4096]);

pub static mut COMMAND_RING_BUFFER: AlignedPage = AlignedPage([0; 4096]);
pub static mut DCBAA_BUFFER: AlignedPage = AlignedPage([0; 4096]);
pub static mut EVENT_RING_SEGMENT_TABLE: AlignedPage = AlignedPage([0; 4096]);
pub static mut EVENT_RING_BUFFER: AlignedPage = AlignedPage([0; 4096]);

// Device Contexts for up to 64 slots. 1024 bytes each.
#[repr(align(4096))]
pub struct DeviceContextBuffer([DeviceContext; 64]);
pub static mut DEVICE_CONTEXT_BUFFERS: DeviceContextBuffer = DeviceContextBuffer(
    [const {
        DeviceContext {
            slot: SlotContext {
                field1: 0,
                field2: 0,
                field3: 0,
                field4: 0,
                reserved: [0; 4],
            },
            endpoints: [EndpointContext {
                field1: 0,
                field2: 0,
                tr_dequeue_pointer: 0,
                field4: 0,
                reserved: [0; 3],
            }; 31],
        }
    }; 64],
);

static mut KEYBOARD_REPORT_BUFFERS: [[u8; 8]; 64] = [[0; 8]; 64];
static mut KEYBOARD_EP_INDICES: [u8; 64] = [0; 64];
static mut PREVIOUS_KEYBOARD_REPORTS: [[u8; 8]; 64] = [[0; 8]; 64];

const KBD_BUF_SIZE: usize = 256;
static mut KEYBOARD_BUFFER: [u8; KBD_BUF_SIZE] = [0; KBD_BUF_SIZE];
static mut KBD_BUF_HEAD: usize = 0;
static mut KBD_BUF_TAIL: usize = 0;

fn push_key(key: u8) {
    unsafe {
        let next_head = (KBD_BUF_HEAD + 1) % KBD_BUF_SIZE;
        if next_head != KBD_BUF_TAIL {
            KEYBOARD_BUFFER[KBD_BUF_HEAD] = key;
            KBD_BUF_HEAD = next_head;
        }
    }
}

pub fn get_key() -> Option<u8> {
    unsafe {
        if KBD_BUF_HEAD == KBD_BUF_TAIL {
            None
        } else {
            let key = KEYBOARD_BUFFER[KBD_BUF_TAIL];
            KBD_BUF_TAIL = (KBD_BUF_TAIL + 1) % KBD_BUF_SIZE;
            Some(key)
        }
    }
}

const HID_ASCII_TABLE: [u8; 128] = {
    let mut table = [0u8; 128];
    table[0x04] = b'a';
    table[0x05] = b'b';
    table[0x06] = b'c';
    table[0x07] = b'd';
    table[0x08] = b'e';
    table[0x09] = b'f';
    table[0x0a] = b'g';
    table[0x0b] = b'h';
    table[0x0c] = b'i';
    table[0x0d] = b'j';
    table[0x0e] = b'k';
    table[0x0f] = b'l';
    table[0x10] = b'm';
    table[0x11] = b'n';
    table[0x12] = b'o';
    table[0x13] = b'p';
    table[0x14] = b'q';
    table[0x15] = b'r';
    table[0x16] = b's';
    table[0x17] = b't';
    table[0x18] = b'u';
    table[0x19] = b'v';
    table[0x1a] = b'w';
    table[0x1b] = b'x';
    table[0x1c] = b'y';
    table[0x1d] = b'z';
    table[0x1e] = b'1';
    table[0x1f] = b'2';
    table[0x20] = b'3';
    table[0x21] = b'4';
    table[0x22] = b'5';
    table[0x23] = b'6';
    table[0x24] = b'7';
    table[0x25] = b'8';
    table[0x26] = b'9';
    table[0x27] = b'0';
    table[0x28] = b'\n'; // Enter
    table[0x29] = 0x1B; // Escape
    table[0x2a] = 0x08; // Backspace
    table[0x2b] = b'\t'; // Tab
    table[0x2c] = b' '; // Space
    table[0x2d] = b'-';
    table[0x2e] = b'=';
    table[0x2f] = b'[';
    table[0x30] = b']';
    table[0x31] = b'\\';
    table[0x35] = b'`';
    table[0x33] = b';';
    table[0x34] = b'\'';
    table[0x36] = b',';
    table[0x37] = b'.';
    table[0x38] = b'/';

    // Arrow Keys
    table[0x4F] = 0x80; // Right
    table[0x50] = 0x81; // Left
    table[0x51] = 0x82; // Down
    table[0x52] = 0x83; // Up

    table
};

const HID_ASCII_SHIFT_TABLE: [u8; 128] = {
    let mut table = [0u8; 128];
    table[0x04] = b'A';
    table[0x05] = b'B';
    table[0x06] = b'C';
    table[0x07] = b'D';
    table[0x08] = b'E';
    table[0x09] = b'F';
    table[0x0a] = b'G';
    table[0x0b] = b'H';
    table[0x0c] = b'I';
    table[0x0d] = b'J';
    table[0x0e] = b'K';
    table[0x0f] = b'L';
    table[0x10] = b'M';
    table[0x11] = b'N';
    table[0x12] = b'O';
    table[0x13] = b'P';
    table[0x14] = b'Q';
    table[0x15] = b'R';
    table[0x16] = b'S';
    table[0x17] = b'T';
    table[0x18] = b'U';
    table[0x19] = b'V';
    table[0x1a] = b'W';
    table[0x1b] = b'X';
    table[0x1c] = b'Y';
    table[0x1d] = b'Z';
    table[0x1e] = b'!';
    table[0x1f] = b'@';
    table[0x20] = b'#';
    table[0x21] = b'$';
    table[0x22] = b'%';
    table[0x23] = b'^';
    table[0x24] = b'&';
    table[0x25] = b'*';
    table[0x26] = b'(';
    table[0x27] = b')';
    table[0x28] = b'\n'; // Enter
    table[0x29] = 0x1B; // Escape
    table[0x2a] = 0x08; // Backspace
    table[0x2b] = b'\t'; // Tab
    table[0x2c] = b' '; // Space
    table[0x2d] = b'_';
    table[0x2e] = b'+';
    table[0x2f] = b'{';
    table[0x30] = b'}';
    table[0x31] = b'|';
    table[0x35] = b'~';
    table[0x33] = b':';
    table[0x34] = b'"';
    table[0x36] = b'<';
    table[0x37] = b'>';
    table[0x38] = b'?';

    // Arrow Keys
    table[0x4F] = 0x80; // Right
    table[0x50] = 0x81; // Left
    table[0x51] = 0x82; // Down
    table[0x52] = 0x83; // Up

    table
};

const HID_MOD_LEFT_SHIFT: u8 = 1 << 1;
const HID_MOD_RIGHT_SHIFT: u8 = 1 << 5;

fn hid_key_to_ascii(key: u8, modifier: u8) -> u8 {
    let table = if modifier & (HID_MOD_LEFT_SHIFT | HID_MOD_RIGHT_SHIFT) != 0 {
        &HID_ASCII_SHIFT_TABLE
    } else {
        &HID_ASCII_TABLE
    };

    if (key as usize) < table.len() {
        table[key as usize]
    } else {
        0
    }
}

// Input Context for command execution (one at a time is fine for now)
pub static mut INPUT_CONTEXT_BUFFER: AlignedPage = AlignedPage([0; 4096]);
pub static mut USB_DATA_BUFFER: AlignedPage = AlignedPage([0; 4096]);

pub unsafe fn init(device: PciDevice) {
    println!("xHCI: Initializing...");

    let mut bar = (device.bar0 as u64) & 0xFFFFFFF0;
    let bar_type = (device.bar0 >> 1) & 0x3;
    if bar_type == 2 {
        bar |= (device.bar1 as u64) << 32;
    }
    println!("xHCI: BAR at {:#x}", bar);

    let cap = bar as *const XhciCapabilityRegisters;
    let caplength = unsafe { read_volatile(&(*cap).caplength) as usize };
    let op = (bar + caplength as u64) as *mut XhciOperationalRegisters;

    let rtsoff = unsafe { read_volatile(&(*cap).rtsoff) & 0xFFFF_FFE0 };
    let rt = (bar + rtsoff as u64) as *mut XhciRuntimeRegisters;

    let dboff = unsafe { read_volatile(&(*cap).dboff) & 0xFFFF_FFFC };
    let db = (bar + dboff as u64) as *mut u32;

    let hccparams1 = unsafe { read_volatile(&(*cap).hccparams1) };
    println!(
        "xHCI: CAPLENGTH={}, RTSOFF={:#x}, DBOFF={:#x}, HCCPARAMS1={:#x}",
        caplength, rtsoff, dboff, hccparams1
    );

    // 1. Reset Controller
    println!("xHCI: Resetting controller...");
    let mut usbcmd = unsafe { read_volatile(&(*op).usbcmd) };
    usbcmd |= 1 << 1; // HCRST: Host Controller Reset
    unsafe { write_volatile(&mut (*op).usbcmd, usbcmd) };

    // Wait for HCRST to become 0
    while (unsafe { read_volatile(&(*op).usbcmd) } & (1 << 1)) != 0 {
        core::hint::spin_loop();
    }
    println!("xHCI: Reset completed");

    // 2. Wait for Controller Not Ready (CNR) to become 0
    println!("xHCI: Waiting for CNR...");
    while (unsafe { read_volatile(&(*op).usbsts) } & (1 << 11)) != 0 {
        core::hint::spin_loop();
    }
    println!("xHCI: Controller ready");

    // 3. Max Device Slots
    let hcsparams1 = unsafe { read_volatile(&(*cap).hcsparams1) };
    let max_slots = hcsparams1 & 0xFF;
    println!("xHCI: Max device slots: {}", max_slots);

    // Enable slots
    let mut config = unsafe { read_volatile(&(*op).config) };
    config &= !0xFF;
    config |= max_slots;
    unsafe { write_volatile(&mut (*op).config, config) };

    // 4. Command Ring Setup
    println!("xHCI: Setting up command ring...");
    let cmd_ring_base = core::ptr::addr_of_mut!(COMMAND_RING_BUFFER).cast::<Trb>();
    let cmd_ring_size = 4096 / core::mem::size_of::<Trb>();

    let cmd_ring = CommandRing::new(cmd_ring_base, 4096);

    let crcr = (cmd_ring_base as u64) | 1; // Bit 0: RCS (Ring Cycle State) = 1
    unsafe { write_volatile(&mut (*op).crcr, crcr) };
    println!(
        "xHCI: Command ring configured at {:#x}",
        cmd_ring_base as u64
    );

    // 5. DCBAA Setup
    println!("xHCI: Setting up DCBAA...");
    let dcbaa_base = core::ptr::addr_of_mut!(DCBAA_BUFFER).cast::<u64>();
    println!("xHCI: DCBAA_BUFFER at {:#x}", dcbaa_base as u64);
    unsafe { core::ptr::write_bytes(dcbaa_base, 0, 4096) };
    unsafe { write_volatile(&mut (*op).dcbaap, dcbaa_base as u64) };

    // 6. Event Ring Setup (Interrupter 0)
    println!("xHCI: Setting up event ring...");
    let erst_base =
        core::ptr::addr_of_mut!(EVENT_RING_SEGMENT_TABLE).cast::<EventRingSegmentTableEntry>();
    let event_ring_base = core::ptr::addr_of_mut!(EVENT_RING_BUFFER).cast::<u8>();
    println!(
        "xHCI: ERST at {:#x}, Event Ring at {:#x}",
        erst_base as u64, event_ring_base as u64
    );

    unsafe {
        println!("XHCI: start write bytes to erst_base");
        core::ptr::write_bytes(erst_base as *mut u8, 0, 4096);
        println!("XHCI: writing");
        core::ptr::write_bytes(event_ring_base as *mut u8, 0, 4096);
    }

    let erst_entry = unsafe { &mut *erst_base };
    erst_entry.base_address = event_ring_base as u64;
    erst_entry.size = (4096 / 16) as u16; // 16 bytes per TRB

    let ir0 = unsafe { &mut (*rt).ir[0] };
    unsafe {
        write_volatile(&mut ir0.erstsz, 1);
        write_volatile(&mut ir0.erstba, erst_base as u64);
        write_volatile(&mut ir0.erdp, event_ring_base as u64);
    }

    // 7. Start Controller
    println!("xHCI: Starting controller...");
    let mut usbcmd = unsafe { read_volatile(&(*op).usbcmd) };
    usbcmd |= 1; // RS: Run/Stop = 1
    unsafe { write_volatile(&mut (*op).usbcmd, usbcmd) };

    while (unsafe { read_volatile(&(*op).usbsts) } & 1) != 0 {
        // Wait for HCH (Host Controller Halted) to become 0
        core::hint::spin_loop();
        break; // In QEMU it might be immediate or we need to be careful with loops
    }
    println!("xHCI: Controller started");

    let max_ports = (hcsparams1 >> 24) as u8;
    println!("xHCI: Max ports: {}", max_ports);

    unsafe {
        XHCI_CTX = Some(XhciContext {
            cap,
            op,
            rt,
            db,
            cmd_ring,
            event_ring_base: event_ring_base as *mut Trb,
            event_ring_size: (4096 / 16),
            event_ring_dequeue_index: 0,
            event_ring_cycle_bit: true,
            max_ports,
        });
    }

    unsafe { poll_ports() };
    unsafe { send_noop_command() };
}

pub unsafe fn poll_ports() {
    let ctx = unsafe {
        (*core::ptr::addr_of!(XHCI_CTX))
            .as_ref()
            .expect("xHCI not initialized")
    };
    let op = ctx.op;
    let max_ports = ctx.max_ports;

    println!("xHCI: Polling {} ports...", max_ports);

    for i in 0..max_ports {
        let portsc_ptr = (op as usize + 0x400 + (i as usize * 0x10)) as *mut u32;
        let portsc = unsafe { read_volatile(portsc_ptr) };
        if (portsc & 1) != 0 {
            // Check if port is enabled (bit 1). If not, reset it.
            if (portsc & (1 << 1)) == 0 {
                println!("xHCI: Resetting port {}...", i + 1);
                unsafe {
                    let mut p = read_volatile(portsc_ptr);
                    p &= 0x0E01_C0E1; // Preserve read-write bits, clear change bits
                    p |= 1 << 4; // PR = 1
                    write_volatile(portsc_ptr, p);
                }
                // Wait for PRC (bit 21)
                while (unsafe { read_volatile(portsc_ptr) } & (1 << 21)) == 0 {
                    core::hint::spin_loop();
                }
                // Clear PRC (write 1 to bit 21)
                unsafe {
                    let mut p = read_volatile(portsc_ptr);
                    p &= 0x0E01_C0E1;
                    p |= 1 << 21;
                    write_volatile(portsc_ptr, p);
                }
            }

            let portsc_final = unsafe { read_volatile(portsc_ptr) };
            let speed = (portsc_final >> 10) & 0xF;
            println!(
                "xHCI: Device detected on port {} (speed {}, enabled={})",
                i + 1,
                speed,
                (portsc_final >> 1) & 1
            );
            let slot_id = unsafe { enable_slot() };
            if slot_id > 0 {
                unsafe { address_device(slot_id, i + 1, speed) };
            }
        }
    }
}

pub unsafe fn process_events() {
    let ctx = match unsafe { &mut *core::ptr::addr_of_mut!(XHCI_CTX) } {
        Some(c) => c,
        None => return,
    };

    let ir0 = unsafe { &mut (*ctx.rt).ir[0] };
    let erdp = unsafe { read_volatile(&ir0.erdp) } & !0xF;
    let mut erdp_val = erdp;

    loop {
        let trb_ptr = erdp_val as *mut Trb;
        let trb = unsafe { read_volatile(trb_ptr) };

        if trb.cycle_bit() != ctx.event_ring_cycle_bit {
            break;
        }

        let event_type = trb.trb_type();

        if event_type == TRB_PORT_STATUS_CHANGE_EVENT {
            let port_id = (trb.param >> 24) as u8;
            println!("xHCI: Event: Port Status Change on port {}", port_id);
        } else if event_type == TRB_COMMAND_COMPLETION_EVENT {
            let code = trb.completion_code();
            let slot_id = trb.slot_id();
            let param = trb.param;
            //println!(
            //    "xHCI: Event: Command Completion. Code={}, SlotID={}, Param={:#x}",
            //    code, slot_id, param
            //);
            unsafe {
                write_volatile(core::ptr::addr_of_mut!(LAST_COMPLETION_CODE), code);
                write_volatile(core::ptr::addr_of_mut!(LAST_SLOT_ID), slot_id);
            }
        } else if event_type == TRB_TRANSFER_EVENT {
            let code = trb.completion_code();
            let slot_id = trb.slot_id();
            let trb_ptr = trb.param;

            // Check if this is a keyboard report (Interrupt In)
            let is_keyboard_report = unsafe {
                let rings_ptr = core::ptr::addr_of_mut!(KEYBOARD_TRANSFER_RINGS);
                if let Some(ref tr) = (*rings_ptr)[slot_id as usize - 1] {
                    trb_ptr >= tr.base as u64 && trb_ptr < (tr.base as u64 + (tr.size * 16) as u64)
                } else {
                    false
                }
            };

            if is_keyboard_report {
                let report = unsafe {
                    &(*core::ptr::addr_of!(KEYBOARD_REPORT_BUFFERS))[slot_id as usize - 1]
                };
                if code == 1 {
                    unsafe {
                        let prev_report =
                            &mut (*core::ptr::addr_of_mut!(PREVIOUS_KEYBOARD_REPORTS))
                                [slot_id as usize - 1];
                        let modifier = report[0];
                        // Process new key presses (Boot Protocol: bytes 2-7 are keycodes)
                        for i in 2..8 {
                            let key = report[i];
                            if key != 0 {
                                // Check if this key was already pressed in previous report
                                let mut found = false;
                                for j in 2..8 {
                                    if prev_report[j] == key {
                                        found = true;
                                        break;
                                    }
                                }
                                if !found {
                                    // New key press!
                                    let ascii = hid_key_to_ascii(key, modifier);
                                    if ascii != 0 {
                                        // print!("{}", ascii as char);
                                        push_key(ascii);
                                    }
                                }
                            }
                        }
                        *prev_report = *report;
                    }

                    // Re-queue request for next report
                    let ep_index = unsafe {
                        (*core::ptr::addr_of!(KEYBOARD_EP_INDICES))[slot_id as usize - 1]
                    };
                    if ep_index != 0 {
                        unsafe { queue_keyboard_report_request(slot_id, ep_index) };
                    }
                } else if code == 6 {
                    println!("xHCI: Keyboard Stall. Attempting to clear...");
                    // In a real driver we would send CLEAR_FEATURE(HALT).
                    // For now, let's just try re-queuing after a while.
                }
            }

            unsafe {
                write_volatile(core::ptr::addr_of_mut!(LAST_COMPLETION_CODE), code);
            }
        } else {
            // println!("xHCI: Event: Other ({})", event_type);
        }

        erdp_val += core::mem::size_of::<Trb>() as u64;
        if erdp_val
            >= ctx.event_ring_base as u64
                + (ctx.event_ring_size * core::mem::size_of::<Trb>()) as u64
        {
            erdp_val = ctx.event_ring_base as u64;
            ctx.event_ring_cycle_bit = !ctx.event_ring_cycle_bit;
        }
    }

    unsafe { write_volatile(&mut ir0.erdp, erdp_val | (1 << 3)) }; // Clear EHB
}

pub unsafe fn send_noop_command() {
    let ctx = match unsafe { &mut *core::ptr::addr_of_mut!(XHCI_CTX) } {
        Some(c) => c,
        None => return,
    };
    println!("xHCI: Sending No-op command...");
    let mut trb = Trb::default();
    trb.control = (TRB_NOOP_COMMAND as u32) << 10;
    unsafe { ctx.cmd_ring.enqueue(trb, ctx.db) };
}

pub unsafe fn enable_slot() -> u8 {
    let ctx = match unsafe { &mut *core::ptr::addr_of_mut!(XHCI_CTX) } {
        Some(c) => c,
        None => panic!("xHCI not initialized"),
    };
    println!("xHCI: Sending Enable Slot command...");
    let mut trb = Trb::default();
    trb.control = (TRB_ENABLE_SLOT_COMMAND as u32) << 10;

    unsafe {
        write_volatile(core::ptr::addr_of_mut!(LAST_COMPLETION_CODE), 0);
        write_volatile(core::ptr::addr_of_mut!(LAST_SLOT_ID), 0);
        ctx.cmd_ring.enqueue(trb, ctx.db);
    }

    // Wait for completion (simple poll)
    while unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } == 0 {
        unsafe { process_events() };
        core::hint::spin_loop();
    }

    let slot_id = unsafe { read_volatile(core::ptr::addr_of!(LAST_SLOT_ID)) };
    println!("xHCI: Assigned Slot ID: {}", slot_id);
    slot_id
}

pub unsafe fn address_device(slot_id: u8, port_id: u8, speed: u32) {
    let ctx = match unsafe { &mut *core::ptr::addr_of_mut!(XHCI_CTX) } {
        Some(c) => c,
        None => panic!("xHCI not initialized"),
    };

    println!(
        "xHCI: Addressing device in slot {} (port {})...",
        slot_id, port_id
    );

    // 1. Initialize Device Context
    let device_ctx_ptr = {
        let buffers_ptr = core::ptr::addr_of_mut!(DEVICE_CONTEXT_BUFFERS);
        let device_ptr = unsafe { core::ptr::addr_of_mut!((*buffers_ptr).0[slot_id as usize - 1]) };
        unsafe {
            core::ptr::write_bytes(
                device_ptr as *mut u8,
                0,
                core::mem::size_of::<DeviceContext>(),
            );
        }
        device_ptr
    };

    // Set Slot Context
    unsafe {
        (*device_ctx_ptr).slot.field1 = (1 << 27) | (speed << 20); // Context Entries = 1, Speed
        (*device_ctx_ptr).slot.field2 = (port_id as u32) << 16; // Root Hub Port Number
    }

    // 2. Initialize EP0 Transfer Ring
    let tr_buffer = {
        let ep0_buffers_ptr = core::ptr::addr_of_mut!(EP0_TR_BUFFERS);
        let buffer_ptr = unsafe {
            core::ptr::addr_of_mut!((*ep0_buffers_ptr)[slot_id as usize - 1].0) as *mut Trb
        };
        buffer_ptr
    };
    let tr = TransferRing::new(tr_buffer, 4096);
    let rings_ptr = core::ptr::addr_of_mut!(EP0_TRANSFER_RINGS);
    unsafe {
        (*rings_ptr)[slot_id as usize - 1] = Some(tr);
    }

    // Set EP0 Context
    let mps = match speed {
        3 => 64,  // High Speed
        4 => 512, // Super Speed
        2 => 8,   // Low Speed
        _ => 64,  // Full Speed default
    };
    unsafe {
        (*device_ctx_ptr).endpoints[0].field2 = (4 << 3) | (mps << 16) | (3 << 1); // EP Type = Control, MPS, CErr = 3
        (*device_ctx_ptr).endpoints[0].tr_dequeue_pointer = (tr_buffer as u64) | 1; // DCS = 1
    }

    // 3. Set DCBAA entry
    let dcbaa = unsafe { read_volatile(&(*ctx.op).dcbaap) } as *mut u64;
    unsafe {
        write_volatile(
            dcbaa.add(slot_id as usize),
            device_ctx_ptr as *const _ as u64,
        );
    }

    // 4. Prepare Input Context
    let input_ctx_ptr = {
        let ptr = core::ptr::addr_of_mut!(INPUT_CONTEXT_BUFFER) as *mut InputContext;
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, 4096);
            (*ptr).add_flags = 0x3; // Add Slot Context (bit 0) and EP0 Context (bit 1)
            core::ptr::copy_nonoverlapping(
                device_ctx_ptr as *const u8,
                core::ptr::addr_of_mut!((*ptr).device) as *mut u8,
                core::mem::size_of::<DeviceContext>(),
            );
        }
        ptr
    };

    // 5. Send Address Device Command
    let mut trb = Trb::default();
    trb.param = input_ctx_ptr as u64;
    trb.control = (TRB_ADDRESS_DEVICE_COMMAND as u32) << 10 | (slot_id as u32) << 24; // BSR = 0

    unsafe {
        write_volatile(core::ptr::addr_of_mut!(LAST_COMPLETION_CODE), 0);
        ctx.cmd_ring.enqueue(trb, ctx.db);
    }

    // Wait for completion
    while unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } == 0 {
        unsafe { process_events() };
        core::hint::spin_loop();
    }

    println!("xHCI: Address Device completed with code {}", unsafe {
        read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE))
    });

    if unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } == 1 {
        unsafe { get_descriptor(slot_id) };
    }
}

pub unsafe fn control_transfer(
    slot_id: u8,
    setup: [u8; 8],
    data: *mut u8,
    data_len: u16,
    is_in: bool,
) {
    let ctx = match unsafe { &mut *core::ptr::addr_of_mut!(XHCI_CTX) } {
        Some(c) => c,
        None => panic!("xHCI not initialized"),
    };
    let db = unsafe { ctx.db.add(slot_id as usize) };
    let rings_ptr = core::ptr::addr_of_mut!(EP0_TRANSFER_RINGS);
    let tr = unsafe { (*rings_ptr)[slot_id as usize - 1].as_mut().unwrap() };

    // 1. Setup Stage
    let mut setup_trb = Trb::default();
    setup_trb.param = u64::from_le_bytes(setup);
    setup_trb.status = 8; // TRB Transfer Length = 8
    let mut control = (TRB_SETUP_STAGE as u32) << 10 | (1 << 6); // IDT = 1
    if data_len > 0 {
        control |= (if is_in { 3 } else { 2 }) << 16; // TRT: 3=In, 2=Out
    }
    setup_trb.control = control;
    unsafe { tr.enqueue(setup_trb, db, 1) }; // EP0 Endpoint ID = 1
    // println!("xHCI: Setup stage enqueued");

    // 2. Data Stage (optional)
    if data_len > 0 {
        let mut data_trb = Trb::default();
        data_trb.param = data as u64;
        data_trb.status = data_len as u32;
        data_trb.control = (TRB_DATA_STAGE as u32) << 10 | (if is_in { 1 } else { 0 }) << 16;
        unsafe { tr.enqueue(data_trb, db, 1) };
        // println!("xHCI: Data stage enqueued");
    }

    // 3. Status Stage
    let mut status_trb = Trb::default();
    status_trb.control = (TRB_STATUS_STAGE as u32) << 10
        | (if is_in && data_len > 0 { 0 } else { 1 }) << 16
        | (1 << 5); // IOC = 1
    unsafe {
        write_volatile(core::ptr::addr_of_mut!(LAST_COMPLETION_CODE), 0);
        tr.enqueue(status_trb, db, 1);
    }
    // println!("xHCI: Status stage enqueued");

    // Wait for completion
    let mut timeout = 0;
    while unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } == 0 {
        unsafe { process_events() };
        core::hint::spin_loop();
        timeout += 1;
        if timeout > 2000000 {
            println!("xHCI: Control transfer timeout!");
            break;
        }
    }
}

pub unsafe fn get_descriptor(slot_id: u8) {
    println!("xHCI: Getting Device Descriptor for slot {}...", slot_id);

    // GET_DESCRIPTOR: 80 06 00 01 00 00 12 00
    let setup: [u8; 8] = [
        0x80, // bmRequestType: Device-to-Host, Standard, Device
        0x06, // bRequest: GET_DESCRIPTOR
        0x00, 0x01, // wValue: Descriptor Type (01) / Index (00)
        0x00, 0x00, // wIndex: 0
        18, 0x00, // wLength: 18
    ];

    let buffer = core::ptr::addr_of_mut!(USB_DATA_BUFFER) as *mut u8;
    unsafe {
        core::ptr::write_bytes(buffer, 0, 18);
        control_transfer(slot_id, setup, buffer, 18, true);
    }

    let code = unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) };
    if code == 1 {
        let vendor_id = unsafe { u16::from_le_bytes([*buffer.add(8), *buffer.add(9)]) };
        let product_id = unsafe { u16::from_le_bytes([*buffer.add(10), *buffer.add(11)]) };
        println!(
            "xHCI: Device Descriptor: VendorID={:#06x}, ProductID={:#06x}",
            vendor_id, product_id
        );

        // Fetch Configuration Descriptor
        unsafe { get_config_descriptor(slot_id) };
    } else {
        println!("xHCI: GET_DESCRIPTOR (Device) failed with code {}", code);
    }
}

pub unsafe fn get_config_descriptor(slot_id: u8) {
    println!(
        "xHCI: Getting Configuration Descriptor for slot {}...",
        slot_id
    );

    // 1. Get first 9 bytes to know total length
    let setup: [u8; 8] = [
        0x80, // bmRequestType
        0x06, // GET_DESCRIPTOR
        0x00, 0x02, // wValue: Config Descriptor (02)
        0x00, 0x00, // wIndex
        0x09, 0x00, // wLength: 9
    ];

    let buffer = unsafe { core::ptr::addr_of_mut!(USB_DATA_BUFFER) as *mut u8 };
    unsafe {
        core::ptr::write_bytes(buffer, 0, 9);
        control_transfer(slot_id, setup, buffer, 9, true);
    }

    if unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } != 1 {
        println!("xHCI: GET_DESCRIPTOR (Config Header) failed");
        return;
    }

    let total_len = unsafe { u16::from_le_bytes([*buffer.add(2), *buffer.add(3)]) };
    println!("xHCI: Config descriptor total length: {}", total_len);

    // 2. Get full configuration descriptor
    let mut setup_full = setup;
    setup_full[6] = (total_len & 0xFF) as u8;
    setup_full[7] = ((total_len >> 8) & 0xFF) as u8;

    unsafe {
        core::ptr::write_bytes(buffer, 0, total_len as usize);
        control_transfer(slot_id, setup_full, buffer, total_len, true);
    }

    if unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } != 1 {
        println!("xHCI: GET_DESCRIPTOR (Config Full) failed");
        return;
    }

    // 3. Parse descriptors to find Interrupt In endpoint
    let mut offset = 0;
    while offset < total_len as usize {
        let len = unsafe { *buffer.add(offset) } as usize;
        let type_ = unsafe { *buffer.add(offset + 1) };

        if type_ == 0x04 {
            // Interface Descriptor
            let class = unsafe { *buffer.add(offset + 5) };
            let sub_class = unsafe { *buffer.add(offset + 6) };
            let protocol = unsafe { *buffer.add(offset + 7) };
            println!(
                "xHCI: Interface: Class={}, SubClass={}, Protocol={}",
                class, sub_class, protocol
            );
        } else if type_ == 0x05 {
            // Endpoint Descriptor
            let addr = unsafe { *buffer.add(offset + 2) };
            let attr = unsafe { *buffer.add(offset + 3) };
            let mps =
                unsafe { u16::from_le_bytes([*buffer.add(offset + 4), *buffer.add(offset + 5)]) };
            let interval = unsafe { *buffer.add(offset + 6) };

            let is_in = (addr & 0x80) != 0;
            let ep_type = attr & 0x03; // 0=Control, 1=Isoch, 2=Bulk, 3=Interrupt

            println!(
                "xHCI: Endpoint: Addr={:#x}, Attr={}, MPS={}, Interval={}",
                addr, attr, mps, interval
            );

            if is_in && ep_type == 3 {
                println!("xHCI: Found Interrupt In endpoint: {:#x}", addr);
                let ep_index = (addr & 0x0F) * 2 + 1;
                unsafe {
                    (*core::ptr::addr_of_mut!(KEYBOARD_EP_INDICES))[slot_id as usize - 1] =
                        ep_index;
                    setup_keyboard_endpoint(slot_id, ep_index, mps, interval);
                }
                return;
            }
        }
        offset += len;
        if len == 0 {
            break;
        }
    }
}

pub unsafe fn setup_keyboard_endpoint(slot_id: u8, ep_index: u8, mps: u16, interval: u8) {
    println!(
        "xHCI: Setting up keyboard endpoint (Slot={}, Index={}, MPS={}, Interval={})...",
        slot_id, ep_index, mps, interval
    );

    // 1. Set Configuration
    let setup_config: [u8; 8] = [
        0x00, // bmRequestType: Host-to-Device, Standard, Device
        0x09, // bRequest: SET_CONFIGURATION
        0x01, 0x00, // wValue: Config Value 1
        0x00, 0x00, // wIndex
        0x00, 0x00, // wLength
    ];
    unsafe { control_transfer(slot_id, setup_config, core::ptr::null_mut(), 0, false) };

    if unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } != 1 {
        println!("xHCI: SET_CONFIGURATION failed");
        return;
    }
    println!("xHCI: Configuration set to 1");

    // 1.1 Set Idle (0 = infinity)
    let setup_idle: [u8; 8] = [
        0x21, // bmRequestType: Host-to-Device, Class, Interface
        0x0A, // SET_IDLE
        0x00, 0x00, // wValue: Duration=0, ReportID=0
        0x00, 0x00, // wIndex: Interface 0
        0x00, 0x00, // wLength
    ];
    unsafe { control_transfer(slot_id, setup_idle, core::ptr::null_mut(), 0, false) };
    println!("xHCI: SET_IDLE completed");

    // 1.2 Set Protocol (0 = Boot Protocol)
    let setup_protocol: [u8; 8] = [
        0x21, // bmRequestType
        0x0B, // SET_PROTOCOL
        0x00, 0x00, // wValue: 0 (Boot Protocol)
        0x00, 0x00, // wIndex
        0x00, 0x00, // wLength
    ];
    unsafe { control_transfer(slot_id, setup_protocol, core::ptr::null_mut(), 0, false) };
    println!("xHCI: SET_PROTOCOL completed");

    // 2. Initialize Interrupt In Transfer Ring
    let tr_buffer = unsafe {
        let buffers_ptr = core::ptr::addr_of_mut!(KEYBOARD_TR_BUFFERS);
        &mut (*buffers_ptr).0[slot_id as usize - 1] as *mut [Trb; 256] as *mut Trb
    };
    let tr = TransferRing::new(tr_buffer, 4096);
    unsafe {
        let rings_ptr = core::ptr::addr_of_mut!(KEYBOARD_TRANSFER_RINGS);
        (*rings_ptr)[slot_id as usize - 1] = Some(tr);
    }

    // 3. Prepare Input Context for Configure Endpoint
    let ctx = match unsafe { &mut *core::ptr::addr_of_mut!(XHCI_CTX) } {
        Some(c) => c,
        None => return,
    };

    let device_ctx_ptr = unsafe {
        let buffers_ptr = core::ptr::addr_of_mut!(DEVICE_CONTEXT_BUFFERS);
        &mut (*buffers_ptr).0[slot_id as usize - 1] as *mut DeviceContext
    };

    let input_ctx_ptr = unsafe {
        let ptr = core::ptr::addr_of_mut!(INPUT_CONTEXT_BUFFER) as *mut InputContext;
        core::ptr::write_bytes(ptr as *mut u8, 0, 4096);
        // Add Flags: bit 0 (Slot), bit ep_index (Endpoint)
        (*ptr).add_flags = 1 | (1 << ep_index);

        // Copy Slot Context
        core::ptr::copy_nonoverlapping(
            &(*device_ctx_ptr).slot as *const _ as *const u8,
            &mut (*ptr).device.slot as *mut _ as *mut u8,
            core::mem::size_of::<SlotContext>(),
        );

        // Update Slot Context in Input Context (Context Entries)
        let entries = ep_index; // index of the last valid endpoint context
        (*ptr).device.slot.field1 =
            ((*ptr).device.slot.field1 & !(0x1F << 27)) | ((entries as u32) << 27);

        // Setup Endpoint Context in Input Context
        let ep_ctx = &mut (*ptr).device.endpoints[ep_index as usize - 1];
        ep_ctx.field2 = (7 << 3) | ((mps as u32) << 16) | (3 << 1); // EP Type = Interrupt In (7), MPS, CErr = 3
        ep_ctx.tr_dequeue_pointer = (tr_buffer as u64) | 1; // DCS = 1
        ep_ctx.field4 = (interval as u32 - 1) << 16; // Interval (logged scale) - for HS: 2^(val) * 125us
        // Note: For FS/LS it's 1ms units. Keyboard HS usually has interval 7-9 (1ms-4ms).
        ptr
    };

    // 4. Send Configure Endpoint Command
    let mut trb = Trb::default();
    trb.param = input_ctx_ptr as u64;
    trb.control = (TRB_CONFIGURE_ENDPOINT_COMMAND as u32) << 10 | (slot_id as u32) << 24;

    unsafe {
        write_volatile(core::ptr::addr_of_mut!(LAST_COMPLETION_CODE), 0);
        ctx.cmd_ring.enqueue(trb, ctx.db);
    }

    // Wait for completion
    while unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } == 0 {
        unsafe { process_events() };
        core::hint::spin_loop();
    }

    if unsafe { read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE)) } == 1 {
        println!("xHCI: Interrupt In endpoint configured");
        // Start polling for reports
        unsafe { queue_keyboard_report_request(slot_id, ep_index) };
    } else {
        println!("xHCI: CONFIGURE_ENDPOINT failed with code {}", unsafe {
            read_volatile(core::ptr::addr_of!(LAST_COMPLETION_CODE))
        });
    }
}

pub unsafe fn queue_keyboard_report_request(slot_id: u8, ep_index: u8) {
    let ctx = match unsafe { &mut *core::ptr::addr_of_mut!(XHCI_CTX) } {
        Some(c) => c,
        None => return,
    };
    let db = unsafe { ctx.db.add(slot_id as usize) };
    let tr = unsafe {
        let rings_ptr = core::ptr::addr_of_mut!(KEYBOARD_TRANSFER_RINGS);
        (*rings_ptr)[slot_id as usize - 1].as_mut().unwrap()
    };
    let buffer = unsafe {
        let buffers_ptr = core::ptr::addr_of_mut!(KEYBOARD_REPORT_BUFFERS);
        &mut (*buffers_ptr)[slot_id as usize - 1] as *mut u8
    };

    let mut trb = Trb::default();
    trb.param = buffer as u64;
    trb.status = 8; // Request 8 bytes
    trb.control = (TRB_NORMAL as u32) << 10 | (1 << 5); // IOC = 1
    // println!("xHCI: Queueing report request for slot {}, ep {}", slot_id, ep_index);
    unsafe { tr.enqueue(trb, db, ep_index) };
}

pub unsafe fn shutdown() {
    if let Some(ctx) = unsafe { &mut *core::ptr::addr_of_mut!(XHCI_CTX) } {
        println!("xHCI: Shutting down...");
        let op = &mut *ctx.op;
        // Stop Controller (RS = 0)
        let mut cmd = read_volatile(&op.usbcmd);
        cmd &= !1;
        write_volatile(&mut op.usbcmd, cmd);

        // Wait for Halted (HCH = 1)
        let mut timeout = 0;
        while (read_volatile(&op.usbsts) & 1) == 0 {
            core::hint::spin_loop();
            timeout += 1;
            if timeout > 10000000 {
                println!("xHCI: Shutdown timeout");
                break;
            }
        }
        println!("xHCI: Shutdown complete");
    }
}
