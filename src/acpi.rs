//! ACPI (Advanced Configuration and Power Interface) table parsing utilities.
//!
//! Provides safe wrappers for locating and parsing the following ACPI tables:
//!
//! - **RSDP** – Root System Description Pointer (v1 / v2+)
//! - **RSDT / XSDT** – Root / Extended System Description Table
//! - **MADT** – Multiple APIC Description Table (interrupt controllers)
//! - **FADT** – Fixed ACPI Description Table (hardware registers)
//! - **HPET** – High Precision Event Timer table
//! - **MCFG** – PCI Express memory-mapped configuration space table
//!
//! All parsing is done with raw pointer reads over identity-mapped physical
//! memory (as set up by `memory.rs`). No heap allocation is required.

// ─── RSDP ──────────────────────────────────────────────────────────────────

/// ACPI 1.0 RSDP (20 bytes).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct Rsdp {
    /// "RSD PTR "
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    /// 0 = ACPI 1.0, ≥ 2 = ACPI 2.0+
    pub revision: u8,
    /// Physical address of the RSDT (32-bit).
    pub rsdt_address: u32,
}

/// ACPI 2.0+ RSDP extension (36 bytes total).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct RsdpV2 {
    pub v1: Rsdp,
    pub length: u32,
    /// Physical address of the XSDT (64-bit).
    pub xsdt_address: u64,
    pub extended_checksum: u8,
    pub reserved: [u8; 3],
}

// ─── Generic SDT header ─────────────────────────────────────────────────────

/// Common header shared by all System Description Tables.
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct SdtHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

// ─── MADT ───────────────────────────────────────────────────────────────────

/// MADT table header (immediately follows `SdtHeader`).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct MadtHeader {
    pub local_apic_address: u32,
    pub flags: u32,
}

/// Entry types inside the MADT interrupt controller structure list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MadtEntryType {
    LocalApic,
    IoApic,
    InterruptSourceOverride,
    NmiSource,
    LocalApicNmi,
    LocalApicAddressOverride,
    IoSapic,
    LocalSapic,
    PlatformInterruptSource,
    LocalX2Apic,
    LocalX2ApicNmi,
    Gic,
    GicDistributor,
    Unknown(u8),
}

impl From<u8> for MadtEntryType {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::LocalApic,
            1 => Self::IoApic,
            2 => Self::InterruptSourceOverride,
            3 => Self::NmiSource,
            4 => Self::LocalApicNmi,
            5 => Self::LocalApicAddressOverride,
            6 => Self::IoSapic,
            7 => Self::LocalSapic,
            8 => Self::PlatformInterruptSource,
            9 => Self::LocalX2Apic,
            10 => Self::LocalX2ApicNmi,
            11 => Self::Gic,
            12 => Self::GicDistributor,
            other => Self::Unknown(other),
        }
    }
}

/// A raw MADT interrupt-controller entry (type + length prefix).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct MadtEntryHeader {
    pub entry_type: u8,
    pub length: u8,
}

/// Processor Local APIC entry (type 0).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct LocalApicEntry {
    pub header: MadtEntryHeader,
    pub acpi_processor_uid: u8,
    pub apic_id: u8,
    /// Bit 0 = processor enabled, bit 1 = online-capable.
    pub flags: u32,
}

/// I/O APIC entry (type 1).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct IoApicEntry {
    pub header: MadtEntryHeader,
    pub io_apic_id: u8,
    pub reserved: u8,
    pub io_apic_address: u32,
    pub global_system_interrupt_base: u32,
}

/// Interrupt Source Override entry (type 2).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct InterruptSourceOverrideEntry {
    pub header: MadtEntryHeader,
    pub bus: u8,
    /// Source IRQ on the ISA bus.
    pub source: u8,
    /// Global System Interrupt this IRQ maps to.
    pub global_system_interrupt: u32,
    /// MPS INTI flags (polarity + trigger mode).
    pub flags: u16,
}

/// Local APIC NMI entry (type 4).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct LocalApicNmiEntry {
    pub header: MadtEntryHeader,
    pub acpi_processor_uid: u8,
    pub flags: u16,
    /// Local APIC LINT# pin (0 or 1).
    pub local_apic_lint: u8,
}

/// Parsed information extracted from a MADT visit.
#[derive(Debug, Clone, Copy)]
pub struct MadtInfo {
    /// Physical address of the Local APIC MMIO registers.
    pub local_apic_address: u64,
    /// Number of enabled processor Local APICs (= CPU count).
    pub cpu_count: u8,
    /// APIC IDs of each enabled CPU (up to 64).
    pub apic_ids: [u8; 64],
    /// Physical address of the first I/O APIC found.
    pub io_apic_address: u32,
    /// Global System Interrupt base of the first I/O APIC.
    pub io_apic_gsi_base: u32,
}

