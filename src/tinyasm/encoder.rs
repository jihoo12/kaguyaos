use super::registers::Register;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    UnsupportedOperand(String),
    InvalidScale(u8),
    InvalidDisplacement(String),
    Other(String),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::UnsupportedOperand(msg) => write!(f, "Unsupported operand: {}", msg),
            EncodeError::InvalidScale(scale) => write!(f, "Invalid scale: {}", scale),
            EncodeError::InvalidDisplacement(msg) => write!(f, "Invalid displacement: {}", msg),
            EncodeError::Other(msg) => write!(f, "Encoding error: {}", msg),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryAddr {
    pub base: Option<Register>,
    pub index: Option<Register>,
    pub scale: u8, // 1, 2, 4, 8
    pub disp: i32,
}

impl fmt::Display for MemoryAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        let mut parts = Vec::new();
        if let Some(base) = self.base {
            parts.push(format!("{}", base));
        }
        if let Some(index) = self.index {
            parts.push(format!("{}*{}", index, self.scale));
        }
        if self.disp != 0 || parts.is_empty() {
            if self.disp > 0 && !parts.is_empty() {
                parts.push(format!("+{}", self.disp));
            } else {
                parts.push(format!("{}", self.disp));
            }
        }
        write!(f, "{}]", parts.join(" + "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    Reg(Register),
    Imm64(u64),
    Imm32(i32),
    Mem(MemoryAddr),
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Reg(r) => write!(f, "{}", r),
            Operand::Imm64(val) => write!(f, "0x{:X}", val),
            Operand::Imm32(val) => write!(f, "0x{:X}", val),
            Operand::Mem(mem) => write!(f, "qword {}", mem),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    Mov(Operand, Operand), // Destination, Source
    Add(Operand, Operand),
    Sub(Operand, Operand),
    And(Operand, Operand),
    Or(Operand, Operand),
    Xor(Operand, Operand),
    Not(Operand),
    Shl(Operand, Operand),
    Shr(Operand, Operand),
    Mul(Operand), // Operand is r/m64
    Div(Operand), // Operand is r/m64
    Cmp(Operand, Operand),
    Call(Operand),
    Jmp(Operand),
    Syscall,
    Ret,
    Push(Operand),
    Pop(Operand),
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instruction::Mov(dst, src) => write!(f, "mov {}, {}", dst, src),
            Instruction::Add(dst, src) => write!(f, "add {}, {}", dst, src),
            Instruction::Sub(dst, src) => write!(f, "sub {}, {}", dst, src),
            Instruction::And(dst, src) => write!(f, "and {}, {}", dst, src),
            Instruction::Or(dst, src) => write!(f, "or {}, {}", dst, src),
            Instruction::Xor(dst, src) => write!(f, "xor {}, {}", dst, src),
            Instruction::Not(op) => write!(f, "not {}", op),
            Instruction::Shl(dst, count) => write!(f, "shl {}, {}", dst, count),
            Instruction::Shr(dst, count) => write!(f, "shr {}, {}", dst, count),
            Instruction::Mul(op) => write!(f, "mul {}", op),
            Instruction::Div(op) => write!(f, "div {}", op),
            Instruction::Cmp(dst, src) => write!(f, "cmp {}, {}", dst, src),
            Instruction::Call(op) => write!(f, "call {}", op),
            Instruction::Jmp(op) => write!(f, "jmp {}", op),
            Instruction::Syscall => write!(f, "syscall"),
            Instruction::Ret => write!(f, "ret"),
            Instruction::Push(op) => write!(f, "push {}", op),
            Instruction::Pop(op) => write!(f, "pop {}", op),
        }
    }
}

