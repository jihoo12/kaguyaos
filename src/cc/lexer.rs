use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    Ident(String),
    Number(u64),
    CharLiteral(u64),
    StringLiteral(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semicolon,
    Comma,
    Return,
    Equal,
}

pub fn lex(src: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = src.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => { chars.next(); }
            '(' => { tokens.push(Token::LParen);    chars.next(); }
            ')' => { tokens.push(Token::RParen);    chars.next(); }
            '{' => { tokens.push(Token::LBrace);    chars.next(); }
            '}' => { tokens.push(Token::RBrace);    chars.next(); }
            ';' => { tokens.push(Token::Semicolon); chars.next(); }
            ',' => { tokens.push(Token::Comma);     chars.next(); }
            '=' => { tokens.push(Token::Equal);     chars.next(); }
            '\'' => {
                chars.next(); // Consume opening quote
                let ch_val = if let Some(&c) = chars.peek() {
                    if c == '\\' {
                        chars.next();
                        match chars.next() {
                            Some('n')  => b'\n' as u64,
                            Some('r')  => b'\r' as u64,
                            Some('t')  => b'\t' as u64,
                            Some('\\') => b'\\' as u64,
                            Some('\'') => b'\'' as u64,
                            Some('0')  => 0u64,
                            Some(other) => return Err(format!("Unknown char escape: '\\{}'", other)),
                            None => return Err(format!("Unterminated char literal")),
                        }
                    } else {
                        chars.next();
                        c as u64
                    }
                } else {
                    return Err(format!("Unterminated char literal"));
                };
                // Expect closing single quote
                match chars.next() {
                    Some('\'') => {}
                    _ => return Err(format!("Unterminated char literal, expected closing '")),
                }
                tokens.push(Token::CharLiteral(ch_val));
            }
            '"' => {
                chars.next(); // Consume opening quote
                let mut content = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '"' {
                        chars.next(); // Consume closing quote
                        break;
                    } else if c == '\\' {
                        chars.next();
                        if let Some(escaped) = chars.next() {
                            match escaped {
                                'n' => content.push('\n'),
                                'r' => content.push('\r'),
                                't' => content.push('\t'),
                                '\\' => content.push('\\'),
                                '"' => content.push('"'),
                                other => {
                                    content.push('\\');
                                    content.push(other);
                                }
                            }
                        }
                    } else {
                        content.push(c);
                        chars.next();
                    }
                }
                tokens.push(Token::StringLiteral(content));
            }
            '0'..='9' => {
                let mut num = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() { num.push(d); chars.next(); } else { break; }
                }
                let n = num.parse::<u64>()
                    .map_err(|_| format!("Bad number: {}", num))?;
                tokens.push(Token::Number(n));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' { word.push(c); chars.next(); } else { break; }
                }
                let tok = if word == "return" { Token::Return } else { Token::Ident(word) };
                tokens.push(tok);
            }
            other => return Err(format!("Unexpected char: '{}'", other)),
        }
    }
    Ok(tokens)
}