impl Default for MadtInfo {
    fn default() -> Self {
        Self {
            local_apic_address: 0,
            cpu_count: 0,
            apic_ids: [0u8; 64],
            io_apic_address: 0,
            io_apic_gsi_base: 0,
        }
    }
}

// ─── FADT ───────────────────────────────────────────────────────────────────

/// Generic Address Structure used throughout ACPI 2.0+ tables.
#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct GenericAddress {
    /// 0 = system memory, 1 = system I/O, 2 = PCI config space, …
    pub address_space_id: u8,
    pub register_bit_width: u8,
    pub register_bit_offset: u8,
    pub access_size: u8,
    pub address: u64,
}

/// Fixed ACPI Description Table (FADT / FACP).
///
/// Only the most commonly used fields are declared here; the struct is laid
/// out `packed` so it can be read directly from mapped memory.
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct Fadt {
    pub header: SdtHeader,
    pub firmware_ctrl: u32,
    pub dsdt: u32,
    pub reserved1: u8,
    pub preferred_power_mgmt_profile: u8,
    pub sci_interrupt: u16,
    pub smi_command_port: u32,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
    pub s4bios_req: u8,
    pub pstate_control: u8,
    pub pm1a_event_block: u32,
    pub pm1b_event_block: u32,
    pub pm1a_control_block: u32,
    pub pm1b_control_block: u32,
    pub pm2_control_block: u32,
    pub pm_timer_block: u32,
    pub gpe0_block: u32,
    pub gpe1_block: u32,
    pub pm1_event_length: u8,
    pub pm1_control_length: u8,
    pub pm2_control_length: u8,
    pub pm_timer_length: u8,
    pub gpe0_length: u8,
    pub gpe1_length: u8,
    pub gpe1_base: u8,
    pub cstate_control: u8,
    pub worst_c2_latency: u16,
    pub worst_c3_latency: u16,
    pub flush_size: u16,
    pub flush_stride: u16,
    pub duty_offset: u8,
    pub duty_width: u8,
    pub day_alarm: u8,
    pub month_alarm: u8,
    pub century: u8,
    pub boot_architecture_flags: u16,
    pub reserved2: u8,
    pub flags: u32,
    pub reset_reg: GenericAddress,
    pub reset_value: u8,
    pub arm_boot_arch: u16,
    pub fadt_minor_version: u8,
    pub x_firmware_control: u64,
    pub x_dsdt: u64,
    pub x_pm1a_event_block: GenericAddress,
    pub x_pm1b_event_block: GenericAddress,
    pub x_pm1a_control_block: GenericAddress,
    pub x_pm1b_control_block: GenericAddress,
    pub x_pm2_control_block: GenericAddress,
    pub x_pm_timer_block: GenericAddress,
    pub x_gpe0_block: GenericAddress,
    pub x_gpe1_block: GenericAddress,
}

// ─── HPET ───────────────────────────────────────────────────────────────────

/// HPET Description Table.
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct Hpet {
    pub header: SdtHeader,
    pub event_timer_block_id: u32,
    pub base_address: GenericAddress,
    pub hpet_number: u8,
    pub minimum_tick: u16,
    pub page_protection: u8,
}

// ─── MCFG ───────────────────────────────────────────────────────────────────

/// MCFG table header (follows `SdtHeader`).
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct McfgHeader {
    pub reserved: u64,
}

/// One PCIe configuration space allocation entry inside MCFG.
#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct McfgAllocation {
    /// Base physical address of this segment's enhanced configuration space.
    pub base_address: u64,
    pub pci_segment_group: u16,
    pub start_bus_number: u8,
    pub end_bus_number: u8,
    pub reserved: u32,
}

// ═══════════════════════════════════════════════════════════════════════════
// Utility helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Compute an 8-bit checksum over `len` bytes starting at `ptr`.
///
/// # Safety
/// The caller must ensure `ptr..ptr+len` is valid mapped readable memory.
pub unsafe fn checksum(ptr: *const u8, len: usize) -> u8 {
    let mut sum: u8 = 0;
    for i in 0..len {
        sum = sum.wrapping_add(unsafe { *ptr.add(i) });
    }
    sum
}

/// Verify the checksum of an SDT (or RSDP) region.
///
/// Returns `true` if the region checksums to zero (valid).
///
/// # Safety
/// `ptr` must point to at least `len` bytes of valid readable mapped memory.
pub unsafe fn verify_checksum(ptr: *const u8, len: usize) -> bool {
    unsafe { checksum(ptr, len) == 0 }
}

