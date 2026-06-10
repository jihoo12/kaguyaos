use super::encoder::{AsmLine, ConditionCode, Instruction, MemoryAddr, Operand};
use super::registers::Register;
use alloc::string::String;
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
    let s = strip_size_prefix(s.trim());
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
    let s = strip_size_prefix(s.trim());
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
            if sign == -1 || index.is_some() {
                return None;
            }
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
            if sign == -1 {
                return None;
            }
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
        let signed = if sign == -1 { raw.checked_neg()? } else { raw };
        disp = disp.checked_add(signed)?;
    }

    Some(MemoryAddr {
        base,
        index,
        scale,
        disp,
    })
}

fn strip_size_prefix(s: &str) -> &str {
    let s = s.trim();
    for prefix in [
        "qword ptr",
        "qword",
        "dword ptr",
        "dword",
        "word ptr",
        "word",
        "byte ptr",
        "byte",
    ] {
        if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
            let rest = s[prefix.len()..].trim_start();
            if rest.starts_with('[') {
                return rest;
            }
        }
    }
    s
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
        let value = u32::from_str_radix(hex, 16).ok()?;
        Some(value as i32)
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
    let line = strip_comment(line).trim();
    if line.is_empty() {
        return None;
    }

    let mut parts = line.split_whitespace();
    let mnemonic_raw = parts.next()?;
    let mnemonic = mnemonic_raw.to_lowercase();
    let rest = line[mnemonic_raw.len()..].trim();

    match mnemonic.as_str() {
        "mov" | "add" | "sub" | "and" | "or" | "xor" | "lea" | "cmp" | "shl" | "shr" => {
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
                    "lea" => Some(Instruction::Lea(dst, src)),
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
        "syscall" if rest.is_empty() => Some(Instruction::Syscall),
        "ret" if rest.is_empty() => Some(Instruction::Ret),
        _ => None,
    }
}

fn strip_comment(line: &str) -> &str {
    let mut in_brackets = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '[' => in_brackets = true,
            ']' => in_brackets = false,
            '#' if !in_brackets => return &line[..idx],
            _ => {}
        }
    }
    line
}

/// Returns `true` if `s` is a valid label identifier: starts with a letter or
/// underscore and contains only alphanumeric characters, underscores, or dots.
fn is_label_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '.' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// Parses a condition-code suffix (the part after the leading `j`), e.g.
/// `"z"` → `Some(ConditionCode::Z)`.
pub fn parse_condition_code(suffix: &str) -> Option<ConditionCode> {
    match suffix {
        "o" => Some(ConditionCode::O),
        "no" => Some(ConditionCode::No),
        "b" | "c" | "nae" => Some(ConditionCode::B),
        "nb" | "nc" | "ae" => Some(ConditionCode::Nb),
        "z" | "e" => Some(ConditionCode::Z),
        "nz" | "ne" => Some(ConditionCode::Nz),
        "be" | "na" => Some(ConditionCode::Be),
        "nbe" | "a" => Some(ConditionCode::Nbe),
        "s" => Some(ConditionCode::S),
        "ns" => Some(ConditionCode::Ns),
        "p" | "pe" => Some(ConditionCode::P),
        "np" | "po" => Some(ConditionCode::Np),
        "l" | "nge" => Some(ConditionCode::L),
        "nl" | "ge" => Some(ConditionCode::Nl),
        "le" | "ng" => Some(ConditionCode::Le),
        "nle" | "g" => Some(ConditionCode::Nle),
        _ => None,
    }
}

/// Parses a single source line into an [`AsmLine`].
///
/// Handles:
/// - Empty lines and comment-only lines → `None`
/// - Label definitions (`loop:`) → `Some(AsmLine::Label(...))`
/// - `jmp <label>` / `call <label>` with a bare identifier → label-ref variants
/// - `j<cc> <label>` conditional jumps
/// - All other instructions via [`parse_instruction`]
pub fn parse_asm_line(line: &str) -> Option<AsmLine> {
    let line = strip_comment(line).trim();
    if line.is_empty() {
        return None;
    }

    // ---- label definition: "loop_start:" ----
    if let Some(name) = line.strip_suffix(':') {
        let name = name.trim();
        if is_label_ident(name) {
            return Some(AsmLine::Label(String::from(name)));
        }
    }

    // ---- split into mnemonic + rest ----
    let mut parts = line.split_whitespace();
    let mnemonic_raw = parts.next()?;
    let mnemonic = mnemonic_raw.to_lowercase();
    let rest = line[mnemonic_raw.len()..].trim();

    // ---- jmp <label> ----
    if mnemonic == "jmp" && is_label_ident(rest) {
        return Some(AsmLine::Instr(Instruction::JmpLabel(String::from(rest))));
    }

    // ---- call <label> ----
    if mnemonic == "call" && is_label_ident(rest) {
        return Some(AsmLine::Instr(Instruction::CallLabel(String::from(rest))));
    }

    // ---- j<cc> <label> ----
    if let Some(suffix) = mnemonic.strip_prefix('j') {
        if let Some(cc) = parse_condition_code(suffix) {
            if is_label_ident(rest) {
                return Some(AsmLine::Instr(Instruction::Jcc(cc, String::from(rest))));
            }
        }
    }

    // ---- fall back to the original instruction parser ----
    parse_instruction(line).map(AsmLine::Instr)
}