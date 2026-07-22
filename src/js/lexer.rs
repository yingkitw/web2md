//! Tokenizer for the built-in JavaScript subset interpreter.
//!
//! Supports: numbers, single/double-quoted strings with escapes, template
//! literals with `${...}` interpolation, identifiers, keywords, comments
//! (// and /* */), and a practical set of punctuators.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Num(f64),
    Str(String),
    Template(Vec<TmplTokenPart>),
    Ident(String),
    Keyword(String),
    Punct(&'static str),
    Eof,
}

/// Raw template parts as produced by the lexer: literal text or unparsed
/// expression source. The parser converts these into [`super::ast::TmplPart`]
/// with the expression source parsed into an AST expression.
#[derive(Debug, Clone, PartialEq)]
pub enum TmplTokenPart {
    Text(String),
    Expr(String),
}

const KEYWORDS: &[&str] = &[
    "var",
    "let",
    "const",
    "if",
    "else",
    "for",
    "while",
    "do",
    "function",
    "return",
    "true",
    "false",
    "null",
    "undefined",
    "new",
    "typeof",
    "of",
    "in",
    "break",
    "continue",
    "this",
    "throw",
    "try",
    "catch",
    "finally",
    "switch",
    "case",
    "default",
    "void",
    "delete",
    "instanceof",
];

const PUNCTUATORS: &[&str] = &[
    ">>>=", ">>>", "...", "===", "!==", "=>", "**=", "**", "<<=", ">>=", "<<", ">>", "==", "!=",
    "<=", ">=", "&&", "||", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "?.", "?",
    "{", "}", "(", ")", "[", "]", ";", ",", ".", ":", "<", ">", "+", "-", "*", "/", "%", "=", "!",
    "&", "|", "^", "~",
];

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
}