/// Read a 4-byte ASCII table signature and return it as a `[u8; 4]`.
///
/// # Safety
/// `ptr` must be valid.
pub unsafe fn read_signature(ptr: *const SdtHeader) -> [u8; 4] {
    unsafe { (*ptr).signature }
}

/// Check whether an SDT has the given 4-byte ASCII signature.
pub unsafe fn sdt_has_signature(ptr: *const SdtHeader, sig: &[u8; 4]) -> bool {
    unsafe { &(*ptr).signature == sig }
}

/// Return the total byte length declared in an SDT header.
///
/// # Safety
/// `ptr` must point to a valid `SdtHeader`.
pub unsafe fn sdt_length(ptr: *const SdtHeader) -> usize {
    unsafe { (*ptr).length as usize }
}

// ═══════════════════════════════════════════════════════════════════════════
// RSDP discovery
// ═══════════════════════════════════════════════════════════════════════════

const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";

/// Scan a memory region `[start, start+len)` for the RSDP signature.
///
/// The RSDP is 16-byte aligned in BIOS/UEFI firmware tables. Returns the
/// virtual (identity-mapped) address of the first valid RSDP found, or
/// `None` if none is found.
///
/// # Safety
/// The address range must be identity-mapped and readable.
pub unsafe fn find_rsdp_in_range(start: usize, len: usize) -> Option<*const Rsdp> {
    let mut addr = start;
    while addr + core::mem::size_of::<Rsdp>() <= start + len {
        let ptr = addr as *const u8;
        // Check 8-byte signature.
        let sig: &[u8; 8] = unsafe { &*(ptr as *const [u8; 8]) };
        if sig == RSDP_SIGNATURE {
            // Validate v1 checksum (first 20 bytes).
            if unsafe { verify_checksum(ptr, 20) } {
                return Some(ptr as *const Rsdp);
            }
        }
        addr += 16; // RSDPs are always 16-byte aligned.
    }
    None
}

/// Search the standard BIOS/UEFI regions for the RSDP:
///
/// 1. EBDA (Extended BIOS Data Area) first 1 KiB  
/// 2. BIOS ROM region `0xE0000 – 0xFFFFF`
///
/// Returns a pointer to the RSDP if found.
///
/// # Safety
/// Requires identity-mapped access to the low 1 MiB of physical memory.
pub unsafe fn find_rsdp() -> Option<*const Rsdp> {
    // 1. Try EBDA: read its segment from 0x040E, then search first 1 KiB.
    let ebda_segment = unsafe { (0x040E as *const u16).read_unaligned() } as usize;
    let ebda_base = ebda_segment << 4;
    if ebda_base != 0 {
        if let Some(p) = unsafe { find_rsdp_in_range(ebda_base, 1024) } {
            return Some(p);
        }
    }

    // 2. BIOS ROM region.
    unsafe { find_rsdp_in_range(0xE0000, 0x20000) }
}

/// Given a known RSDP physical address (e.g. from UEFI configuration tables),
/// validate and return a typed pointer.
///
/// # Safety
/// `phys_addr` must be mapped and contain a valid RSDP.
pub unsafe fn rsdp_from_address(phys_addr: u64) -> Option<*const Rsdp> {
    let ptr = phys_addr as *const u8;
    if unsafe { verify_checksum(ptr, 20) } {
        Some(ptr as *const Rsdp)
    } else {
        None
    }
}

/// Return whether the RSDP is ACPI 2.0+ (revision ≥ 2).
pub unsafe fn rsdp_is_v2(rsdp: *const Rsdp) -> bool {
    unsafe { (*rsdp).revision >= 2 }
}

/// Return the physical address of the XSDT from an ACPI 2.0+ RSDP, or `None`
/// if the RSDP is v1.
pub unsafe fn rsdp_xsdt_address(rsdp: *const Rsdp) -> Option<u64> {
    if unsafe { rsdp_is_v2(rsdp) } {
        let v2 = rsdp as *const RsdpV2;
        Some(unsafe { (*v2).xsdt_address })
    } else {
        None
    }
}

/// Return the physical address of the RSDT from the RSDP (always present).
pub unsafe fn rsdp_rsdt_address(rsdp: *const Rsdp) -> u32 {
    unsafe { (*rsdp).rsdt_address }
}

// ═══════════════════════════════════════════════════════════════════════════
// RSDT / XSDT iteration
// ═══════════════════════════════════════════════════════════════════════════