pub fn encode_instruction(instr: Instruction, bytes: &mut Vec<u8>) -> Result<(), EncodeError> {
    match instr {
        Instruction::Mov(dst, src) => encode_mov(dst, src, bytes)?,
        Instruction::Add(dst, src) => encode_arithmetic(0x01, 0x03, 0, dst, src, bytes)?,
        Instruction::Sub(dst, src) => encode_arithmetic(0x29, 0x2B, 5, dst, src, bytes)?,
        Instruction::And(dst, src) => encode_arithmetic(0x21, 0x23, 4, dst, src, bytes)?,
        Instruction::Or(dst, src) => encode_arithmetic(0x09, 0x0B, 1, dst, src, bytes)?,
        Instruction::Xor(dst, src) => encode_arithmetic(0x31, 0x33, 6, dst, src, bytes)?,
        Instruction::Shl(dst, count) => encode_shift(4, dst, count, bytes)?,
        Instruction::Shr(dst, count) => encode_shift(5, dst, count, bytes)?,
        Instruction::Not(op) => encode_unary(0xF7, 2, op, bytes)?,
        Instruction::Mul(op) => encode_unary(0xF7, 4, op, bytes)?,
        Instruction::Div(op) => encode_unary(0xF7, 6, op, bytes)?,
        Instruction::Cmp(dst, src) => encode_arithmetic(0x39, 0x3B, 7, dst, src, bytes)?,
        Instruction::Call(op) => encode_call(op, bytes)?,
        Instruction::Jmp(op) => encode_jmp(op, bytes)?,
        Instruction::Syscall => bytes.extend_from_slice(&[0x0F, 0x05]),
        Instruction::Ret => bytes.push(0xC3),
        Instruction::Push(op) => encode_push(op, bytes)?,
        Instruction::Pop(op) => encode_pop(op, bytes)?,
    }
    Ok(())
}

fn encode_rex(
    w: bool,
    r: Option<Register>,
    x: Option<Register>,
    b: Option<Register>,
    bytes: &mut Vec<u8>,
) {
    let mut rex = 0x40;
    if w {
        rex |= 0x08;
    }
    if let Some(reg) = r {
        if reg.is_extended() {
            rex |= 0x04;
        }
    }
    if let Some(reg) = x {
        if reg.is_extended() {
            rex |= 0x02;
        }
    }
    if let Some(reg) = b {
        if reg.is_extended() {
            rex |= 0x01;
        }
    }

    if rex != 0x40 {
        bytes.push(rex);
    }
}

fn encode_call(op: Operand, bytes: &mut Vec<u8>) -> Result<(), EncodeError> {
    match op {
        Operand::Reg(reg) => {
            // CALL r/m64 -> FF /2
            if reg.is_extended() {
                encode_rex(false, None, None, Some(reg), bytes);
            }
            bytes.push(0xFF);
            bytes.push(0xC0 | (2 << 3) | reg.code());
        }
        Operand::Mem(mem) => {
            // CALL r/m64 -> FF /2
            let (modrm, sib, disp_size) = encode_mem_parts(2, false, mem, bytes)?;
            bytes.push(0xFF);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }
        Operand::Imm32(imm) => {
            // CALL rel32 -> E8 cd
            bytes.push(0xE8);
            bytes.extend_from_slice(&imm.to_le_bytes());
        }
        _ => {
            return Err(EncodeError::UnsupportedOperand(format!(
                "CALL operand {}",
                op
            )));
        }
    }
    Ok(())
}

fn encode_jmp(op: Operand, bytes: &mut Vec<u8>) -> Result<(), EncodeError> {
    match op {
        Operand::Reg(reg) => {
            // JMP r/m64 -> FF /4
            if reg.is_extended() {
                encode_rex(false, None, None, Some(reg), bytes);
            }
            bytes.push(0xFF);
            bytes.push(0xC0 | (4 << 3) | reg.code());
        }
        Operand::Mem(mem) => {
            // JMP r/m64 -> FF /4
            let (modrm, sib, disp_size) = encode_mem_parts(4, false, mem, bytes)?;
            bytes.push(0xFF);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }
        Operand::Imm32(imm) => {
            // JMP rel32 -> E9 cd
            bytes.push(0xE9);
            bytes.extend_from_slice(&imm.to_le_bytes());
        }
        _ => {
            return Err(EncodeError::UnsupportedOperand(format!(
                "JMP operand {}",
                op
            )));
        }
    }
    Ok(())
}

