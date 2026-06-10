use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use super::lexer::Token;

#[derive(Debug, Clone)]
pub enum Expr {
    Number(u64),
    Variable(String),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl { name: String, val: Expr },
    Assign { name: String, val: Expr },
    Asm(String),
    Return(Expr),
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
}

pub fn parse_functions(tokens: &[Token]) -> Result<BTreeMap<String, Function>, String> {
    let mut map = BTreeMap::new();
    let mut i = 0;

    while i < tokens.len() {
        let func = parse_function(tokens, &mut i)?;
        if map.contains_key(&func.name) {
            return Err(format!("Duplicate function: '{}'", func.name));
        }
        map.insert(func.name.clone(), func);
    }

    Ok(map)
}

fn parse_expr(tokens: &[Token], i: &mut usize) -> Result<Expr, String> {
    match tokens.get(*i) {
        Some(Token::Number(n)) => {
            *i += 1;
            Ok(Expr::Number(*n))
        }
        Some(Token::Ident(name)) => {
            let name = name.clone();
            *i += 1;
            // Check if it's a function call (followed by LParen)
            if let Some(Token::LParen) = tokens.get(*i) {
                *i += 1; // Consume LParen
                let mut args = Vec::new();
                // Parse argument list (may be empty)
                if tokens.get(*i) != Some(&Token::RParen) {
                    args.push(parse_expr(tokens, i)?);
                    while tokens.get(*i) == Some(&Token::Comma) {
                        *i += 1; // Consume comma
                        args.push(parse_expr(tokens, i)?);
                    }
                }
                expect(tokens, i, &Token::RParen)?;
                Ok(Expr::Call(name, args))
            } else {
                Ok(Expr::Variable(name))
            }
        }
        other => Err(format!("Expected expression, got {:?}", other)),
    }
}

fn parse_stmt(tokens: &[Token], i: &mut usize) -> Result<Stmt, String> {
    match tokens.get(*i) {
        Some(Token::Return) => {
            *i += 1;
            let expr = parse_expr(tokens, i)?;
            expect(tokens, i, &Token::Semicolon)?;
            Ok(Stmt::Return(expr))
        }
        Some(Token::Ident(keyword)) if keyword == "asm" || keyword == "__asm__" => {
            *i += 1;
            expect(tokens, i, &Token::LParen)?;
            let asm_str = match tokens.get(*i) {
                Some(Token::StringLiteral(s)) => {
                    let val = s.clone();
                    *i += 1;
                    val
                }
                other => return Err(format!("Expected string literal inside asm block, got {:?}", other)),
            };
            expect(tokens, i, &Token::RParen)?;
            expect(tokens, i, &Token::Semicolon)?;
            Ok(Stmt::Asm(asm_str))
        }
        Some(Token::Ident(type_or_var)) => {
            let type_or_var = type_or_var.clone();
            *i += 1;
            
            // Check if it's a variable declaration: e.g. "uint64_t x = ..."
            if let Some(Token::Ident(var_name)) = tokens.get(*i) {
                let var_name = var_name.clone();
                *i += 1;
                expect(tokens, i, &Token::Equal)?;
                let expr = parse_expr(tokens, i)?;
                expect(tokens, i, &Token::Semicolon)?;
                Ok(Stmt::VarDecl { name: var_name, val: expr })
            } else {
                // Otherwise it's an assignment: e.g. "x = ..."
                expect(tokens, i, &Token::Equal)?;
                let expr = parse_expr(tokens, i)?;
                expect(tokens, i, &Token::Semicolon)?;
                Ok(Stmt::Assign { name: type_or_var, val: expr })
            }
        }
        other => Err(format!("Expected statement, got {:?}", other)),
    }
}

fn parse_function(tokens: &[Token], i: &mut usize) -> Result<Function, String> {
    // Return-type identifier
    match tokens.get(*i) {
        Some(Token::Ident(_)) => *i += 1,
        _ => return Err("Expected return-type identifier".to_string()),
    }

    // Function name
    let name = match tokens.get(*i) {
        Some(Token::Ident(n)) => { let s = n.clone(); *i += 1; s }
        _ => return Err("Expected function name".to_string()),
    };

    expect(tokens, i, &Token::LParen)?;

    // Parse parameter list: e.g. (uint64_t a, uint64_t b)
    let mut params = Vec::new();
    if tokens.get(*i) != Some(&Token::RParen) {
        // First parameter
        let param_ty = match tokens.get(*i) {
            Some(Token::Ident(t)) => { let s = t.clone(); *i += 1; s }
            _ => return Err("Expected parameter type".to_string()),
        };
        let param_name = match tokens.get(*i) {
            Some(Token::Ident(n)) => { let s = n.clone(); *i += 1; s }
            _ => return Err("Expected parameter name".to_string()),
        };
        params.push(Param { ty: param_ty, name: param_name });

        // Additional parameters
        while tokens.get(*i) == Some(&Token::Comma) {
            *i += 1; // Consume comma
            let param_ty = match tokens.get(*i) {
                Some(Token::Ident(t)) => { let s = t.clone(); *i += 1; s }
                _ => return Err("Expected parameter type".to_string()),
            };
            let param_name = match tokens.get(*i) {
                Some(Token::Ident(n)) => { let s = n.clone(); *i += 1; s }
                _ => return Err("Expected parameter name".to_string()),
            };
            params.push(Param { ty: param_ty, name: param_name });
        }
    }

    expect(tokens, i, &Token::RParen)?;

    expect(tokens, i, &Token::LBrace)?;

    let mut body = Vec::new();
    while let Some(tok) = tokens.get(*i) {
        if tok == &Token::RBrace {
            break;
        }
        body.push(parse_stmt(tokens, i)?);
    }

    expect(tokens, i, &Token::RBrace)?;

    Ok(Function { name, params, body })
}

pub fn parse_return_value(tokens: &[Token]) -> Result<u64, String> {
    let map = parse_functions(tokens)?;
    let first_func = map.into_values().next().ok_or_else(|| "No functions found".to_string())?;
    if let Some(Stmt::Return(Expr::Number(n))) = first_func.body.first() {
        Ok(*n)
    } else {
        Err("Expected a simple return of integer literal".to_string())
    }
}

fn expect(tokens: &[Token], i: &mut usize, expected: &Token) -> Result<(), String> {
    match tokens.get(*i) {
        Some(t) if t == expected => { *i += 1; Ok(()) }
        other => Err(format!("Expected {:?}, got {:?}", expected, other)),
    }
}