/// Iterate over all child SDT pointers in the RSDT (32-bit entries).
///
/// `callback` receives the physical address of each child table.
///
/// # Safety
/// `rsdt` must point to a valid, identity-mapped RSDT.
pub unsafe fn iter_rsdt<F>(rsdt: *const SdtHeader, mut callback: F)
where
    F: FnMut(u64),
{
    let len = unsafe { sdt_length(rsdt) };
    let entry_count = (len - core::mem::size_of::<SdtHeader>()) / 4;
    let entries = unsafe { (rsdt as *const u8).add(core::mem::size_of::<SdtHeader>()) as *const u32 };
    for i in 0..entry_count {
        let addr = unsafe { entries.add(i).read_unaligned() } as u64;
        callback(addr);
    }
}

/// Iterate over all child SDT pointers in the XSDT (64-bit entries).
///
/// `callback` receives the physical address of each child table.
///
/// # Safety
/// `xsdt` must point to a valid, identity-mapped XSDT.
pub unsafe fn iter_xsdt<F>(xsdt: *const SdtHeader, mut callback: F)
where
    F: FnMut(u64),
{
    let len = unsafe { sdt_length(xsdt) };
    let entry_count = (len - core::mem::size_of::<SdtHeader>()) / 8;
    let entries = unsafe { (xsdt as *const u8).add(core::mem::size_of::<SdtHeader>()) as *const u64 };
    for i in 0..entry_count {
        let addr = unsafe { entries.add(i).read_unaligned() };
        callback(addr);
    }
}

/// Find a child table by its 4-byte signature, searching the RSDT.
///
/// Returns the identity-mapped virtual address (= physical for an
/// identity-mapped kernel) of the matching table header, or `None`.
///
/// # Safety
/// `rsdt` and all child pointers must be identity-mapped and readable.
pub unsafe fn find_table_in_rsdt(rsdt: *const SdtHeader, sig: &[u8; 4]) -> Option<*const SdtHeader> {
    let mut found: Option<*const SdtHeader> = None;
    unsafe {
        iter_rsdt(rsdt, |addr| {
            if found.is_some() {
                return;
            }
            let hdr = addr as *const SdtHeader;
            if sdt_has_signature(hdr, sig) {
                found = Some(hdr);
            }
        });
    }
    found
}

/// Find a child table by its 4-byte signature, searching the XSDT.
///
/// # Safety
/// `xsdt` and all child pointers must be identity-mapped and readable.
pub unsafe fn find_table_in_xsdt(xsdt: *const SdtHeader, sig: &[u8; 4]) -> Option<*const SdtHeader> {
    let mut found: Option<*const SdtHeader> = None;
    unsafe {
        iter_xsdt(xsdt, |addr| {
            if found.is_some() {
                return;
            }
            let hdr = addr as *const SdtHeader;
            if sdt_has_signature(hdr, sig) {
                found = Some(hdr);
            }
        });
    }
    found
}

// ═══════════════════════════════════════════════════════════════════════════
// MADT parsing
// ═══════════════════════════════════════════════════════════════════════════

/// Iterate over every interrupt-controller entry in the MADT, calling
/// `callback` with a pointer to each raw `MadtEntryHeader`.
///
/// # Safety
/// `madt` must point to a valid, fully readable MADT.
pub unsafe fn iter_madt_entries<F>(madt: *const SdtHeader, mut callback: F)
where
    F: FnMut(*const MadtEntryHeader),
{
    let total_len = unsafe { sdt_length(madt) };
    // Skip SDT header + MADT-specific header (local APIC address + flags).
    let start_offset = core::mem::size_of::<SdtHeader>() + core::mem::size_of::<MadtHeader>();
    let base = madt as usize + start_offset;
    let end = madt as usize + total_len;

    let mut cur = base;
    while cur + core::mem::size_of::<MadtEntryHeader>() <= end {
        let entry = cur as *const MadtEntryHeader;
        let entry_len = unsafe { (*entry).length as usize };
        if entry_len < 2 || cur + entry_len > end {
            break; // malformed table
        }
        callback(entry);
        cur += entry_len;
    }
}