fn encode_push(op: Operand, bytes: &mut Vec<u8>) -> Result<(), EncodeError> {
    match op {
        Operand::Reg(reg) => {
            // PUSH r64 -> 50 + rd
            if reg.is_extended() {
                encode_rex(false, None, None, Some(reg), bytes);
            }
            bytes.push(0x50 + reg.code());
        }
        Operand::Imm32(imm) => {
            // PUSH imm32 -> 68 id
            bytes.push(0x68);
            bytes.extend_from_slice(&imm.to_le_bytes());
        }
        Operand::Mem(mem) => {
            // PUSH r/m64 -> FF /6
            let (modrm, sib, disp_size) = encode_mem_parts(6, false, mem, bytes)?;
            bytes.push(0xFF);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }
        _ => return Err(EncodeError::UnsupportedOperand(format!("PUSH {}", op))),
    }
    Ok(())
}

fn encode_pop(op: Operand, bytes: &mut Vec<u8>) -> Result<(), EncodeError> {
    match op {
        Operand::Reg(reg) => {
            // POP r64 -> 58 + rd
            if reg.is_extended() {
                encode_rex(false, None, None, Some(reg), bytes);
            }
            bytes.push(0x58 + reg.code());
        }
        Operand::Mem(mem) => {
            // POP r/m64 -> 8F /0
            let (modrm, sib, disp_size) = encode_mem_parts(0, false, mem, bytes)?;
            bytes.push(0x8F);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }
        _ => return Err(EncodeError::UnsupportedOperand(format!("POP {}", op))),
    }
    Ok(())
}

fn encode_shift(
    ext_idx: u8,
    dst: Operand,
    count: Operand,
    bytes: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    match count {
        Operand::Reg(Register::RCX) => {
            // SHL r/m64, CL -> D3 /ext
            match dst {
                Operand::Reg(reg) => {
                    encode_rex(true, None, None, Some(reg), bytes);
                    bytes.push(0xD3);
                    bytes.push(0xC0 | (ext_idx << 3) | reg.code());
                }
                Operand::Mem(mem) => {
                    let (modrm, sib, disp_size) = encode_mem_parts(ext_idx, false, mem, bytes)?;
                    bytes.push(0xD3);
                    bytes.push(modrm);
                    if let Some(s) = sib {
                        bytes.push(s);
                    }
                    push_displacement(mem.disp, disp_size, bytes);
                }
                _ => {
                    return Err(EncodeError::UnsupportedOperand(format!(
                        "shift destination {}",
                        dst
                    )));
                }
            }
        }
        Operand::Imm32(imm) => {
            if imm == 1 {
                // SHL r/m64, 1 -> D1 /ext
                match dst {
                    Operand::Reg(reg) => {
                        encode_rex(true, None, None, Some(reg), bytes);
                        bytes.push(0xD1);
                        bytes.push(0xC0 | (ext_idx << 3) | reg.code());
                    }
                    Operand::Mem(mem) => {
                        let (modrm, sib, disp_size) = encode_mem_parts(ext_idx, false, mem, bytes)?;
                        bytes.push(0xD1);
                        bytes.push(modrm);
                        if let Some(s) = sib {
                            bytes.push(s);
                        }
                        push_displacement(mem.disp, disp_size, bytes);
                    }
                    _ => {
                        return Err(EncodeError::UnsupportedOperand(format!(
                            "shift destination {}",
                            dst
                        )));
                    }
                }
            } else {
                // SHL r/m64, imm8 -> C1 /ext ib
                match dst {
                    Operand::Reg(reg) => {
                        encode_rex(true, None, None, Some(reg), bytes);
                        bytes.push(0xC1);
                        bytes.push(0xC0 | (ext_idx << 3) | reg.code());
                        bytes.push(imm as u8);
                    }
                    Operand::Mem(mem) => {
                        let (modrm, sib, disp_size) = encode_mem_parts(ext_idx, false, mem, bytes)?;
                        bytes.push(0xC1);
                        bytes.push(modrm);
                        if let Some(s) = sib {
                            bytes.push(s);
                        }
                        push_displacement(mem.disp, disp_size, bytes);
                        bytes.push(imm as u8);
                    }
                    _ => {
                        return Err(EncodeError::UnsupportedOperand(format!(
                            "shift destination {}",
                            dst
                        )));
                    }
                }
            }
        }
        _ => {
            return Err(EncodeError::UnsupportedOperand(format!(
                "shift count {}",
                count
            )));
        }
    }
    Ok(())
}

