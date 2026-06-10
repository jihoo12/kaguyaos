//! Tiny C JIT compiler.
//!
//! Public API: [`compile_and_run`].
//!
//! Internal layout:
//!   lexer   — source text → tokens
//!   parser  — tokens → AST / values
//!   codegen — values → x86-64 machine bytes

pub mod lexer;
pub mod parser;
pub mod codegen;

use alloc::string::String;
use crate::tinyasm::jit::JitMemory;

/// Compile all functions in `src`, locate `main`, JIT-compile it, and execute it.
///
/// # Supported grammar
/// One or more functions of the form:
/// ```c
/// uint64_t <name>(<params>) { <body> }
/// ```
/// where `<params>` is empty or a comma-separated list of `uint64_t <name>` pairs
/// (up to 6 parameters following the System V AMD64 ABI).
/// One function must be named `main`.
///
/// # Example
/// ```
/// let result = compile_and_run("uint64_t main() { return 42; }").unwrap();
/// assert_eq!(result, 42);
/// ```
///
/// Multiple functions are allowed; only `main` is executed:
/// ```
/// let src = "
///     uint64_t helper() { return 0; }
///     uint64_t main()   { return 7; }
/// ";
/// assert_eq!(compile_and_run(src).unwrap(), 7);
/// ```
///
/// Functions with arguments:
/// ```
/// let src = "
///     uint64_t add(uint64_t a, uint64_t b) { return a; }
///     uint64_t main() { return add(20, 22); }
/// ";
/// assert_eq!(compile_and_run(src).unwrap(), 20);
/// ```
pub fn compile_and_run(src: &str) -> Result<u64, String> {
    // 1. Lex
    let tokens = lexer::lex(src)?;

    // 2. Parse all functions
    let functions = parser::parse_functions(&tokens)?;

    // 3. Emit machine code
    let code = codegen::compile_program(&functions)?;

    // 4. Load into executable memory and call
    let mut mem = JitMemory::new(code.len())?;
    mem.write(&code)?;
    mem.make_executable()?;

    let result = unsafe { mem.as_fn_u64()() };
    Ok(result)
}