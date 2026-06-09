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

    // Try parsing as immediate (hex or decimal).
    // Imm32 is tested first intentionally: any value that fits in i32 is
    // emitted as Imm32 even when it would also be a valid u64.  This keeps
    // sign-extended immediates in the smaller encoding and matches the
    // conventional assembler preference for signed 32-bit operands.
    // Only values that overflow i32 (e.g. 0x1_0000_0000) fall through to
    // Imm64.
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
    // `[` and `]` are single-byte ASCII, so byte-slicing here is safe.
    let inner: &str = s[1..s.len() - 1].trim();

    // Full SIB + displacement parsing for:
    //   [reg]
    //   [reg + disp]
    //   [reg - disp]
    //   [reg + index*scale]
    //   [reg + index*scale + disp]
    //   [reg + index*scale - disp]
    //   [disp]  (absolute)
    //
    // Strategy: split on `+` and `-` (keeping the sign), then classify
    // each token as base/index*scale/displacement.
    //
    // We tokenise by walking the string and splitting at every `+` or `-`
    // that is NOT part of a `0x…` literal.  Each token carries the sign
    // that preceded it (the leading token is implicitly positive).
    let mut tokens: Vec<(i32, &str)> = Vec::new(); // (sign, token_str)
    {
        let mut sign = 1i32;
        let mut start = 0usize;
        let bytes = inner.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if (b == b'+' || b == b'-') && i != 0 {
                let tok = inner[start..i].trim();
                if !tok.is_empty() {
                    tokens.push((sign, tok));
                }
                sign = if b == b'-' { -1 } else { 1 };
                start = i + 1;
            }
            i += 1;
        }
        let tok = inner[start..].trim();
        if !tok.is_empty() {
            tokens.push((sign, tok));
        }
    }

    let mut base: Option<Register> = None;
    let mut index: Option<Register> = None;
    let mut scale: u8 = 1;
    let mut disp: i32 = 0;

    for (sign, tok) in tokens {
        // index*scale  e.g. "rbx*4"
        if let Some(star_idx) = tok.find('*') {
            let idx_part = tok[..star_idx].trim();
            let scale_part = tok[star_idx + 1..].trim();
            let idx_reg = parse_register(idx_part)?;
            let s: u8 = scale_part.parse().ok()?;
            if ![1u8, 2, 4, 8].contains(&s) {
                return None;
            }
            index = Some(idx_reg);
            scale = s;
            continue;
        }

        // plain register token
        if let Some(reg) = parse_register(tok) {
            if base.is_none() {
                base = Some(reg);
            } else if index.is_none() {
                index = Some(reg);
                // scale stays 1
            } else {
                return None; // too many registers
            }
            continue;
        }

        // displacement token — honour the sign already captured
        let raw = parse_i32(tok)?;
        let signed = if sign == -1 {
            raw.checked_neg()?
        } else {
            raw
        };
        disp = disp.checked_add(signed)?;
    }

    Some(MemoryAddr {
        base,
        index,
        scale,
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