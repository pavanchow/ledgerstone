//! A hand-written lexer for the Ledgerstone SQL subset. Turns raw text into a
//! flat token list. Never panics: unrecognized input becomes `LsError::Syntax`.

use crate::error::{LsError, LsResult};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Create,
    Table,
    Insert,
    Into,
    Values,
    Select,
    From,
    Where,
    Order,
    By,
    Asc,
    Desc,
    Limit,
    Delete,
    Update,
    Set,
    And,
    Or,
    Integer,
    Text,
    Real,

    Ident(String),
    IntLit(i64),
    RealLit(f64),
    StrLit(String),

    Star,
    Comma,
    LParen,
    RParen,
    Semicolon,
    Dot,
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,

    Eof,
}

/// Caps how many tokens a single statement may produce, a cheap backstop
/// against pathological input long before the parser's own depth guard runs.
const MAX_TOKENS: usize = 100_000;

pub fn lex(src: &str) -> LsResult<Vec<Token>> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut out = Vec::new();

    while i < chars.len() {
        if out.len() > MAX_TOKENS {
            return Err(LsError::Syntax("statement too long".into()));
        }
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // line comment: -- to end of line
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        match c {
            '*' => {
                out.push(Token::Star);
                i += 1;
            }
            ',' => {
                out.push(Token::Comma);
                i += 1;
            }
            '(' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' => {
                out.push(Token::RParen);
                i += 1;
            }
            ';' => {
                out.push(Token::Semicolon);
                i += 1;
            }
            '.' => {
                out.push(Token::Dot);
                i += 1;
            }
            '=' => {
                out.push(Token::Eq);
                i += 1;
            }
            '!' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    out.push(Token::NotEq);
                    i += 2;
                } else {
                    return Err(LsError::Syntax(format!("unexpected character '!' at {i}")));
                }
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    out.push(Token::Le);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '>' {
                    out.push(Token::NotEq);
                    i += 2;
                } else {
                    out.push(Token::Lt);
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    out.push(Token::Ge);
                    i += 2;
                } else {
                    out.push(Token::Gt);
                    i += 1;
                }
            }
            '\'' => {
                let (s, next) = lex_string(&chars, i)?;
                out.push(Token::StrLit(s));
                i = next;
            }
            _ if c.is_ascii_digit() => {
                let (tok, next) = lex_number(&chars, i)?;
                out.push(tok);
                i = next;
            }
            _ if c.is_alphabetic() || c == '_' => {
                let (word, next) = lex_ident(&chars, i);
                out.push(keyword_or_ident(&word));
                i = next;
            }
            _ => {
                return Err(LsError::Syntax(format!(
                    "unexpected character '{c}' at position {i}"
                )));
            }
        }
    }

    out.push(Token::Eof);
    Ok(out)
}

fn lex_string(chars: &[char], start: usize) -> LsResult<(String, usize)> {
    let mut i = start + 1;
    let mut s = String::new();
    loop {
        if i >= chars.len() {
            return Err(LsError::Syntax("unterminated string literal".into()));
        }
        let c = chars[i];
        if c == '\'' {
            // '' inside a string is an escaped single quote
            if i + 1 < chars.len() && chars[i + 1] == '\'' {
                s.push('\'');
                i += 2;
                continue;
            }
            return Ok((s, i + 1));
        }
        s.push(c);
        i += 1;
    }
}

fn lex_number(chars: &[char], start: usize) -> LsResult<(Token, usize)> {
    let mut i = start;
    let mut is_real = false;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
        is_real = true;
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    let text: String = chars[start..i].iter().collect();
    if is_real {
        let v: f64 = text
            .parse()
            .map_err(|_| LsError::Syntax(format!("invalid numeric literal '{text}'")))?;
        Ok((Token::RealLit(v), i))
    } else {
        let v: i64 = text
            .parse()
            .map_err(|_| LsError::Syntax(format!("invalid numeric literal '{text}'")))?;
        Ok((Token::IntLit(v), i))
    }
}

fn lex_ident(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}

fn keyword_or_ident(word: &str) -> Token {
    match word.to_ascii_uppercase().as_str() {
        "CREATE" => Token::Create,
        "TABLE" => Token::Table,
        "INSERT" => Token::Insert,
        "INTO" => Token::Into,
        "VALUES" => Token::Values,
        "SELECT" => Token::Select,
        "FROM" => Token::From,
        "WHERE" => Token::Where,
        "ORDER" => Token::Order,
        "BY" => Token::By,
        "ASC" => Token::Asc,
        "DESC" => Token::Desc,
        "LIMIT" => Token::Limit,
        "DELETE" => Token::Delete,
        "UPDATE" => Token::Update,
        "SET" => Token::Set,
        "AND" => Token::And,
        "OR" => Token::Or,
        "INTEGER" | "INT" => Token::Integer,
        "TEXT" => Token::Text,
        "REAL" | "FLOAT" => Token::Real,
        _ => Token::Ident(word.to_string()),
    }
}
