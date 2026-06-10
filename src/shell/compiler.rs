/// Compilation and assembly helpers, plus process execution.

use alloc::string::String;
use alloc::vec::Vec;
use crate::std::stdio::print;
use super::fs_utils::fs_read_file;

// ---------------------------------------------------------------------------
// Compilation / Assembly
// ---------------------------------------------------------------------------

/// Determine file type by extension and compile/assemble accordingly.
pub fn compile_by_extension(filename: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    if is_c_source(filename) {
        let src = core::str::from_utf8(data)
            .map_err(|_| String::from("File content is not valid UTF-8 for compilation."))?;
        compile_c(src)
    } else if is_asm_source(filename) {
        let src = core::str::from_utf8(data)
            .map_err(|_| String::from("File content is not valid UTF-8 for assembly."))?;
        assemble(src)
    } else {
        Err(String::from("Unsupported file extension. Only .c, .asm, and .s files are supported."))
    }
}

/// Check whether `filename` looks like a C source file.
pub fn is_c_source(filename: &str) -> bool {
    filename.ends_with(".c") || filename.ends_with(".C")
}

/// Check whether `filename` looks like an assembly source file.
pub fn is_asm_source(filename: &str) -> bool {
    filename.ends_with(".asm")
        || filename.ends_with(".ASM")
        || filename.ends_with(".s")
        || filename.ends_with(".S")
}

/// Returns true if the file is a source that needs compilation before running.
pub fn needs_compilation(filename: &str) -> bool {
    is_c_source(filename) || is_asm_source(filename)
}

fn compile_c(src: &str) -> Result<Vec<u8>, String> {
    let tokens = crate::cc::lexer::lex(src)?;
    let functions = crate::cc::parser::parse_functions(&tokens)?;
    crate::cc::codegen::compile_program(&functions)
}

fn assemble(asm_src: &str) -> Result<Vec<u8>, String> {
    use crate::tinyasm::encoder::assemble as encode;
    use crate::tinyasm::parser::parse_asm_line;

    let lines: Vec<_> = asm_src
        .split(|c| c == ';' || c == '\n')
        .filter_map(|part| parse_asm_line(part.trim()))
        .collect();

    if lines.is_empty() {
        return Err(String::from("No valid instructions found."));
    }

    encode(&lines).map_err(|e| alloc::format!("Encoding error: {}", e))
}

// ---------------------------------------------------------------------------
// Process execution
// ---------------------------------------------------------------------------

/// The trampoline that converts a process's return value in `rax` into the
/// first argument for `exit_process`.
#[unsafe(naked)]
extern "sysv64" fn user_exit_trampoline() -> ! {
    core::arch::naked_asm!(
        "mov rdi, rax",
        "jmp {exit_process_fn}",
        exit_process_fn = sym crate::std::exit_process
    );
}

/// Process status codes returned by `get_process_status`.
const PROCESS_TERMINATED: usize = 2;
const PROCESS_NOT_FOUND: usize = 3;

/// Spawn the given machine code as a child process, wait for it to finish,
/// and report its exit code.
pub fn run_as_process(code: &[u8]) -> Result<(), String> {
    use crate::tinyasm::jit::JitMemory;
    use crate::std::{spawn_process, yield_process, get_process_status, get_process_exit_code, sys_alloc, sys_free};

    let mut jit = JitMemory::new(code.len())?;
    jit.write(code)?;
    jit.make_executable()?;

    const STACK_SIZE: usize = 16384;
    let stack_bottom = unsafe { sys_alloc(STACK_SIZE, 16) };
    if stack_bottom.is_null() {
        return Err(String::from("Failed to allocate process stack"));
    }

    // Set up the stack so that when the process returns, it hits our trampoline.
    let initial_rsp = unsafe { stack_bottom.add(STACK_SIZE).sub(8) };
    unsafe {
        *(initial_rsp as *mut u64) = user_exit_trampoline as *const () as u64;
    }

    let entry_point = unsafe { jit.as_fn_u64() as *const () as u64 };
    let pid = spawn_process(entry_point as usize, initial_rsp as usize);
    print(&alloc::format!("Spawned process PID={}\n", pid));

    // Block until the process terminates.
    loop {
        yield_process();
        let status = get_process_status(pid);
        if status == PROCESS_TERMINATED || status == PROCESS_NOT_FOUND {
            break;
        }
    }

    let exit_code = get_process_exit_code(pid);
    print(&alloc::format!("Process finished with exit code: {}\n", exit_code));

    unsafe { sys_free(stack_bottom) };
    Ok(())
}

/// Read a file, compile it if it's a source file, and run the resulting
/// machine code as a process.
pub fn load_and_run(filename: &str) -> Result<(), String> {
    let data = fs_read_file(filename)?;

    let code = if needs_compilation(filename) {
        compile_by_extension(filename, &data)?
    } else {
        data
    };

    run_as_process(&code)
}
