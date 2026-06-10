use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::format;

use super::parser::{Function, Stmt, Expr};
use crate::tinyasm::parser::parse_asm_line;
use crate::tinyasm::encoder::assemble;

/// System V AMD64 ABI: first 6 integer argument registers.
/// Each entry is the REX.B‑extended register number used in ModR/M encoding.
const ARG_REGS: &[(u8, u8)] = &[
    // (REX prefix, reg-in-modrm)  for "mov [rbp+disp32], reg"
    // rdi=7, rsi=6, rdx=2, rcx=1, r8=0(+REX.R), r9=1(+REX.R)
    (0x48, 0xBD),  // rdi  -> mov [rbp+disp32], rdi  (opcode bytes: REX.W 89 /r)
    (0x48, 0xB5),  // rsi
    (0x48, 0x95),  // rdx
    (0x48, 0x8D),  // rcx
    (0x4C, 0x85),  // r8
    (0x4C, 0x8D),  // r9
];

struct Relocation {
    target: String,
    patch_offset: usize,
}

pub fn compile_program(functions: &BTreeMap<String, Function>) -> Result<Vec<u8>, String> {
    let mut code = Vec::new();
    let mut relocs = Vec::new();
    let mut func_offsets = BTreeMap::new();

    // 1. Establish compilation order. 'main' must be compiled first so that it sits at offset 0.
    let mut compile_order = Vec::new();
    if let Some(main_func) = functions.get("main") {
        compile_order.push(main_func);
    } else {
        return Err("No 'main' function found".to_string());
    }

    for (name, func) in functions {
        if name != "main" {
            compile_order.push(func);
        }
    }

    // 2. Compile each function
    for func in compile_order {
        func_offsets.insert(func.name.clone(), code.len());

        // Prologue
        code.push(0x55); // push rbp
        code.push(0x48); // REX.W
        code.push(0x89);
        code.push(0xE5); // mov rbp, rsp

        // Pre-pass to count unique variable declarations and assign offsets from RBP.
        // Parameters are allocated first so they can be referenced like local variables.
        let mut var_offsets = BTreeMap::new();
        let mut next_offset = 8;

        // Allocate stack slots for parameters
        for param in &func.params {
            if !var_offsets.contains_key(&param.name) {
                var_offsets.insert(param.name.clone(), next_offset);
                next_offset += 8;
            }
        }

        for stmt in &func.body {
            if let Stmt::VarDecl { name, .. } = stmt {
                if !var_offsets.contains_key(name) {
                    var_offsets.insert(name.clone(), next_offset);
                    next_offset += 8;
                }
            }
        }

        // Sub RSP to allocate stack space (aligned to 16 bytes)
        let stack_space = (next_offset - 8 + 15) & !15;
        if stack_space > 0 {
            // sub rsp, stack_space
            code.push(0x48); // REX.W
            code.push(0x81); // SUB
            code.push(0xEC); // ModR/M or opcode extension for RSP
            code.extend_from_slice(&(stack_space as u32).to_le_bytes());
        }

        // Move parameter registers into their stack slots.
        for (idx, param) in func.params.iter().enumerate() {
            if idx >= ARG_REGS.len() {
                return Err(format!("Too many parameters (max {})", ARG_REGS.len()));
            }
            let offset = *var_offsets.get(&param.name).unwrap();
            let disp = -(offset as i32);
            let (rex, modrm) = ARG_REGS[idx];
            // mov [rbp + disp32], <reg>
            code.push(rex);
            code.push(0x89);
            code.push(modrm);
            code.extend_from_slice(&disp.to_le_bytes());
        }

        // Compile statements
        for stmt in &func.body {
            match stmt {
                Stmt::VarDecl { name, val } => {
                    compile_expr(val, &mut code, &var_offsets, &mut relocs)?;
                    let offset = *var_offsets.get(name).unwrap();
                    let disp = -(offset as i32);
                    // mov [rbp - offset], rax
                    code.push(0x48);
                    code.push(0x89);
                    code.push(0x85);
                    code.extend_from_slice(&disp.to_le_bytes());
                }
                Stmt::Assign { name, val } => {
                    compile_expr(val, &mut code, &var_offsets, &mut relocs)?;
                    let offset = *var_offsets.get(name).ok_or_else(|| format!("Undeclared variable: {}", name))?;
                    let disp = -(offset as i32);
                    // mov [rbp - offset], rax
                    code.push(0x48);
                    code.push(0x89);
                    code.push(0x85);
                    code.extend_from_slice(&disp.to_le_bytes());
                }
                Stmt::Asm(asm_str) => {
                    let lines: Vec<_> = asm_str
                        .split(';')
                        .flat_map(|s| s.split('\n'))
                        .filter_map(|part| parse_asm_line(part.trim()))
                        .collect();
                    if !lines.is_empty() {
                        let asm_bytes = assemble(&lines).map_err(|e| format!("Asm error: {}", e))?;
                        code.extend_from_slice(&asm_bytes);
                    }
                }
                Stmt::Return(expr) => {
                    compile_expr(expr, &mut code, &var_offsets, &mut relocs)?;
                    code.push(0xC9); // leave
                    code.push(0xC3); // ret
                }
            }
        }

        // Default epilogue (just in case function has no return statement at the end)
        code.push(0xC9); // leave
        code.push(0xC3); // ret
    }

    // 3. Resolve relocations for function calls
    for rel in &relocs {
        let target_offset = *func_offsets.get(&rel.target).ok_or_else(|| format!("Undefined function: {}", rel.target))?;
        let next_instr = rel.patch_offset + 4;
        let rel_offset = (target_offset as isize) - (next_instr as isize);
        let rel_offset_i32 = rel_offset as i32;
        let bytes = rel_offset_i32.to_le_bytes();
        code[rel.patch_offset..rel.patch_offset + 4].copy_from_slice(&bytes);
    }

    Ok(code)
}

