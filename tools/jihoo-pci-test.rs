use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, PartialEq, Eq, Clone)]
#[repr(C)]
struct Device {
    bus: u8,
    device: u8,
    function: u8,
    vendor_id: u16,
    device_id: u16,
    bar0: u32,
    bar1: u32,
    interrupt_line: u8,
    interrupt_pin: u8,
}

unsafe extern "C" {
    fn jihoo_pci_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32;
    fn jihoo_pci_read16(bus: u8, device: u8, function: u8, offset: u8) -> u16;
    fn jihoo_pci_read8(bus: u8, device: u8, function: u8, offset: u8) -> u8;
    fn jihoo_pci_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32);
    fn jihoo_pci_bar0(bar0: u32, bar1: u32) -> u64;
    fn jihoo_pci_scan(visit: extern "C" fn(u8, u8, u8));
    fn jihoo_pci_find_xhci(index: usize, output: *mut Device) -> bool;
}

#[derive(Default)]
struct Ports {
    selected: u32,
    devices: BTreeMap<u32, [u32; 64]>,
    writes: Vec<(u16, u32)>,
    visited: Vec<(u8, u8, u8)>,
}

static PORTS: OnceLock<Mutex<Ports>> = OnceLock::new();
fn ports() -> std::sync::MutexGuard<'static, Ports> {
    PORTS.get().unwrap().lock().unwrap()
}

#[unsafe(no_mangle)]
extern "C" fn jihoo_outl(port: u16, value: u32) {
    let mut state = ports();
    match port {
        0xcf8 => {
            assert_eq!(value & 0x8000_0003, 0x8000_0000);
            state.selected = value;
        }
        0xcfc => {
            let selected = state.selected;
            state.writes.push((port, value));
            state.devices.get_mut(&(selected & !0xff)).unwrap()[(selected & 0xfc) as usize / 4] =
                value;
        }
        _ => panic!("unexpected port {port:#x}"),
    }
}

#[unsafe(no_mangle)]
extern "C" fn jihoo_inl(port: u16) -> u32 {
    assert_eq!(port, 0xcfc);
    let state = ports();
    state
        .devices
        .get(&(state.selected & !0xff))
        .map(|d| d[(state.selected & 0xfc) as usize / 4])
        .unwrap_or(u32::MAX)
}

extern "C" fn visit(bus: u8, device: u8, function: u8) {
    ports().visited.push((bus, device, function));
}

fn fixture(bus: u8, device: u8, function: u8, vendor: u16, id: u16, class: u32, multi: bool) {
    let mut config = [0; 64];
    config[0] = vendor as u32 | ((id as u32) << 16);
    config[2] = class;
    config[3] = if multi { 0x0080_0000 } else { 0 };
    config[4] = 0xfe00_0004;
    config[5] = 1;
    config[15] = 0x10b;
    config[63] = 0x1234_5678;
    let address =
        0x8000_0000 | ((bus as u32) << 16) | ((device as u32) << 11) | ((function as u32) << 8);
    ports().devices.insert(address, config);
}

fn main() {
    PORTS.set(Mutex::new(Ports::default())).ok().unwrap();
    fixture(0, 2, 0, 0x1b36, 0xd, 0x0c03_3000, false);
    fixture(0, 3, 0, 0x1000, 1, 0, false);
    fixture(0, 3, 1, 0x1000, 2, 0x0200_0000, false); // Not visited: function 0 is single-function.
    fixture(0, 4, 0, 0x1000, 3, 0, true);
    fixture(0, 4, 1, 0x1000, 4, 0x0200_0000, false);
    fixture(255, 31, 0, 0x1000, 5, 0, true);
    fixture(255, 31, 7, 0x1234, 0x5678, 0x0c03_3000, false);
    unsafe {
        assert_eq!(jihoo_pci_read32(255, 31, 7, 0xff), 0x1234_5678);
        assert_eq!(ports().selected, 0x80ff_fffc);
        assert_eq!(jihoo_pci_read16(255, 31, 7, 0xfc), 0x5678);
        assert_eq!(jihoo_pci_read16(255, 31, 7, 0xff), 0x1234);
        for (offset, expected) in [(0xfc, 0x78), (0xfd, 0x56), (0xfe, 0x34), (0xff, 0x12)] {
            assert_eq!(jihoo_pci_read8(255, 31, 7, offset), expected);
        }
        assert_eq!(jihoo_pci_read16(0, 0, 0, 0), 0xffff);
        jihoo_pci_write32(255, 31, 7, 0xff, 0xdead_beef);
        assert_eq!(ports().selected, 0x80ff_fffc);
        assert_eq!(jihoo_pci_read32(255, 31, 7, 0xfc), 0xdead_beef);
        assert_eq!(ports().writes, [(0xcfc, 0xdead_beef)]);
        ports().writes.clear();
        assert_eq!(jihoo_pci_bar0(0xfedc_ba08, u32::MAX), 0xfedc_ba00);
        assert_eq!(
            jihoo_pci_bar0(0x89ab_cd04, 0x0123_4567),
            0x0123_4567_89ab_cd00
        );
        jihoo_pci_scan(visit);
        assert_eq!(
            ports().visited,
            [
                (0, 2, 0),
                (0, 3, 0),
                (0, 4, 0),
                (0, 4, 1),
                (255, 31, 0),
                (255, 31, 7)
            ]
        );
        let mut output = Device {
            bus: 0,
            device: 0,
            function: 0,
            vendor_id: 0,
            device_id: 0,
            bar0: 0,
            bar1: 0,
            interrupt_line: 0,
            interrupt_pin: 0,
        };
        assert_eq!(std::mem::size_of::<Device>(), 20);
        assert_eq!(std::mem::offset_of!(Device, vendor_id), 4);
        assert_eq!(std::mem::offset_of!(Device, bar0), 8);
        assert_eq!(std::mem::offset_of!(Device, interrupt_line), 16);
        assert!(jihoo_pci_find_xhci(0, &mut output));
        assert_eq!(
            output,
            Device {
                bus: 0,
                device: 2,
                function: 0,
                vendor_id: 0x1b36,
                device_id: 0xd,
                bar0: 0xfe00_0004,
                bar1: 1,
                interrupt_line: 11,
                interrupt_pin: 1
            }
        );
        assert!(jihoo_pci_find_xhci(1, &mut output));
        assert_eq!((output.bus, output.device, output.function), (255, 31, 7));
        assert_eq!((output.vendor_id, output.device_id), (0x1234, 0x5678));
        let saved = output.clone();
        assert!(!jihoo_pci_find_xhci(2, &mut output));
        assert_eq!(output, saved, "failed discovery must not initialize output");
        assert!(!jihoo_pci_find_xhci(usize::MAX, &mut output));
        assert!(
            ports().writes.is_empty(),
            "discovery must not enable devices or change config registers"
        );
    }
    println!(
        "jihoo PCI: config widths/addressing, writes, BARs, multifunction scan and read-only indexed xHCI discovery passed"
    );
}