fn encode_unary(
    opcode: u8,
    ext_idx: u8,
    op: Operand,
    bytes: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    match op {
        Operand::Reg(reg) => {
            encode_rex(true, None, None, Some(reg), bytes);
            bytes.push(opcode);
            let modrm = 0xC0 | (ext_idx << 3) | reg.code();
            bytes.push(modrm);
        }
        Operand::Mem(mem) => {
            let (modrm, sib, disp_size) = encode_mem_parts(ext_idx, false, mem, bytes)?;
            bytes.push(opcode);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }
        _ => {
            return Err(EncodeError::UnsupportedOperand(format!(
                "unary operand {}",
                op
            )));
        }
    }
    Ok(())
}

fn encode_mov(dst: Operand, src: Operand, bytes: &mut Vec<u8>) -> Result<(), EncodeError> {
    match (dst, src) {
        // MOV r64, imm64
        (Operand::Reg(dst_reg), Operand::Imm64(imm)) => {
            encode_rex(true, None, None, Some(dst_reg), bytes);
            bytes.push(0xB8 + dst_reg.code());
            bytes.extend_from_slice(&imm.to_le_bytes());
        }

        // MOV r64, r64
        (Operand::Reg(dst_reg), Operand::Reg(src_reg)) => {
            encode_rex(true, Some(src_reg), None, Some(dst_reg), bytes);
            bytes.push(0x89);
            let modrm = 0xC0 | (src_reg.code() << 3) | dst_reg.code();
            bytes.push(modrm);
        }

        // MOV r64, m64 (Load)
        (Operand::Reg(dst_reg), Operand::Mem(mem)) => {
            let (modrm, sib, disp_size) =
                encode_mem_parts(dst_reg.code(), dst_reg.is_extended(), mem, bytes)?;
            bytes.push(0x8B); // Opcode for MOV r64, r/m64
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }

        // MOV m64, r64 (Store)
        (Operand::Mem(mem), Operand::Reg(src_reg)) => {
            let (modrm, sib, disp_size) =
                encode_mem_parts(src_reg.code(), src_reg.is_extended(), mem, bytes)?;
            bytes.push(0x89); // Opcode for MOV r/m64, r64
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }
        // MOV r/m64, imm32
        (Operand::Reg(dst_reg), Operand::Imm32(imm)) => {
            encode_rex(true, None, None, Some(dst_reg), bytes);
            bytes.push(0xC7);
            let modrm = 0xC0 | (0 << 3) | dst_reg.code();
            bytes.push(modrm);
            bytes.extend_from_slice(&imm.to_le_bytes());
        }

        (Operand::Mem(mem), Operand::Imm32(imm)) => {
            let (modrm, sib, disp_size) = encode_mem_parts(0, false, mem, bytes)?;
            bytes.push(0xC7);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
            bytes.extend_from_slice(&imm.to_le_bytes());
        }

        _ => {
            return Err(EncodeError::UnsupportedOperand(format!(
                "MOV {} -> {}",
                src, dst
            )));
        }
    }
    Ok(())
}