#[derive(Debug, Clone)]
pub struct LexError(pub String);

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lex error: {}", self.0)
    }
}
impl std::error::Error for LexError {}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, off: usize) -> Option<u8> {
        self.src.get(self.pos + off).copied()
    }

    #[allow(dead_code)]
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if (c as char).is_ascii_whitespace() => {
                    self.pos += 1;
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    self.pos += 2;
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    self.pos += 2;
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == b'*' && self.peek() == Some(b'/') {
                            self.pos += 1;
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_ws_and_comments();
        let c = match self.peek() {
            Some(c) => c,
            None => return Ok(Token::Eof),
        };

        // Numbers
        if c.is_ascii_digit() || (c == b'.' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit()))
        {
            return self.read_number();
        }

        // Strings
        if c == b'"' || c == b'\'' {
            let quote = c;
            self.pos += 1;
            return self.read_string(quote);
        }

        // Template literal
        if c == b'`' {
            self.pos += 1;
            return self.read_template();
        }

        // Identifiers / keywords
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            return Ok(self.read_ident());
        }

        // Punctuators (longest match)
        for p in PUNCTUATORS {
            if self.matches_punct(p) {
                self.pos += p.len();
                return Ok(Token::Punct(p));
            }
        }

        Err(LexError(format!(
            "unexpected character {:?} at offset {}",
            c as char, self.pos
        )))
    }

    fn matches_punct(&self, p: &'static str) -> bool {
        let pb = p.as_bytes();
        if self.pos + pb.len() > self.src.len() {
            return false;
        }
        &self.src[self.pos..self.pos + pb.len()] == pb
    }

    fn read_number(&mut self) -> Result<Token, LexError> {
        let start = self.pos;
        if self.peek() == Some(b'0')
            && matches!(self.peek_at(1), Some(b'x') | Some(b'X'))
        {
            self.pos += 2;
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let text = std::str::from_utf8(&self.src[start + 2..self.pos]).unwrap_or("");
            let n =
                i64::from_str_radix(text, 16).map_err(|e| LexError(format!("bad hex: {e}")))? as f64;
            return Ok(Token::Num(n));
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        // exponent
        if let Some(e) = self.peek()
            && (e == b'e' || e == b'E') {
                self.pos += 1;
                if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                    self.pos += 1;
                }
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("0");
        let n: f64 = text.parse().map_err(|e| LexError(format!("bad number: {e}")))?;
        Ok(Token::Num(n))
    }

    fn read_string(&mut self, quote: u8) -> Result<Token, LexError> {
        let mut out = String::new();
        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => return Err(LexError("unterminated string".into())),
            };
            if c == quote {
                self.pos += 1;
                break;
            }
            if c == b'\n' {
                return Err(LexError("unterminated string (newline)".into()));
            }
            if c == b'\\' {
                self.pos += 1;
                let esc = self.peek().ok_or_else(|| LexError("bad escape".into()))?;
                self.pos += 1;
                match esc {
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'v' => out.push('\u{000B}'),
                    b'0' => out.push('\0'),
                    b'\\' => out.push('\\'),
                    b'\'' => out.push('\''),
                    b'"' => out.push('"'),
                    b'`' => out.push('`'),
                    b'/' => out.push('/'),
                    b'\n' => {} // line continuation
                    b'x' => {
                        let h = self
                            .read_hex(2)
                            .ok_or_else(|| LexError("bad \\x escape".into()))?;
                        if let Some(ch) = char::from_u32(h as u32) {
                            out.push(ch);
                        }
                    }
                    b'u' => {
                        if self.peek() == Some(b'{') {
                            self.pos += 1;
                            let mut code = 0u32;
                            while let Some(c) = self.peek() {
                                if c == b'}' {
                                    self.pos += 1;
                                    break;
                                }
                                code = code * 16
                                    + (c as char).to_digit(16).unwrap_or(0);
                                self.pos += 1;
                            }
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            }
                        } else {
                            let h = self
                                .read_hex(4)
                                .ok_or_else(|| LexError("bad \\u escape".into()))?;
                            if let Some(ch) = char::from_u32(h as u32) {
                                out.push(ch);
                            }
                        }
                    }
                    other => out.push(other as char),
                }
            } else {
                // copy raw UTF-8 byte sequence
                let ch_start = self.pos;
                self.pos += utf8_len(c);
                out.push_str(std::str::from_utf8(&self.src[ch_start..self.pos]).unwrap_or(""));
            }
        }
        Ok(Token::Str(out))
    }

    fn read_hex(&mut self, n: usize) -> Option<u64> {
        let mut v = 0u64;
        for _ in 0..n {
            let c = self.peek()?;
            let d = (c as char).to_digit(16)?;
            v = v * 16 + d as u64;
            self.pos += 1;
        }
        Some(v)
    }

    fn read_template(&mut self) -> Result<Token, LexError> {
        let mut parts: Vec<TmplTokenPart> = Vec::new();
        let mut text = String::new();
        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => return Err(LexError("unterminated template".into())),
            };
            match c {
                b'`' => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    self.pos += 1;
                    let esc = self.peek().ok_or_else(|| LexError("bad escape".into()))?;
                    self.pos += 1;
                    match esc {
                        b'n' => text.push('\n'),
                        b't' => text.push('\t'),
                        b'r' => text.push('\r'),
                        b'`' => text.push('`'),
                        b'$' => text.push('$'),
                        b'\\' => text.push('\\'),
                        other => text.push(other as char),
                    }
                }
                b'$' if self.peek_at(1) == Some(b'{') => {
                    if !text.is_empty() {
                        parts.push(TmplTokenPart::Text(std::mem::take(&mut text)));
                    }
                    self.pos += 2;
                    let expr_src = self.read_template_expr()?;
                    parts.push(TmplTokenPart::Expr(expr_src));
                }
                _ => {
                    let ch_start = self.pos;
                    self.pos += utf8_len(c);
                    text.push_str(std::str::from_utf8(&self.src[ch_start..self.pos]).unwrap_or(""));
                }
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(TmplTokenPart::Text(text));
        }
        Ok(Token::Template(parts))
    }

    /// Read until the matching `}` that closes a `${...}` expression.
    /// Tracks nested braces, strings, and templates to avoid false matches.
    fn read_template_expr(&mut self) -> Result<String, LexError> {
        let start = self.pos;
        let mut depth: i32 = 1;
        while let Some(c) = self.peek() {
            match c {
                b'{' => {
                    depth += 1;
                    self.pos += 1;
                }
                b'}' => {
                    depth -= 1;
                    self.pos += 1;
                    if depth == 0 {
                        let s = std::str::from_utf8(&self.src[start..self.pos - 1])
                            .unwrap_or("")
                            .to_string();
                        return Ok(s);
                    }
                }
                b'"' | b'\'' => {
                    let q = c;
                    self.pos += 1;
                    while let Some(sc) = self.peek() {
                        self.pos += 1;
                        if sc == b'\\' {
                            self.pos += 1;
                        } else if sc == q {
                            break;
                        }
                    }
                }
                b'`' => {
                    self.pos += 1;
                    let mut tdepth: i32 = 1;
                    while let Some(sc) = self.peek() {
                        if sc == b'\\' {
                            self.pos += 2;
                            continue;
                        }
                        if sc == b'`' {
                            self.pos += 1;
                            tdepth -= 1;
                            if tdepth == 0 {
                                break;
                            }
                        } else if sc == b'$' && self.peek_at(1) == Some(b'{') {
                            self.pos += 2;
                            let _ = self.read_template_expr()?;
                        } else {
                            self.pos += 1;
                        }
                    }
                }
                _ => self.pos += utf8_len(c),
            }
        }
        Err(LexError("unterminated template expression".into()))
    }

    fn read_ident(&mut self) -> Token {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
        if KEYWORDS.contains(&s) {
            Token::Keyword(s.to_string())
        } else {
            Token::Ident(s.to_string())
        }
    }
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else if first >> 3 == 0b1_1110 {
        4
    } else {
        1
    }
}