/// Parse the MADT and return a compact `MadtInfo` summary.
///
/// # Safety
/// `madt` must point to a valid MADT in identity-mapped memory.
pub unsafe fn parse_madt(madt: *const SdtHeader) -> MadtInfo {
    let madt_hdr_ptr = unsafe {
        (madt as *const u8).add(core::mem::size_of::<SdtHeader>()) as *const MadtHeader
    };
    let mut info = MadtInfo {
        local_apic_address: unsafe { (*madt_hdr_ptr).local_apic_address } as u64,
        ..MadtInfo::default()
    };

    let flags = unsafe { (*madt_hdr_ptr).flags };
    // Bit 0: PCAT compatibility (8259 present).
    let _ = flags;

    unsafe {
        iter_madt_entries(madt, |entry| {
            let entry_type = MadtEntryType::from((*entry).entry_type);
            match entry_type {
                MadtEntryType::LocalApic => {
                    let lapic = entry as *const LocalApicEntry;
                    // Bit 0 = enabled, bit 1 = online capable.
                    if (*lapic).flags & 0x1 != 0 {
                        let idx = info.cpu_count as usize;
                        if idx < 64 {
                            info.apic_ids[idx] = (*lapic).apic_id;
                            info.cpu_count += 1;
                        }
                    }
                }
                MadtEntryType::IoApic => {
                    // Record only the first I/O APIC found.
                    if info.io_apic_address == 0 {
                        let ioapic = entry as *const IoApicEntry;
                        info.io_apic_address = (*ioapic).io_apic_address;
                        info.io_apic_gsi_base = (*ioapic).global_system_interrupt_base;
                    }
                }
                MadtEntryType::LocalApicAddressOverride => {
                    // 64-bit override for the Local APIC address.
                    // Layout: [header(2)] [reserved(2)] [address(8)]
                    let addr_ptr = (entry as *const u8).add(4) as *const u64;
                    info.local_apic_address = unsafe { addr_ptr.read_unaligned() };
                }
                _ => {}
            }
        });
    }

    info
}

// ═══════════════════════════════════════════════════════════════════════════
// FADT helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Return the physical address of the DSDT, preferring the 64-bit field on
/// ACPI 2.0+ tables.
///
/// # Safety
/// `fadt` must point to a valid, readable FADT.
pub unsafe fn fadt_dsdt_address(fadt: *const Fadt) -> u64 {
    let x_dsdt = unsafe { (*fadt).x_dsdt };
    if x_dsdt != 0 {
        x_dsdt
    } else {
        unsafe { (*fadt).dsdt as u64 }
    }
}

/// Return the I/O port address of the PM1a control block.
///
/// Falls back to the 32-bit `pm1a_control_block` field when the extended
/// Generic Address structure reports an I/O-space address of zero.
///
/// # Safety
/// `fadt` must be valid.
pub unsafe fn fadt_pm1a_control_port(fadt: *const Fadt) -> u16 {
    let ext = unsafe { (*fadt).x_pm1a_control_block };
    if ext.address_space_id == 1 && ext.address != 0 {
        ext.address as u16
    } else {
        unsafe { (*fadt).pm1a_control_block as u16 }
    }
}

/// Return the I/O port address of the ACPI PM timer block.
///
/// # Safety
/// `fadt` must be valid.
pub unsafe fn fadt_pm_timer_port(fadt: *const Fadt) -> u32 {
    let ext = unsafe { (*fadt).x_pm_timer_block };
    if ext.address_space_id == 1 && ext.address != 0 {
        ext.address as u32
    } else {
        unsafe { (*fadt).pm_timer_block }
    }
}

/// Read the current 24-bit (or 32-bit) ACPI PM timer value.
///
/// The PM timer runs at 3.579545 MHz. Bit 8 of `flags` indicates whether the
/// counter is 32-bit wide.
///
/// # Safety
/// Requires I/O port access. `port` must be a valid PM timer port.
pub unsafe fn read_pm_timer(port: u32) -> u32 {
    use crate::io::inl;
    unsafe { inl(port as u16) }
}

