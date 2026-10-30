unsafe extern "C" {
    #[link_name = "jihoo_outb"]
    pub fn outb(port: u16, value: u8);
    #[link_name = "jihoo_inb"]
    pub fn inb(port: u16) -> u8;
    #[link_name = "jihoo_outw"]
    pub fn outw(port: u16, value: u16);
    #[link_name = "jihoo_inw"]
    pub fn inw(port: u16) -> u16;
    #[link_name = "jihoo_outl"]
    pub fn outl(port: u16, value: u32);
    #[link_name = "jihoo_inl"]
    pub fn inl(port: u16) -> u32;
    #[link_name = "jihoo_io_wait"]
    pub fn io_wait();
}
