#![allow(dead_code)]
#![allow(unused_variables)]

use crate::drivers::pci::PciDevice;
use crate::println;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};

// ============================================================================
// Constants & Opcodes
// ============================================================================

pub const NVME_ADMIN_OP_DELETE_IOSQ: u8 = 0x00;
pub const NVME_ADMIN_OP_CREATE_IOSQ: u8 = 0x01;
pub const NVME_ADMIN_OP_GET_LOG_PAGE: u8 = 0x02;
pub const NVME_ADMIN_OP_DELETE_IOCQ: u8 = 0x04;
pub const NVME_ADMIN_OP_CREATE_IOCQ: u8 = 0x05;
pub const NVME_ADMIN_OP_IDENTIFY: u8 = 0x06;
pub const NVME_ADMIN_OP_ABORT: u8 = 0x08;
pub const NVME_ADMIN_OP_SET_FEATURES: u8 = 0x09;
pub const NVME_ADMIN_OP_GET_FEATURES: u8 = 0x0A;
pub const NVME_ADMIN_OP_ASYNC_EVENT_REQ: u8 = 0x0C;
pub const NVME_ADMIN_OP_NS_MGMT: u8 = 0x0D;
pub const NVME_ADMIN_OP_FW_COMMIT: u8 = 0x10;
pub const NVME_ADMIN_OP_FW_IMAGE_DL: u8 = 0x11;
pub const NVME_ADMIN_OP_DEV_SELF_TEST: u8 = 0x14;
pub const NVME_ADMIN_OP_NS_ATTACH: u8 = 0x15;
pub const NVME_ADMIN_OP_KEEP_ALIVE: u8 = 0x18;
pub const NVME_ADMIN_OP_DIRECTIVE_SEND: u8 = 0x19;
pub const NVME_ADMIN_OP_DIRECTIVE_RECV: u8 = 0x1A;
pub const NVME_ADMIN_OP_VIRT_MGMT: u8 = 0x1C;
pub const NVME_ADMIN_OP_NVME_MI_SEND: u8 = 0x1D;
pub const NVME_ADMIN_OP_NVME_MI_RECV: u8 = 0x1E;
pub const NVME_ADMIN_OP_DOORBELL_BUF_OL: u8 = 0x7C;

pub const NVME_OP_READ: u8 = 0x02;
pub const NVME_OP_WRITE: u8 = 0x01;

// ============================================================================
// Struct Definitions
// ============================================================================

