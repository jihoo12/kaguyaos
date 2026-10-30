unsafe extern "C" {
    #[link_name = "jihoo_pic_init"]
    pub fn init();
    #[link_name = "jihoo_pic_eoi"]
    pub fn notify_eoi(irq: u8);
    #[link_name = "jihoo_pic_unmask"]
    pub fn unmask_irq(irq: u8) -> bool;
}
