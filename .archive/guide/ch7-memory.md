# Chapter 7: Physical Memory & Paging

This chapter covers how to discover and manage physical RAM, and how to set up the x86_64 4-level page tables for virtual memory.

## What UEFI Gives Us

After `ExitBootServices()`, the UEFI memory map is our only guide to physical RAM. Each descriptor tells us a region's type:

| Type | Meaning |
|------|---------|
| `EFI_CONVENTIONAL_MEMORY` | Free RAM — we can allocate from this |
| `EFI_LOADER_CODE/DATA` | Our own code/data — don't overwrite |
| `EFI_ACPI_RECLAIM_MEMORY` | ACPI tables — must stay mapped |
| `EFI_MEMORY_MAPPED_IO` | Hardware registers (NIC, NVMe, etc.) |
| Everything else | Reserved — don't touch |

## The Frame Allocator

kaguyaOS walks the UEFI memory map to find free pages:

```rust
// src/memory/mod.rs:14-63
pub struct FrameAllocator {
    memory_map: *const u8,       // Pointer to UEFI memory map
    memory_map_size: usize,
    pub descriptor_size: usize,
    pub descriptor_version: u32,
    current_descriptor_index: usize,
    current_page_offset: u64,
}

impl FrameAllocator {
    pub fn allocate_frame(&mut self) -> Option<u64> {
        let num_descriptors = self.memory_map_size / self.descriptor_size;

        while self.current_descriptor_index < num_descriptors {
            let offset = self.current_descriptor_index * self.descriptor_size;
            let descriptor = unsafe {
                &*(self.memory_map.add(offset) as *const EFI_MEMORY_DESCRIPTOR)
            };

            if descriptor.Type == EFI_CONVENTIONAL_MEMORY {
                if self.current_page_offset < descriptor.NumberOfPages {
                    let frame_address =
                        descriptor.PhysicalStart + self.current_page_offset * PAGE_SIZE;
                    self.current_page_offset += 1;
                    if frame_address > 0 {
                        return Some(frame_address);
                    }
                }
            }

            self.current_descriptor_index += 1;
            self.current_page_offset = 0;
        }
        None
    }
}
```

Each call returns the physical address of the next free 4 KiB page. Simple, sequential, and sufficient for a hobby OS.

## Why We Need Paging

Without paging, every physical address equals its virtual address. This means:

- User programs can access kernel memory (no protection)
- Two programs can't have the same virtual address mapped to different physical pages
- No ASLR, no memory isolation

x86_64 uses 4-level page tables: **PML4 → PDPT → PD → PT → Page**. Each level indexes 9 bits of the virtual address, and each table has 512 entries of 8 bytes each.

```
Virtual Address (48 bits used):
┌─────┬─────┬─────┬─────┬────────┐
│PML4 │PDPT │ PD  │ PT  │ Offset │
│ 9b  │ 9b  │ 9b  │ 9b  │  12b   │
└──┬──┴──┬──┴──┬──┴──┬──┴────────┘
   │     │     │     │
   ▼     ▼     ▼     ▼
 PML4T  PDPT   PD    PT    Physical Page
[512]  [512] [512] [512]   (4 KiB)
```

## Page Table Entry Flags

Each page table entry is a 64-bit value: physical address (upper bits) + flags (lower bits):

```rust
// src/memory/mod.rs:7-11
pub const PAGE_PRESENT: u64     = 1 << 0;  // Page is mapped
pub const PAGE_WRITABLE: u64    = 1 << 1;  // Writable
pub const PAGE_USER: u64        = 1 << 2;  // Ring 3 can access
pub const PAGE_CACHE_DISABLE: u64 = 1 << 4; // No caching (MMIO)
pub const PAGE_NO_EXECUTE: u64  = 1 << 63;  // NX bit
```

## Mapping a Page

The `map_page` function walks down the page table tree, allocating intermediate tables as needed:

```rust
// src/memory/mod.rs:173-222 (simplified)
pub unsafe fn map_page(
    pml4: &mut PageTable,
    virt_addr: u64,
    phys_addr: u64,
    flags: u64,
    allocator: &mut FrameAllocator,
) {
    let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
    let pdp_idx  = ((virt_addr >> 30) & 0x1FF) as usize;
    let pd_idx   = ((virt_addr >> 21) & 0x1FF) as usize;
    let pt_idx   = ((virt_addr >> 12) & 0x1FF) as usize;

    // 1. Get or create PDPT
    if (pml4.entries[pml4_idx] & PAGE_PRESENT) == 0 {
        let frame = allocator.allocate_frame().expect("OOM allocating PDPT");
        let table = get_table_mut(frame);
        table.zero();
        pml4.entries[pml4_idx] = frame | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    }
    let pdpt = get_table_mut(pml4.entries[pml4_idx] & !0xFFF);

    // 2. Get or create PD
    // 3. Get or create PT
    // ... (same pattern) ...

    // 4. Set the final mapping
    pt.entries[pt_idx] = phys_addr | flags | PAGE_PRESENT;

    // 5. Flush TLB
    core::arch::asm!("invlpg [{}]", in(reg) virt_addr);
}
```

## Setting Up Paging at Boot

`init_paging()` identity-maps the entire physical address space (so the kernel keeps running) plus the framebuffer:

```rust
// src/memory/mod.rs:270-318 (simplified)
pub unsafe fn init_paging(
    boot_info: &BootInfo,
    allocator: &mut FrameAllocator,
) -> u64 {
    // 1. Allocate PML4
    let pml4_phys = allocator.allocate_frame().expect("Failed to allocate PML4");
    let pml4 = get_table_mut(pml4_phys);
    pml4.zero();

    // 2. Identity map all conventional + loader + ACPI regions
    for i in 0..num_descriptors {
        match descriptor.Type {
            EFI_CONVENTIONAL_MEMORY | EFI_LOADER_CODE | EFI_LOADER_DATA
            | EFI_BOOT_SERVICES_CODE | EFI_BOOT_SERVICES_DATA
            | EFI_ACPI_RECLAIM_MEMORY | EFI_MEMORY_MAPPED_IO => {
                for addr in (start..end).step_by(PAGE_SIZE as usize) {
                    map_page(pml4, addr, addr, PAGE_WRITABLE, allocator);
                }
            }
            _ => {}
        }
    }

    // 3. Map framebuffer
    for addr in (fb_base..fb_base + fb_size).step_by(PAGE_SIZE as usize) {
        map_page(pml4, addr, addr, PAGE_WRITABLE, allocator);
    }

    // 4. Load new page tables
    core::arch::asm!("mov cr3, {}", in(reg) pml4_phys);

    pml4_phys
}
```

**Why identity-map?** The kernel code is loaded at some physical address and running from there. If we don't map `phys == virt`, the instruction pointer becomes invalid after `mov cr3`.

## The Global Allocator Pattern

The frame allocator is created once, used heavily during boot, then its state is saved so `exec` syscalls can resume allocation later:

```rust
// src/memory/mod.rs:94-133
static mut GLOBAL_ALLOCATOR: Option<FrameAllocator> = None;

pub fn new_frame_allocator() -> FrameAllocator {
    // Clone the current global position
}

pub fn commit_frame_allocator(alloc: &FrameAllocator) {
    // Save position back to global
}
```

## kaguyaOS Reference

| File | Lines | What it does |
|------|-------|-------------|
| `src/memory/mod.rs` | 14-63 | FrameAllocator |
| `src/memory/mod.rs` | 148-170 | PageTable struct, get_table_mut |
| `src/memory/mod.rs` | 173-222 | map_page() |
| `src/memory/mod.rs` | 270-318 | init_paging() — full identity map |

---

**Next:** [Chapter 8 — The Kernel Heap](ch8-heap.md) — Dynamic memory allocation for `Vec`, `Box`, and friends.