#[repr(C)]
pub struct NvmeRegisters {
    pub cap: u64,   // Controller Capabilities
    pub vs: u32,    // Version
    pub intms: u32, // Interrupt Mask Set
    pub intmc: u32, // Interrupt Mask Clear
    pub cc: u32,    // Controller Configuration
    pub reserved1: u32,
    pub csts: u32,   // Controller Status
    pub nssr: u32,   // NVM Subsystem Reset (Optional)
    pub aqa: u32,    // Admin Queue Attributes
    pub asq: u64,    // Admin Submission Queue Base Address
    pub acq: u64,    // Admin Completion Queue Base Address
    pub cmbloc: u32, // Controller Memory Buffer Location (Optional)
    pub cmbsz: u32,  // Controller Memory Buffer Size (Optional)
    pub bpinfo: u32, // Boot Partition Information
    pub bprsel: u32, // Boot Partition Read Select
    pub bpmbl: u64,  // Boot Partition Memory Buffer Location
    pub cmbmsc: u64, // Controller Memory Buffer Memory Space Control
    pub cmbsts: u32, // Controller Memory Buffer Status
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct NvmeSQEntry {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub nsid: u32,
    pub reserved1: u64,
    pub metadata_ptr: u64,
    pub prp1: u64,
    pub prp2: u64,
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct NvmeCQEntry {
    pub cdw0: u32,
    pub reserved: u32,
    pub sq_head: u16,
    pub sq_id: u16,
    pub command_id: u16,
    pub status: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct NvmeQueue {
    pub id: u16,
    pub tail: u16,
    pub head: u16,
    pub size: u16,
    pub phase: u16,
    pub doorbell_tail: *mut u32,
    pub doorbell_head: *mut u32,
    pub sq_base: *mut NvmeSQEntry,
    pub cq_base: *mut NvmeCQEntry,
}

impl Default for NvmeQueue {
    fn default() -> Self {
        Self {
            id: 0,
            tail: 0,
            head: 0,
            size: 0,
            phase: 1,
            doorbell_tail: core::ptr::null_mut(),
            doorbell_head: core::ptr::null_mut(),
            sq_base: core::ptr::null_mut(),
            cq_base: core::ptr::null_mut(),
        }
    }
}

pub struct NvmeContext {
    pub pci_dev: Option<PciDevice>,
    pub regs: *mut NvmeRegisters,
    pub admin_queue: NvmeQueue,
    pub io_queue: NvmeQueue,
    pub nsid: u32,
}

// ============================================================================
// Global State
// ============================================================================

#[repr(align(4096))]
struct AlignedPage([u8; 4096]);

static mut NVME_CTX: NvmeContext = NvmeContext {
    pci_dev: None,
    regs: core::ptr::null_mut(),
    admin_queue: NvmeQueue {
        id: 0,
        tail: 0,
        head: 0,
        size: 0,
        phase: 1,
        doorbell_tail: core::ptr::null_mut(),
        doorbell_head: core::ptr::null_mut(),
        sq_base: core::ptr::null_mut(),
        cq_base: core::ptr::null_mut(),
    },
    io_queue: NvmeQueue {
        id: 0,
        tail: 0,
        head: 0,
        size: 0,
        phase: 1,
        doorbell_tail: core::ptr::null_mut(),
        doorbell_head: core::ptr::null_mut(),
        sq_base: core::ptr::null_mut(),
        cq_base: core::ptr::null_mut(),
    },
    nsid: 0,
};

static mut ADMIN_SQ_BUFFER: AlignedPage = AlignedPage([0; 4096]);
static mut ADMIN_CQ_BUFFER: AlignedPage = AlignedPage([0; 4096]);
static mut IDENTIFY_BUFFER: AlignedPage = AlignedPage([0; 4096]);
static mut IO_SQ_BUFFER: AlignedPage = AlignedPage([0; 4096]);
static mut IO_CQ_BUFFER: AlignedPage = AlignedPage([0; 4096]);

// ============================================================================
// Helper Functions
// ============================================================================

unsafe fn sleep_stub(count: i32) {
    for _ in 0..(count * 1000) {
        unsafe {
            core::arch::asm!("nop");
        }
    }
}

// Using raw pointers to avoid reference to static mut UB
pub unsafe fn nvme_submit_command(q_ptr: *mut NvmeQueue, cmd: &NvmeSQEntry) {
    unsafe {
        let q = &mut *q_ptr;
        // Copy command to SQ slot
        let slot = q.sq_base.add(q.tail as usize);
        *slot = *cmd;

        // Increment Tail
        q.tail += 1;
        if q.tail >= q.size {
            q.tail = 0;
        }

        // Write Tail Doorbell
        write_volatile(q.doorbell_tail, q.tail as u32);
    }
}

pub unsafe fn nvme_wait_for_completion(q_ptr: *mut NvmeQueue, cid: u16) {
    unsafe {
        let q = &mut *q_ptr;
        loop {
            let entry_ptr = q.cq_base.add(q.head as usize);
            let entry = read_volatile(entry_ptr);

            // Check Phase Tag
            if (entry.status & 0x1) == q.phase {
                // Entry is new
                if entry.command_id == cid {
                    // Handled
                    q.head += 1;
                    if q.head >= q.size {
                        q.head = 0;
                        q.phase = if q.phase == 1 { 0 } else { 1 };
                    }

                    // Ring Head Doorbell
                    write_volatile(q.doorbell_head, q.head as u32);
                    return;
                } else {
                    // Consume other completions
                    q.head += 1;
                    if q.head >= q.size {
                        q.head = 0;
                        q.phase = if q.phase == 1 { 0 } else { 1 };
                    }
                    write_volatile(q.doorbell_head, q.head as u32);
                }
            } else {
                // Wait
                core::hint::spin_loop();
            }
        }
    }
}

unsafe fn nvme_setup_io_queues(ctx_ptr: *mut NvmeContext) {
    let mut cmd = NvmeSQEntry::default();

    unsafe {
        let ctx = &mut *ctx_ptr;
        // 1. Create IO Completion Queue
        cmd.opcode = NVME_ADMIN_OP_CREATE_IOCQ;
        cmd.command_id = 2;
        cmd.prp1 = addr_of_mut!(IO_CQ_BUFFER).cast::<u8>() as u64;
        cmd.cdw10 = ((64 - 1) << 16) | 1; // Size 64, QID 1
        cmd.cdw11 = 1; // Phys Contiguous

        nvme_submit_command(addr_of_mut!(ctx.admin_queue), &cmd);
        nvme_wait_for_completion(addr_of_mut!(ctx.admin_queue), 2);

        // 2. Create IO Submission Queue
        cmd = NvmeSQEntry::default();
        cmd.opcode = NVME_ADMIN_OP_CREATE_IOSQ;
        cmd.command_id = 3;
        cmd.prp1 = addr_of_mut!(IO_SQ_BUFFER).cast::<u8>() as u64;
        cmd.cdw10 = ((64 - 1) << 16) | 1; // Size 64, QID 1
        cmd.cdw11 = (1 << 16) | 1; // CQID 1, Phys Contiguous

        nvme_submit_command(addr_of_mut!(ctx.admin_queue), &cmd);
        nvme_wait_for_completion(addr_of_mut!(ctx.admin_queue), 3);

        // Setup Local Queue Struct
        // We can access fields of ctx directly or via references, since we have &mut *ctx_ptr
        // BUT we need to be careful not to create a reference to ctx if it points to static mut and we access it via static mut elsewhere.
        // Here we just work on ctx which is derived from ctx_ptr.

        ctx.io_queue.id = 1;
        ctx.io_queue.head = 0;
        ctx.io_queue.tail = 0;
        ctx.io_queue.size = 64;
        ctx.io_queue.phase = 1;
        ctx.io_queue.sq_base = addr_of_mut!(IO_SQ_BUFFER).cast::<NvmeSQEntry>();
        ctx.io_queue.cq_base = addr_of_mut!(IO_CQ_BUFFER).cast::<NvmeCQEntry>();

        // Doorbell for QID 1
        let db_base = (ctx.regs as usize) + 0x1000;
        ctx.io_queue.doorbell_tail = (db_base + (2 * 1 * 4)) as *mut u32; // 0x1000 + 8
        ctx.io_queue.doorbell_head = (db_base + (2 * 1 * 4) + 4) as *mut u32; // 0x1000 + 12
    }
}

unsafe fn nvme_identify_controller(ctx_ptr: *mut NvmeContext) {
    let mut cmd = NvmeSQEntry::default();

    unsafe {
        let ctx = &mut *ctx_ptr;
        cmd.opcode = NVME_ADMIN_OP_IDENTIFY;
        cmd.command_id = 1;
        cmd.prp1 = addr_of_mut!(IDENTIFY_BUFFER).cast::<u8>() as u64;
        cmd.cdw10 = 1; // CNS = 1 (Identify Controller)

        nvme_submit_command(addr_of_mut!(ctx.admin_queue), &cmd);
        nvme_wait_for_completion(addr_of_mut!(ctx.admin_queue), 1);

        // Parse Model (byte 24, length 40)
        let buffer_ptr = addr_of_mut!(IDENTIFY_BUFFER).cast::<u8>();
        // buffer[24..64]

        let mut model_str = [0u8; 41];
        for i in 0..40 {
            model_str[i] = *buffer_ptr.add(24 + i);
        }

        // Trim spaces from right
        let mut len = 40;
        while len > 0 && model_str[len - 1] == b' ' {
            model_str[len - 1] = 0;
            len -= 1;
        }

        if let Ok(s) = core::str::from_utf8(&model_str[..len]) {
            println!("NVME MODEL: {}", s);
        } else {
            println!("NVME MODEL: (Invalid UTF-8)");
        }
    }
}

unsafe fn nvme_identify_namespace(ctx_ptr: *mut NvmeContext) {
    unsafe {
        let ctx = &mut *ctx_ptr;
        ctx.nsid = 1;
        println!("NVME: DEFAULT NSID 1 SELECTED");
    }
}

pub unsafe fn init(device: PciDevice) {
    unsafe {
        println!("NVMe: Init started");
        let ctx_ptr = addr_of_mut!(NVME_CTX);
        let ctx = &mut *ctx_ptr;

        ctx.pci_dev = Some(device);

        // 1. Map BAR0
        let mut bar = (device.bar0 as u64) & 0xFFFFFFF0;
        if device.bar1 != 0 {
            bar |= (device.bar1 as u64) << 32;
        }

        println!("NVMe: BAR0 mapped at {:#x}", bar);
        ctx.regs = bar as *mut NvmeRegisters;
        let regs = &mut *ctx.regs;

        // 2. Disable Controller
        let cc = read_volatile(&regs.cc);
        if (cc & 0x1) != 0 {
            write_volatile(&mut regs.cc, cc & !0x1);
        }

        println!("NVMe: Waiting for controller disable...");
        // Wait for CSTS.RDY to become 0
        while (read_volatile(&regs.csts) & 0x1) != 0 {
            sleep_stub(1);
        }
        println!("NVMe: Controller disabled");

        // 3. Configure Admin Queue
        let q_size: u32 = 64;
        write_volatile(&mut regs.aqa, ((q_size - 1) << 16) | (q_size - 1));

        // Zero buffers
        // Use write_bytes on raw pointers
        core::ptr::write_bytes(addr_of_mut!(ADMIN_SQ_BUFFER).cast::<u8>(), 0, 4096);
        core::ptr::write_bytes(addr_of_mut!(ADMIN_CQ_BUFFER).cast::<u8>(), 0, 4096);

        write_volatile(
            &mut regs.asq,
            addr_of_mut!(ADMIN_SQ_BUFFER).cast::<u8>() as u64,
        );
        write_volatile(
            &mut regs.acq,
            addr_of_mut!(ADMIN_CQ_BUFFER).cast::<u8>() as u64,
        );

        // 4. Setup Internal Queue Struct
        ctx.admin_queue.id = 0;
        ctx.admin_queue.head = 0;
        ctx.admin_queue.tail = 0;
        ctx.admin_queue.size = q_size as u16;
        ctx.admin_queue.phase = 1;
        ctx.admin_queue.sq_base = addr_of_mut!(ADMIN_SQ_BUFFER).cast::<NvmeSQEntry>();
        ctx.admin_queue.cq_base = addr_of_mut!(ADMIN_CQ_BUFFER).cast::<NvmeCQEntry>();

        // Doorbell registers
        let db_base = (regs as *mut NvmeRegisters as usize) + 0x1000;
        ctx.admin_queue.doorbell_tail = db_base as *mut u32;
        ctx.admin_queue.doorbell_head = (db_base + 4) as *mut u32;

        println!("NVMe: Admin queue configured");

        // 5. Enable Controller
        let mut new_cc = 0;
        new_cc |= 4 << 20; // IOCQES = 4 (16 bytes)
        new_cc |= 6 << 16; // IOSQES = 6 (64 bytes)
        new_cc |= 1; // EN = 1
        write_volatile(&mut regs.cc, new_cc);

        println!("NVMe: Waiting for controller enable...");
        // Wait for RDY
        while (read_volatile(&regs.csts) & 0x1) == 0 {
            sleep_stub(1);
        }
        println!("NVMe: Controller enabled");

        // 6. Identify Controller
        println!("NVMe: Identifying Controller...");
        nvme_identify_controller(ctx_ptr);

        // 7. Setup IO Queues
        println!("NVMe: Setting up IO Queues...");
        nvme_setup_io_queues(ctx_ptr);

        // 8. Identify Namespace
        nvme_identify_namespace(ctx_ptr);
        println!("NVMe: Init complete");
    }
}

pub unsafe fn nvme_read(nsid: u32, lba: u64, buffer: *mut u8, count: u32) -> i32 {
    let mut cmd = NvmeSQEntry::default();

    cmd.opcode = NVME_OP_READ;
    cmd.command_id = 100;
    cmd.nsid = nsid;
    cmd.prp1 = buffer as u64;
    cmd.cdw10 = lba as u32;
    cmd.cdw11 = (lba >> 32) as u32;
    cmd.cdw12 = (count - 1) & 0xFFFF;

    unsafe {
        let ctx_ptr = addr_of_mut!(NVME_CTX);
        let q_ptr = addr_of_mut!((*ctx_ptr).io_queue);
        nvme_submit_command(q_ptr, &cmd);
        nvme_wait_for_completion(q_ptr, 100);
    }

    0
}

pub unsafe fn nvme_write(nsid: u32, lba: u64, buffer: *mut u8, count: u32) -> i32 {
    let mut cmd = NvmeSQEntry::default();

    cmd.opcode = NVME_OP_WRITE;
    cmd.command_id = 101;
    cmd.nsid = nsid;
    cmd.prp1 = buffer as u64;
    cmd.cdw10 = lba as u32;
    cmd.cdw11 = (lba >> 32) as u32;
    cmd.cdw12 = (count - 1) & 0xFFFF;

    unsafe {
        let ctx_ptr = addr_of_mut!(NVME_CTX);
        let q_ptr = addr_of_mut!((*ctx_ptr).io_queue);
        nvme_submit_command(q_ptr, &cmd);
        nvme_wait_for_completion(q_ptr, 101);
    }

    0
}

pub unsafe fn default_nsid() -> Option<u32> {
    unsafe {
        let ctx_ptr = addr_of_mut!(NVME_CTX);
        let ctx = &*ctx_ptr;
        if ctx.regs.is_null() || ctx.io_queue.size == 0 || ctx.nsid == 0 {
            None
        } else {
            Some(ctx.nsid)
        }
    }
}

pub unsafe fn shutdown() { unsafe {
    let ctx_ptr = addr_of_mut!(NVME_CTX);
    let ctx = &mut *ctx_ptr;
    if !ctx.regs.is_null() {
        println!("NVMe: Shutting down...");
        let regs = &mut *ctx.regs;
        // Set CC.SHN = 01b (Normal Shutdown)
        let mut cc = read_volatile(&regs.cc);
        cc &= !(0x3 << 14);
        cc |= 0x1 << 14;
        write_volatile(&mut regs.cc, cc);

        // Wait for CSTS.SHST = 10b (Shutdown Complete)
        // SHST values: 00=Normal, 01=Occurring, 10=Complete
        let mut timeout = 0;
        while (read_volatile(&regs.csts) >> 2) & 0x3 != 0x2 {
            core::hint::spin_loop();
            timeout += 1;
            if timeout > 10000000 {
                println!("NVMe: Shutdown timeout");
                break;
            }
        }
        println!("NVMe: Shutdown complete");
    }
}}