/// Tokenize an entire source string.
pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    let mut lx = Lexer::new(src);
    let mut out = Vec::new();
    loop {
        let t = lx.next_token()?;
        let eof = t == Token::Eof;
        out.push(t);
        if eof {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_numbers() {
        let t = tokenize("42 3.14 0x1F 1e3").unwrap();
        assert!(matches!(t[0], Token::Num(n) if n == 42.0));
        assert!(matches!(t[1], Token::Num(n) if n == 3.14));
        assert!(matches!(t[2], Token::Num(n) if n == 31.0));
        assert!(matches!(t[3], Token::Num(n) if n == 1000.0));
    }

    #[test]
    fn lex_strings_with_escapes() {
        let t = tokenize(r#""a\nb\tc""#).unwrap();
        assert!(matches!(&t[0], Token::Str(s) if s == "a\nb\tc"));
    }

    #[test]
    fn lex_template_literal() {
        let t = tokenize("`hello ${name}!`").unwrap();
        match &t[0] {
            Token::Template(parts) => {
                assert_eq!(parts.len(), 3);
                assert!(matches!(&parts[0], TmplTokenPart::Text(s) if s == "hello "));
                assert!(matches!(&parts[1], TmplTokenPart::Expr(s) if s.trim() == "name"));
                assert!(matches!(&parts[2], TmplTokenPart::Text(s) if s == "!"));
            }
            other => panic!("expected template, got {:?}", other),
        }
    }

    #[test]
    fn lex_keywords_and_idents() {
        let t = tokenize("var x = true").unwrap();
        assert!(matches!(&t[0], Token::Keyword(s) if s == "var"));
        assert!(matches!(&t[1], Token::Ident(s) if s == "x"));
        assert!(matches!(&t[2], Token::Punct("=")));
        assert!(matches!(&t[3], Token::Keyword(s) if s == "true"));
    }

    #[test]
    fn lex_comments_skipped() {
        let t = tokenize("a // comment\nb /* block */ c").unwrap();
        let idents: Vec<&str> = t
            .iter()
            .filter_map(|tok| match tok {
                Token::Ident(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(idents, vec!["a", "b", "c"]);
    }
}