/// Spin-wait for approximately `microseconds` using the ACPI PM timer.
///
/// # Safety
/// `port` must be the ACPI PM timer I/O port, and I/O access must be
/// permitted.
pub unsafe fn pm_timer_wait_us(port: u32, microseconds: u32) {
    // PM timer frequency = 3,579,545 Hz ≈ 3.58 ticks/µs.
    const TICKS_PER_US: u32 = 4; // conservative ceiling
    let ticks_needed = microseconds.saturating_mul(TICKS_PER_US);
    let start = unsafe { read_pm_timer(port) } & 0x00FF_FFFF;
    loop {
        let now = unsafe { read_pm_timer(port) } & 0x00FF_FFFF;
        let elapsed = now.wrapping_sub(start) & 0x00FF_FFFF;
        if elapsed >= ticks_needed {
            break;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HPET helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Return the physical base address of the HPET MMIO register block.
///
/// # Safety
/// `hpet` must point to a valid HPET table.
pub unsafe fn hpet_base_address(hpet: *const Hpet) -> u64 {
    unsafe { (*hpet).base_address.address }
}

/// Return the HPET number (0-based index in the system).
///
/// # Safety
/// `hpet` must be valid.
pub unsafe fn hpet_number(hpet: *const Hpet) -> u8 {
    unsafe { (*hpet).hpet_number }
}

/// Return the minimum clock tick (in femtoseconds) of the HPET main counter.
///
/// # Safety
/// `hpet` must be valid.
pub unsafe fn hpet_minimum_tick(hpet: *const Hpet) -> u16 {
    unsafe { (*hpet).minimum_tick }
}

// ═══════════════════════════════════════════════════════════════════════════
// MCFG helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Iterate over all PCIe configuration space allocations in the MCFG table.
///
/// `callback` receives a pointer to each `McfgAllocation` entry.
///
/// # Safety
/// `mcfg` must point to a valid, fully readable MCFG table.
pub unsafe fn iter_mcfg_allocations<F>(mcfg: *const SdtHeader, mut callback: F)
where
    F: FnMut(*const McfgAllocation),
{
    let total_len = unsafe { sdt_length(mcfg) };
    let header_size = core::mem::size_of::<SdtHeader>() + core::mem::size_of::<McfgHeader>();
    let alloc_size = core::mem::size_of::<McfgAllocation>();

    if total_len <= header_size {
        return;
    }

    let count = (total_len - header_size) / alloc_size;
    let base = unsafe { (mcfg as *const u8).add(header_size) as *const McfgAllocation };

    for i in 0..count {
        callback(unsafe { base.add(i) });
    }
}

/// Compute the physical base address of the PCIe Enhanced Configuration Space
/// (ECAM) for a given segment group, bus, device, and function.
///
/// Formula: `base + ((bus - start_bus) << 20 | device << 15 | function << 12)`
pub fn ecam_address(
    alloc: &McfgAllocation,
    bus: u8,
    device: u8,
    function: u8,
) -> Option<u64> {
    if bus < alloc.start_bus_number || bus > alloc.end_bus_number {
        return None;
    }
    let offset = ((bus as u64 - alloc.start_bus_number as u64) << 20)
        | ((device as u64) << 15)
        | ((function as u64) << 12);
    Some(alloc.base_address + offset)
}

// ═══════════════════════════════════════════════════════════════════════════
// High-level convenience: initialise from RSDP
// ═══════════════════════════════════════════════════════════════════════════

/// Collection of located ACPI tables (all are physical = virtual for an
/// identity-mapped kernel).
#[derive(Debug, Clone, Copy)]
pub struct AcpiTables {
    pub rsdp: *const Rsdp,
    pub rsdt: Option<*const SdtHeader>,
    pub xsdt: Option<*const SdtHeader>,
    pub madt: Option<*const SdtHeader>,
    pub fadt: Option<*const Fadt>,
    pub hpet: Option<*const Hpet>,
    pub mcfg: Option<*const SdtHeader>,
}

impl AcpiTables {
    /// Discover and locate all known ACPI tables starting from `rsdp`.
    ///
    /// Prefers XSDT over RSDT when both are available (ACPI 2.0+ systems).
    ///
    /// # Safety
    /// All referenced physical addresses must be identity-mapped and readable.
    pub unsafe fn from_rsdp(rsdp: *const Rsdp) -> Self {
        let mut tables = AcpiTables {
            rsdp,
            rsdt: None,
            xsdt: None,
            madt: None,
            fadt: None,
            hpet: None,
            mcfg: None,
        };

        // Prefer XSDT (64-bit) on ACPI 2.0+.
        if let Some(xsdt_addr) = unsafe { rsdp_xsdt_address(rsdp) } {
            if xsdt_addr != 0 {
                let xsdt = xsdt_addr as *const SdtHeader;
                tables.xsdt = Some(xsdt);
                unsafe { tables.locate_tables_in_xsdt(xsdt) };
                return tables;
            }
        }

        // Fall back to RSDT.
        let rsdt_addr = unsafe { rsdp_rsdt_address(rsdp) } as u64;
        if rsdt_addr != 0 {
            let rsdt = rsdt_addr as *const SdtHeader;
            tables.rsdt = Some(rsdt);
            unsafe { tables.locate_tables_in_rsdt(rsdt) };
        }

        tables
    }

    unsafe fn locate_tables_in_xsdt(&mut self, xsdt: *const SdtHeader) {
        unsafe {
            iter_xsdt(xsdt, |addr| {
                let hdr = addr as *const SdtHeader;
                let sig = (*hdr).signature;
                match &sig {
                    b"APIC" => self.madt = Some(hdr),
                    b"FACP" => self.fadt = Some(hdr as *const Fadt),
                    b"HPET" => self.hpet = Some(hdr as *const Hpet),
                    b"MCFG" => self.mcfg = Some(hdr),
                    _ => {}
                }
            });
        }
    }

    unsafe fn locate_tables_in_rsdt(&mut self, rsdt: *const SdtHeader) {
        unsafe {
            iter_rsdt(rsdt, |addr| {
                let hdr = addr as *const SdtHeader;
                let sig = (*hdr).signature;
                match &sig {
                    b"APIC" => self.madt = Some(hdr),
                    b"FACP" => self.fadt = Some(hdr as *const Fadt),
                    b"HPET" => self.hpet = Some(hdr as *const Hpet),
                    b"MCFG" => self.mcfg = Some(hdr),
                    _ => {}
                }
            });
        }
    }

    /// Parse the MADT (if present) and return interrupt topology information.
    ///
    /// # Safety
    /// Requires identity-mapped MADT.
    pub unsafe fn madt_info(&self) -> Option<MadtInfo> {
        self.madt.map(|madt| unsafe { parse_madt(madt) })
    }

    /// Return the physical base address of the HPET MMIO block (if present).
    ///
    /// # Safety
    /// Requires identity-mapped HPET table.
    pub unsafe fn hpet_address(&self) -> Option<u64> {
        self.hpet.map(|hpet| unsafe { hpet_base_address(hpet) })
    }

    /// Return the DSDT physical address (from FADT), if the FADT is present.
    ///
    /// # Safety
    /// Requires identity-mapped FADT.
    pub unsafe fn dsdt_address(&self) -> Option<u64> {
        self.fadt.map(|fadt| unsafe { fadt_dsdt_address(fadt) })
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Diagnostic helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Print a human-readable summary of all discovered ACPI tables via `println!`.
///
/// # Safety
/// All table pointers in `tables` must be valid and readable.
pub unsafe fn dump_tables(tables: &AcpiTables) {
    use crate::println;

    println!("[ACPI] RSDP @ {:#x}", tables.rsdp as u64);

    if let Some(xsdt) = tables.xsdt {
        println!("[ACPI] XSDT @ {:#x}", xsdt as u64);
    } else if let Some(rsdt) = tables.rsdt {
        println!("[ACPI] RSDT @ {:#x}", rsdt as u64);
    }

    if let Some(madt) = tables.madt {
        println!("[ACPI] MADT @ {:#x}", madt as u64);
        let info = unsafe { parse_madt(madt) };
        println!(
            "[ACPI]   Local APIC @ {:#x}, {} CPU(s)",
            info.local_apic_address, info.cpu_count
        );
        for i in 0..info.cpu_count as usize {
            println!("[ACPI]     CPU[{}] APIC ID = {}", i, info.apic_ids[i]);
        }
        if info.io_apic_address != 0 {
            println!(
                "[ACPI]   I/O APIC @ {:#x} (GSI base {})",
                info.io_apic_address, info.io_apic_gsi_base
            );
        }
    }

    if let Some(fadt) = tables.fadt {
        println!("[ACPI] FADT @ {:#x}", fadt as u64);
        let dsdt = unsafe { fadt_dsdt_address(fadt) };
        println!("[ACPI]   DSDT @ {:#x}", dsdt);
    }

    if let Some(hpet) = tables.hpet {
        println!(
            "[ACPI] HPET @ {:#x} (base {:#x})",
            hpet as u64,
            unsafe { hpet_base_address(hpet) }
        );
    }

    if let Some(mcfg) = tables.mcfg {
        println!("[ACPI] MCFG @ {:#x}", mcfg as u64);
        unsafe {
            iter_mcfg_allocations(mcfg, |alloc| {
                // Copy fields out of the packed struct before passing to
                // println! to avoid creating misaligned references.
                let seg  = core::ptr::read_unaligned(core::ptr::addr_of!((*alloc).pci_segment_group));
                let sbus = core::ptr::read_unaligned(core::ptr::addr_of!((*alloc).start_bus_number));
                let ebus = core::ptr::read_unaligned(core::ptr::addr_of!((*alloc).end_bus_number));
                let base = core::ptr::read_unaligned(core::ptr::addr_of!((*alloc).base_address));
                println!(
                    "[ACPI]   PCIe segment {} buses {}-{} base {:#x}",
                    seg, sbus, ebus, base
                );
            });
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Page-mapping helpers (call these before accessing any ACPI data)
// ═══════════════════════════════════════════════════════════════════════════

/// Map the memory regions that `find_rsdp()` must be able to read:
///
/// * The first 1 KiB of the Extended BIOS Data Area (EBDA), whose segment is
///   read from physical address `0x040E`.
/// * The BIOS ROM region `0xE0000 – 0xFFFFF` (128 KiB).
///
/// These regions are usually `EFI_RESERVED_MEMORY_TYPE` and therefore **not**
/// covered by `init_paging`. Call this function right after `init_paging`
/// returns, before calling `find_rsdp()`.
///
/// # Safety
/// * `pml4` must be the active page-table root.
/// * `allocator` must be valid.
pub unsafe fn map_rsdp_scan_regions(
    pml4: &mut crate::memory::PageTable,
    allocator: &mut crate::memory::FrameAllocator,
) {
    use crate::memory::{map_page, PAGE_PRESENT, PAGE_WRITABLE};

    const PAGE: u64 = 4096;
    let flags = PAGE_PRESENT | PAGE_WRITABLE;

    // ── EBDA ──────────────────────────────────────────────────────────────
    // The EBDA segment is stored as a 16-bit value at physical 0x040E.
    // We must map that word's page first so we can read it.
    unsafe { map_page(pml4, 0x0000, 0x0000, flags, allocator) };

    let ebda_segment = unsafe { (0x040E as *const u16).read_unaligned() } as u64;
    let ebda_base = (ebda_segment << 4) & !0xFFF; // align down to page boundary
    if ebda_base != 0 {
        // Map the first two pages of the EBDA (covers the 1 KiB search window).
        unsafe { map_page(pml4, ebda_base, ebda_base, flags, allocator) };
        unsafe { map_page(pml4, ebda_base + PAGE, ebda_base + PAGE, flags, allocator) };
    }

    // ── BIOS ROM: 0xE0000 – 0xFFFFF (32 pages × 4 KiB = 128 KiB) ─────────
    let mut addr: u64 = 0xE0000;
    while addr < 0x100000 {
        unsafe { map_page(pml4, addr, addr, flags, allocator) };
        addr += PAGE;
    }
}

/// Map every page that holds an ACPI table discovered in `tables`.
///
/// `find_rsdp` and `AcpiTables::from_rsdp` only store pointers — they do
/// **not** map the pointed-to memory. Call this after `from_rsdp` returns
/// and before dereferencing any table (including `dump_tables` or
/// `madt_info`).
///
/// Tables are rounded up to 4 KiB boundaries to ensure all bytes are covered.
///
/// # Safety
/// * `pml4` must be the active page-table root.
/// * `allocator` must be valid.
/// * All addresses inside `tables` must be physical addresses in an
///   identity-mapped kernel.
pub unsafe fn map_acpi_tables(
    pml4: &mut crate::memory::PageTable,
    allocator: &mut crate::memory::FrameAllocator,
    tables: &AcpiTables,
) {
    use crate::memory::{map_page, PAGE_PRESENT, PAGE_WRITABLE};

    const PAGE: u64 = 4096;
    let flags = PAGE_PRESENT | PAGE_WRITABLE;

    /// Map `len` bytes starting at `base`, rounding up to full pages.
    unsafe fn map_region(
        pml4: &mut crate::memory::PageTable,
        allocator: &mut crate::memory::FrameAllocator,
        base: u64,
        len: usize,
        flags: u64,
    ) {
        let page_base = base & !0xFFF;
        let page_end = (base + len as u64 + 0xFFF) & !0xFFF;
        let mut addr = page_base;
        while addr < page_end {
            unsafe { map_page(pml4, addr, addr, flags, allocator) };
            addr += 4096;
        }
    }

    // ── RSDP (20 bytes for v1, 36 for v2) ────────────────────────────────
    let rsdp_len = if unsafe { (*tables.rsdp).revision } >= 2 {
        core::mem::size_of::<RsdpV2>()
    } else {
        core::mem::size_of::<Rsdp>()
    };
    unsafe { map_region(pml4, allocator, tables.rsdp as u64, rsdp_len, flags) };

    // Helper: map an SDT given its pointer. The SDT header's `length` field
    // tells us the full table size (including any variable-length payload).
    unsafe fn map_sdt(
        pml4: &mut crate::memory::PageTable,
        allocator: &mut crate::memory::FrameAllocator,
        hdr: *const SdtHeader,
        flags: u64,
    ) {
        // First map just the header so we can safely read `length`.
        unsafe {
            map_region(
                pml4,
                allocator,
                hdr as u64,
                core::mem::size_of::<SdtHeader>(),
                flags,
            )
        };
        // Now read the real length and map the rest.
        let total = unsafe { (*hdr).length as usize };
        if total > core::mem::size_of::<SdtHeader>() {
            unsafe { map_region(pml4, allocator, hdr as u64, total, flags) };
        }
    }

    // ── Root table (XSDT or RSDT) ─────────────────────────────────────────
    if let Some(xsdt) = tables.xsdt {
        unsafe { map_sdt(pml4, allocator, xsdt, flags) };
        // Walk entries and map every child table.
        unsafe {
            iter_xsdt(xsdt, |addr| {
                let child = addr as *const SdtHeader;
                map_sdt(pml4, allocator, child, flags);
            });
        }
    } else if let Some(rsdt) = tables.rsdt {
        unsafe { map_sdt(pml4, allocator, rsdt, flags) };
        unsafe {
            iter_rsdt(rsdt, |addr| {
                let child = addr as *const SdtHeader;
                map_sdt(pml4, allocator, child, flags);
            });
        }
    }
}