fn compile_expr(
    expr: &Expr,
    code: &mut Vec<u8>,
    var_offsets: &BTreeMap<String, usize>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), String> {
    match expr {
        Expr::Number(n) => {
            // mov rax, n
            code.push(0x48);
            code.push(0xB8);
            code.extend_from_slice(&n.to_le_bytes());
        }
        Expr::Variable(name) => {
            let offset = var_offsets.get(name).ok_or_else(|| format!("Undeclared variable: {}", name))?;
            let disp = -(*offset as i32);
            // mov rax, [rbp - offset]
            code.push(0x48);
            code.push(0x8B);
            code.push(0x85);
            code.extend_from_slice(&disp.to_le_bytes());
        }
        Expr::Call(func_name, args) => {
            if args.len() > ARG_REGS.len() {
                return Err(format!("Too many arguments (max {})", ARG_REGS.len()));
            }

            // Evaluate each argument onto the stack first to avoid clobbering registers.
            // Push all evaluated args (in rax) onto the machine stack.
            for arg in args.iter() {
                compile_expr(arg, code, var_offsets, relocs)?;
                // push rax
                code.push(0x50);
            }

            // Pop them into the correct argument registers in reverse order.
            // Registers: rdi, rsi, rdx, rcx, r8, r9
            for idx in (0..args.len()).rev() {
                match idx {
                    0 => { code.push(0x5F); }                    // pop rdi
                    1 => { code.push(0x5E); }                    // pop rsi
                    2 => { code.push(0x5A); }                    // pop rdx
                    3 => { code.push(0x59); }                    // pop rcx
                    4 => { code.push(0x41); code.push(0x58); }   // pop r8
                    5 => { code.push(0x41); code.push(0x59); }   // pop r9
                    _ => unreachable!(),
                }
            }

            // call func_name
            code.push(0xE8);
            let patch_offset = code.len();
            code.extend_from_slice(&[0u8; 4]);
            relocs.push(Relocation {
                target: func_name.clone(),
                patch_offset,
            });
        }
    }
    Ok(())
}

/// Legacy single return value helper (kept for compatibility).
pub fn emit_return_u64(value: u64) -> Vec<u8> {
    let mut code = Vec::with_capacity(11);
    code.push(0x48);        // REX.W
    code.push(0xB8);        // MOV RAX, imm64
    code.extend_from_slice(&value.to_le_bytes());
    code.push(0xC3);        // RET
    code
}