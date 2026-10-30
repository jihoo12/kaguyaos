unsafe extern "C" {
    fn jihoo_checksum(data: *const u8, length: usize) -> u16;
    fn jihoo_transport_checksum(
        src: u32,
        dst: u32,
        protocol: u8,
        data: *const u8,
        length: usize,
    ) -> u16;
}

pub fn calculate_checksum(data: &[u8]) -> u16 {
    unsafe { jihoo_checksum(data.as_ptr(), data.len()) }
}

pub fn transport_checksum(src: [u8; 4], dst: [u8; 4], protocol: u8, segment: &[u8]) -> u16 {
    unsafe {
        jihoo_transport_checksum(
            u32::from_le_bytes(src),
            u32::from_le_bytes(dst),
            protocol,
            segment.as_ptr(),
            segment.len(),
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kaguya_kernel_panic(message: *const u8, length: usize) -> ! {
    let bytes = unsafe { core::slice::from_raw_parts(message, length) };
    let message = core::str::from_utf8(bytes).unwrap_or("invalid panic message");
    panic!("jihoo kernel: {message}");
}

#[unsafe(no_mangle)]
extern "C" fn kaguya_alloc_frame() -> u64 {
    crate::memory::alloc_frame().unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn kaguya_free_frame(frame: u64) {
    crate::memory::free_frame(frame);
}