fn encode_arithmetic(
    op_mr: u8,   // r/m64, r64 (e.g., 0x01 for ADD)
    op_rm: u8,   // r64, r/m64 (e.g., 0x03 for ADD)
    ext_idx: u8, // extension for imm (e.g., 0 for ADD, 5 for SUB)
    dst: Operand,
    src: Operand,
    bytes: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    match (dst, src) {
        // OP r64, r64
        (Operand::Reg(dst_reg), Operand::Reg(src_reg)) => {
            encode_rex(true, Some(src_reg), None, Some(dst_reg), bytes);
            bytes.push(op_mr);
            let modrm = 0xC0 | (src_reg.code() << 3) | dst_reg.code();
            bytes.push(modrm);
        }

        // OP r64, m64
        (Operand::Reg(dst_reg), Operand::Mem(mem)) => {
            let (modrm, sib, disp_size) =
                encode_mem_parts(dst_reg.code(), dst_reg.is_extended(), mem, bytes)?;
            bytes.push(op_rm);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }

        // OP m64, r64
        (Operand::Mem(mem), Operand::Reg(src_reg)) => {
            let (modrm, sib, disp_size) =
                encode_mem_parts(src_reg.code(), src_reg.is_extended(), mem, bytes)?;
            bytes.push(op_mr);
            bytes.push(modrm);
            if let Some(s) = sib {
                bytes.push(s);
            }
            push_displacement(mem.disp, disp_size, bytes);
        }

        // OP r/m64, imm32
        (dst, Operand::Imm32(imm)) => {
            let (dst_reg_code, is_ext, mem_info) = match dst {
                Operand::Reg(r) => (r.code(), r.is_extended(), None),
                Operand::Mem(m) => (0, false, Some(m)), // Default is_ext false, will be set in encode_mem_parts if mem.base is ext
                _ => {
                    return Err(EncodeError::UnsupportedOperand(format!(
                        "arithmetic dest {}",
                        dst
                    )));
                }
            };

            let (opcode, is_8bit) = if imm >= -128 && imm <= 127 {
                (0x83, true)
            } else {
                (0x81, false)
            };

            if let Some(mem) = mem_info {
                let (modrm, sib, disp_size) = encode_mem_parts(ext_idx, false, mem, bytes)?;
                bytes.push(opcode);
                bytes.push(modrm);
                if let Some(s) = sib {
                    bytes.push(s);
                }
                push_displacement(mem.disp, disp_size, bytes);
            } else {
                let mut rex = 0x48;
                if is_ext {
                    rex |= 0x01;
                }
                bytes.push(rex);
                bytes.push(opcode);
                let modrm = 0xC0 | (ext_idx << 3) | dst_reg_code;
                bytes.push(modrm);
            }

            if is_8bit {
                bytes.push(imm as u8);
            } else {
                bytes.extend_from_slice(&imm.to_le_bytes());
            }
        }
        _ => {
            return Err(EncodeError::UnsupportedOperand(format!(
                "arithmetic {} {}",
                dst, src
            )));
        }
    }
    Ok(())
}

fn push_displacement(disp: i32, size: usize, bytes: &mut Vec<u8>) {
    if size == 1 {
        bytes.push(disp as u8);
    } else if size == 4 {
        bytes.extend_from_slice(&disp.to_le_bytes());
    }
}

fn encode_mem_parts(
    reg_val: u8,
    reg_ext: bool,
    mem: MemoryAddr,
    bytes: &mut Vec<u8>,
) -> Result<(u8, Option<u8>, usize), EncodeError> {
    let (rex, modrm, sib, disp_size) = mem_parts(reg_val, reg_ext, mem)?;
    bytes.push(rex);
    Ok((modrm, sib, disp_size))
}

fn mem_parts(
    reg_val: u8,
    reg_ext: bool,
    mem: MemoryAddr,
) -> Result<(u8, u8, Option<u8>, usize), EncodeError> {
    if !(reg_val < 8) {
        return Err(EncodeError::Other(format!("Invalid ModR/M reg field {}", reg_val)));
    }

    let scale_bits = match mem.scale {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => return Err(EncodeError::InvalidScale(mem.scale)),
    };

    if let Some(index) = mem.index {
        if index.code() == Register::RSP.code() {
            return Err(EncodeError::UnsupportedOperand(format!(
                "{} cannot be used as a SIB index",
                index
            )));
        }
    }

    let mut rex = 0x48;
    if reg_ext {
        rex |= 0x04;
    }
    if let Some(b) = mem.base {
        if b.is_extended() {
            rex |= 0x01;
        }
    }
    if let Some(i) = mem.index {
        if i.is_extended() {
            rex |= 0x02;
        }
    }

    let (mod_bits, disp_size) = match mem.base {
        None => (0x00, 4),
        Some(Register::RBP) | Some(Register::R13) if mem.disp == 0 => (0x01, 1),
        Some(_) if mem.disp == 0 => (0x00, 0),
        Some(_) if mem.disp >= -128 && mem.disp <= 127 => (0x01, 1),
        Some(_) => (0x10, 4),
    };

    let use_sib = mem.base.is_none()
        || mem.index.is_some()
        || mem.base == Some(Register::RSP)
        || mem.base == Some(Register::R12);
    let rm_bits = if use_sib {
        0x04
    } else {
        mem.base
            .ok_or_else(|| EncodeError::Other(String::from("Missing memory base")))?
            .code()
    };
    let modrm = (mod_bits << 6) | (reg_val << 3) | rm_bits;

    if use_sib {
        let index_bits = mem.index.map(|r| r.code()).unwrap_or(0x04);
        let base_bits = mem.base.map(|r| r.code()).unwrap_or(0x05);
        let sib = (scale_bits << 6) | (index_bits << 3) | base_bits;
        Ok((rex, modrm, Some(sib), disp_size))
    } else {
        Ok((rex, modrm, None, disp_size))
    }
}

