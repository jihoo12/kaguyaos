// PORTSC writes must preserve ordinary RW fields without echoing W1C/W1S
// status/action bits. In particular PP (bit 9) must survive a port reset.
pub const fn neutral(value: u32) -> u32 {
    value & ((15 << 5) | (1 << 9) | (3 << 14) | (7 << 25))
}

#[cfg(test)]
mod tests {
    use super::neutral;

    #[test]
    fn reset_and_ack_keep_a_powered_port_on() {
        let before = 0x0002_0603; // connected, enabled, powered, full-speed, CSC
        let reset = neutral(before) | (1 << 4) | (1 << 21);
        assert_eq!(reset & (1 << 9), 1 << 9);
        assert_eq!(reset & 2, 0); // Writing PED=1 disables the port.
        assert_eq!(reset & (1 << 17), 0); // Do not acknowledge unrelated changes.
        let completed = before | (1 << 21);
        let ack = neutral(completed) | (1 << 21);
        assert_eq!(ack & (1 << 9), 1 << 9);
        assert_eq!(ack & ((1 << 4) | (1 << 16) | 2), 0);
    }

    #[test]
    fn status_bits_cannot_trigger_unrequested_actions() {
        let value = neutral(u32::MAX);
        assert_eq!(value & ((1 << 31) | (1 << 16) | (0x7f << 17) | (1 << 4) | 2), 0);
        assert_eq!(value & ((7 << 25) | (3 << 14) | (15 << 5)), (7 << 25) | (3 << 14) | (15 << 5));
    }
}
