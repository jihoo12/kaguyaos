use super::encoder::{Instruction, MemoryAddr, Operand};
use super::registers::Register;
use alloc::vec::Vec;

pub fn parse_register(s: &str) -> Option<Register> {
    match s.to_lowercase().as_str() {
        "rax" => Some(Register::RAX),
        "rcx" => Some(Register::RCX),
        "rdx" => Some(Register::RDX),
        "rbx" => Some(Register::RBX),
        "rsp" => Some(Register::RSP),
        "rbp" => Some(Register::RBP),
        "rsi" => Some(Register::RSI),
        "rdi" => Some(Register::RDI),
        "r8" => Some(Register::R8),
        "r9" => Some(Register::R9),
        "r10" => Some(Register::R10),
        "r11" => Some(Register::R11),
        "r12" => Some(Register::R12),
        "r13" => Some(Register::R13),
        "r14" => Some(Register::R14),
        "r15" => Some(Register::R15),
        _ => None,
    }
}

pub fn parse_operand(s: &str) -> Option<Operand> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Try parsing as register
    if let Some(reg) = parse_register(s) {
        return Some(Operand::Reg(reg));
    }

    // Try parsing as immediate (hex or decimal)
    if let Some(val) = parse_i32(s) {
        return Some(Operand::Imm32(val));
    }
    if let Some(val) = parse_u64(s) {
        return Some(Operand::Imm64(val));
    }

    // Try parsing as memory
    if let Some(mem) = parse_memory(s) {
        return Some(Operand::Mem(mem));
    }

    None
}

pub fn parse_memory(s: &str) -> Option<MemoryAddr> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return None;
    }
    let inner = &s[1..s.len() - 1].trim();

    // Simplistic parsing for [reg], [reg+disp], [reg-disp]
    if let Some(plus_idx) = inner.find('+') {
        let reg_part = inner[..plus_idx].trim();
        let disp_part = inner[plus_idx + 1..].trim();
        let reg = parse_register(reg_part)?;
        let disp = parse_i32(disp_part)?;
        return Some(MemoryAddr {
            base: Some(reg),
            index: None,
            scale: 1,
            disp,
        });
    }

    if let Some(minus_idx) = inner.find('-') {
        let reg_part = inner[..minus_idx].trim();
        let disp_part = inner[minus_idx + 1..].trim();
        let reg = parse_register(reg_part)?;
        let disp = parse_i32(disp_part)?;
        return Some(MemoryAddr {
            base: Some(reg),
            index: None,
            scale: 1,
            disp: -disp,
        });
    }

    // Just [reg]
    if let Some(reg) = parse_register(inner) {
        return Some(MemoryAddr {
            base: Some(reg),
            index: None,
            scale: 1,
            disp: 0,
        });
    }

    // Just [disp] (absolute)
    let disp = parse_i32(inner)?;

    Some(MemoryAddr {
        base: None,
        index: None,
        scale: 1,
        disp,
    })
}

fn strip_hex_prefix(s: &str) -> Option<&str> {
    s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
}

fn parse_i32(s: &str) -> Option<i32> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('-') {
        let value = parse_i32(rest)?;
        value.checked_neg()
    } else if let Some(rest) = s.strip_prefix('+') {
        parse_i32(rest)
    } else if let Some(hex) = strip_hex_prefix(s) {
        i32::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<i32>().ok()
    }
}

fn parse_u64(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.starts_with('-') || s.starts_with('+') {
        return None;
    }
    if let Some(hex) = strip_hex_prefix(s) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

pub fn parse_instruction(line: &str) -> Option<Instruction> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let mut parts = line.split_whitespace();
    let mnemonic = parts.next()?.to_lowercase();
    let rest = line[mnemonic.len()..].trim();

    match mnemonic.as_str() {
        "mov" | "add" | "sub" | "and" | "or" | "xor" | "cmp" | "shl" | "shr" => {
            let operands: Vec<&str> = rest.split(',').collect();
            if operands.len() == 2 {
                let dst = parse_operand(operands[0])?;
                let src = parse_operand(operands[1])?;
                match mnemonic.as_str() {
                    "mov" => Some(Instruction::Mov(dst, src)),
                    "add" => Some(Instruction::Add(dst, src)),
                    "sub" => Some(Instruction::Sub(dst, src)),
                    "and" => Some(Instruction::And(dst, src)),
                    "or" => Some(Instruction::Or(dst, src)),
                    "xor" => Some(Instruction::Xor(dst, src)),
                    "cmp" => Some(Instruction::Cmp(dst, src)),
                    "shl" => Some(Instruction::Shl(dst, src)),
                    "shr" => Some(Instruction::Shr(dst, src)),
                    _ => unreachable!(),
                }
            } else {
                None
            }
        }
        "mul" | "div" | "not" | "call" | "jmp" => {
            let op = parse_operand(rest)?;
            match mnemonic.as_str() {
                "mul" => Some(Instruction::Mul(op)),
                "div" => Some(Instruction::Div(op)),
                "not" => Some(Instruction::Not(op)),
                "call" => Some(Instruction::Call(op)),
                "jmp" => Some(Instruction::Jmp(op)),
                _ => unreachable!(),
            }
        }
        "push" | "pop" => {
            let op = parse_operand(rest)?;
            match mnemonic.as_str() {
                "push" => Some(Instruction::Push(op)),
                "pop" => Some(Instruction::Pop(op)),
                _ => unreachable!(),
            }
        }
        "syscall" => Some(Instruction::Syscall),
        "ret" => Some(Instruction::Ret),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::registers::Register;
    use super::*;

    #[test]
    fn parse_uppercase_hex_immediates() {
        assert_eq!(parse_operand("0X12"), Some(Operand::Imm32(0x12)));
        assert_eq!(
            parse_operand("0X1_0000_0000"),
            None,
            "underscores are not part of tinyasm numeric syntax"
        );
        assert_eq!(
            parse_operand("0X100000000"),
            Some(Operand::Imm64(0x1_0000_0000))
        );
    }

    #[test]
    fn parse_signed_hex_displacements() {
        assert_eq!(
            parse_memory("[rax-0X10]"),
            Some(MemoryAddr {
                base: Some(Register::RAX),
                index: None,
                scale: 1,
                disp: -0x10,
            })
        );
        assert_eq!(
            parse_memory("[-0x20]"),
            Some(MemoryAddr {
                base: None,
                index: None,
                scale: 1,
                disp: -0x20,
            })
        );
    }

    #[test]
    fn parse_instruction_is_case_insensitive() {
        assert_eq!(
            parse_instruction("MOV RAX, 0X2A"),
            Some(Instruction::Mov(
                Operand::Reg(Register::RAX),
                Operand::Imm32(0x2A),
            ))
        );
    }
}