#[cfg(test)]
mod tests {
    use super::super::registers::Register;
    use super::*;

    #[test]
    fn test_encode_cmp() {
        let mut bytes = Vec::new();
        // CMP RAX, RBX -> 48 39 D8
        encode_instruction(
            Instruction::Cmp(Operand::Reg(Register::RAX), Operand::Reg(Register::RBX)),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(bytes, vec![0x48, 0x39, 0xD8]);

        bytes.clear();
        // CMP RAX, 0x12 -> 48 83 F8 12
        encode_instruction(
            Instruction::Cmp(Operand::Reg(Register::RAX), Operand::Imm32(0x12)),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(bytes, vec![0x48, 0x83, 0xF8, 0x12]);
    }

    #[test]
    fn test_encode_call() {
        let mut bytes = Vec::new();
        // CALL RAX -> FF D0
        encode_instruction(Instruction::Call(Operand::Reg(Register::RAX)), &mut bytes).unwrap();
        assert_eq!(bytes, vec![0xFF, 0xD0]);

        bytes.clear();
        // CALL 0x1234 (relative) -> E8 34 12 00 00
        encode_instruction(Instruction::Call(Operand::Imm32(0x1234)), &mut bytes).unwrap();
        assert_eq!(bytes, vec![0xE8, 0x34, 0x12, 0x00, 0x00]);
    }

    #[test]
    fn test_encode_jmp() {
        let mut bytes = Vec::new();
        // JMP RAX -> FF E0
        encode_instruction(Instruction::Jmp(Operand::Reg(Register::RAX)), &mut bytes).unwrap();
        assert_eq!(bytes, vec![0xFF, 0xE0]);

        bytes.clear();
        // JMP 0x1234 (relative) -> E9 34 12 00 00
        encode_instruction(Instruction::Jmp(Operand::Imm32(0x1234)), &mut bytes).unwrap();
        assert_eq!(bytes, vec![0xE9, 0x34, 0x12, 0x00, 0x00]);
    }

    #[test]
    fn test_encode_memory_bases_that_require_sib_or_displacement() {
        let mut bytes = Vec::new();

        encode_instruction(
            Instruction::Mov(
                Operand::Reg(Register::RAX),
                Operand::Mem(MemoryAddr {
                    base: Some(Register::RSP),
                    index: None,
                    scale: 1,
                    disp: 0,
                }),
            ),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(bytes, vec![0x48, 0x8B, 0x04, 0x24]);

        bytes.clear();
        encode_instruction(
            Instruction::Mov(
                Operand::Reg(Register::RAX),
                Operand::Mem(MemoryAddr {
                    base: Some(Register::RBP),
                    index: None,
                    scale: 1,
                    disp: 0,
                }),
            ),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(bytes, vec![0x48, 0x8B, 0x45, 0x00]);
    }

    #[test]
    fn test_encode_absolute_memory_uses_sib_disp32() {
        let mut bytes = Vec::new();

        encode_instruction(
            Instruction::Mov(
                Operand::Reg(Register::RAX),
                Operand::Mem(MemoryAddr {
                    base: None,
                    index: None,
                    scale: 1,
                    disp: 0x1234,
                }),
            ),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(bytes, vec![0x48, 0x8B, 0x04, 0x25, 0x34, 0x12, 0x00, 0x00]);
    }

    #[test]
    fn test_invalid_memory_scale_does_not_write_partial_prefix() {
        let mut bytes = vec![0xAA];

        let err = encode_instruction(
            Instruction::Mov(
                Operand::Reg(Register::RAX),
                Operand::Mem(MemoryAddr {
                    base: Some(Register::RAX),
                    index: Some(Register::RBX),
                    scale: 3,
                    disp: 0,
                }),
            ),
            &mut bytes,
        )
        .unwrap_err();

        assert_eq!(err, EncodeError::InvalidScale(3));
        assert_eq!(bytes, vec![0xAA]);
    }
}
