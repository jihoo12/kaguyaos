/// Parses the token stream produced by the lexer.
///
/// Understands one or more functions of the form:
///
///   uint64_t <name>() { return <integer>; }
///
/// Extend this file to support richer statements, expressions, and types.

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use super::lexer::Token;

/// A parsed function: its name and the literal value it returns.
pub struct Function {
    pub name: String,
    pub return_value: u64,
}

/// Parse all top-level functions from the token stream.
/// Returns a map of function name → return value.
pub fn parse_functions(tokens: &[Token]) -> Result<BTreeMap<String, u64>, String> {
    let mut map = BTreeMap::new();
    let mut i = 0;

    while i < tokens.len() {
        let func = parse_function(tokens, &mut i)?;
        if map.contains_key(&func.name) {
            return Err(format!("Duplicate function: '{}'", func.name));
        }
        map.insert(func.name, func.return_value);
    }

    Ok(map)
}

/// Parse a single function and advance `i` past it.
fn parse_function(tokens: &[Token], i: &mut usize) -> Result<Function, String> {
    // Return-type identifier (we accept any ident, e.g. `uint64_t`)
    match tokens.get(*i) {
        Some(Token::Ident(_)) => *i += 1,
        _ => return Err("Expected return-type identifier".to_string()),
    }

    // Function name
    let name = match tokens.get(*i) {
        Some(Token::Ident(n)) => { let s = n.clone(); *i += 1; s }
        _ => return Err("Expected function name".to_string()),
    };

    // ()
    expect(tokens, i, &Token::LParen)?;
    expect(tokens, i, &Token::RParen)?;

    // { return <n>; }
    expect(tokens, i, &Token::LBrace)?;
    expect(tokens, i, &Token::Return)?;

    let return_value = match tokens.get(*i) {
        Some(Token::Number(n)) => { let v = *n; *i += 1; v }
        _ => return Err("Expected integer literal after 'return'".to_string()),
    };

    expect(tokens, i, &Token::Semicolon)?;
    expect(tokens, i, &Token::RBrace)?;

    Ok(Function { name, return_value })
}

/// Legacy single-function entry point (kept for compatibility).
pub fn parse_return_value(tokens: &[Token]) -> Result<u64, String> {
    let map = parse_functions(tokens)?;
    map.into_values().next().ok_or_else(|| "No functions found".to_string())
}

fn expect(tokens: &[Token], i: &mut usize, expected: &Token) -> Result<(), String> {
    match tokens.get(*i) {
        Some(t) if t == expected => { *i += 1; Ok(()) }
        other => Err(format!("Expected {:?}, got {:?}", expected, other)),
    }
}