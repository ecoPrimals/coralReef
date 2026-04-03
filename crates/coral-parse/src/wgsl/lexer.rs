// SPDX-License-Identifier: AGPL-3.0-only
//! WGSL lexer — tokenizes WGSL source text into a token stream.
//!
//! Zero external dependencies. Produces tokens with byte-offset spans
//! for error reporting.

/// Source span (byte offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// A token with its source span.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Span,
}

/// WGSL tokens.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    IntLiteral(i64),
    UintLiteral(u64),
    FloatLiteral(f64),
    BoolLiteral(bool),

    // Identifiers and keywords
    Ident(String),

    // Keywords
    Fn,
    Let,
    Var,
    Const,
    Return,
    If,
    Else,
    For,
    While,
    Loop,
    Break,
    Continue,
    Switch,
    Case,
    Default,
    Struct,
    Array,
    Atomic,

    // Type keywords
    Bool,
    F32,
    F64,
    I32,
    U32,
    Vec2,
    Vec3,
    Vec4,
    Mat2x2,
    Mat3x3,
    Mat4x4,
    Mat2x3,
    Mat2x4,
    Mat3x2,
    Mat3x4,
    Mat4x2,
    Mat4x3,
    Ptr,

    // Punctuation
    At,           // @
    LeftParen,    // (
    RightParen,   // )
    LeftBrace,    // {
    RightBrace,   // }
    LeftBracket,  // [
    RightBracket, // ]
    LeftAngle,    // <
    RightAngle,   // >
    Semicolon,    // ;
    Colon,        // :
    Comma,        // ,
    Period,       // .
    Arrow,        // ->
    Underscore,   // _

    // Operators
    Plus,         // +
    Minus,        // -
    Star,         // *
    Slash,        // /
    Percent,      // %
    Ampersand,    // &
    Pipe,         // |
    Caret,        // ^
    Tilde,        // ~
    Bang,         // !
    AmpersandAmpersand, // &&
    PipePipe,     // ||
    ShiftLeft,    // <<
    ShiftRight,   // >>

    // Comparison
    EqualEqual,   // ==
    BangEqual,    // !=
    LessEqual,    // <=
    GreaterEqual, // >=

    // Assignment
    Equal,        // =
    PlusEqual,    // +=
    MinusEqual,   // -=
    StarEqual,    // *=
    SlashEqual,   // /=
    PercentEqual, // %=
    AmpEqual,     // &=
    PipeEqual,    // |=
    CaretEqual,   // ^=
    ShiftLeftEqual,  // <<=
    ShiftRightEqual, // >>=
    PlusPlus,     // ++
    MinusMinus,   // --

    // Access
    Read,
    ReadWrite,
    Write,

    // Stage
    Compute,
    Vertex,
    Fragment,

    // Special builtins
    WorkgroupBarrier,
    StorageBarrier,

    // End of file
    Eof,
}

/// Tokenizer for WGSL source text.
pub struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek_byte() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => {
                    self.pos += 1;
                }
                Some(b'/') => {
                    if self.bytes.get(self.pos + 1) == Some(&b'/') {
                        self.pos += 2;
                        while let Some(b) = self.peek_byte() {
                            self.pos += 1;
                            if b == b'\n' {
                                break;
                            }
                        }
                    } else if self.bytes.get(self.pos + 1) == Some(&b'*') {
                        self.pos += 2;
                        let mut depth = 1u32;
                        while depth > 0 {
                            match self.advance() {
                                Some(b'/') if self.peek_byte() == Some(b'*') => {
                                    self.pos += 1;
                                    depth += 1;
                                }
                                Some(b'*') if self.peek_byte() == Some(b'/') => {
                                    self.pos += 1;
                                    depth -= 1;
                                }
                                None => break,
                                _ => {}
                            }
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    fn read_ident_or_keyword(&mut self, start: usize) -> Spanned<Token> {
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = &self.source[start..self.pos];
        let token = match text {
            "fn" => Token::Fn,
            "let" => Token::Let,
            "var" => Token::Var,
            "const" => Token::Const,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "for" => Token::For,
            "while" => Token::While,
            "loop" => Token::Loop,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "switch" => Token::Switch,
            "case" => Token::Case,
            "default" => Token::Default,
            "struct" => Token::Struct,
            "array" => Token::Array,
            "atomic" => Token::Atomic,
            "bool" => Token::Bool,
            "f32" => Token::F32,
            "f64" => Token::F64,
            "i32" => Token::I32,
            "u32" => Token::U32,
            "vec2" => Token::Vec2,
            "vec3" => Token::Vec3,
            "vec4" => Token::Vec4,
            "mat2x2" => Token::Mat2x2,
            "mat3x3" => Token::Mat3x3,
            "mat4x4" => Token::Mat4x4,
            "mat2x3" => Token::Mat2x3,
            "mat2x4" => Token::Mat2x4,
            "mat3x2" => Token::Mat3x2,
            "mat3x4" => Token::Mat3x4,
            "mat4x2" => Token::Mat4x2,
            "mat4x3" => Token::Mat4x3,
            "ptr" => Token::Ptr,
            "true" => Token::BoolLiteral(true),
            "false" => Token::BoolLiteral(false),
            "read" => Token::Read,
            "read_write" => Token::ReadWrite,
            "write" => Token::Write,
            "compute" => Token::Compute,
            "vertex" => Token::Vertex,
            "fragment" => Token::Fragment,
            "workgroupBarrier" => Token::WorkgroupBarrier,
            "storageBarrier" => Token::StorageBarrier,
            "_" => Token::Underscore,
            other => Token::Ident(other.to_string()),
        };
        Spanned {
            value: token,
            span: Span {
                start: start as u32,
                end: self.pos as u32,
            },
        }
    }

    fn read_number(&mut self, start: usize) -> Spanned<Token> {
        let mut is_float = false;
        let mut is_hex = false;

        if self.source[start..].starts_with("0x") || self.source[start..].starts_with("0X") {
            is_hex = true;
            self.pos = start + 2;
            while let Some(b) = self.peek_byte() {
                if b.is_ascii_hexdigit() || b == b'_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        } else {
            while let Some(b) = self.peek_byte() {
                if b.is_ascii_digit() || b == b'_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.peek_byte() == Some(b'.') && self.bytes.get(self.pos + 1).map_or(false, |b| b.is_ascii_digit()) {
                is_float = true;
                self.pos += 1;
                while let Some(b) = self.peek_byte() {
                    if b.is_ascii_digit() || b == b'_' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
            if self.peek_byte() == Some(b'e') || self.peek_byte() == Some(b'E') {
                is_float = true;
                self.pos += 1;
                if self.peek_byte() == Some(b'+') || self.peek_byte() == Some(b'-') {
                    self.pos += 1;
                }
                while let Some(b) = self.peek_byte() {
                    if b.is_ascii_digit() {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
        }

        let text: String = self.source[start..self.pos].chars().filter(|&c| c != '_').collect();

        // Check for type suffix
        let is_uint = self.peek_byte() == Some(b'u');
        if is_uint {
            self.pos += 1;
        }

        let span = Span {
            start: start as u32,
            end: self.pos as u32,
        };

        if is_float {
            let val: f64 = text.parse().unwrap_or(0.0);
            Spanned { value: Token::FloatLiteral(val), span }
        } else if is_uint {
            let val = if is_hex {
                u64::from_str_radix(&text[2..], 16).unwrap_or(0)
            } else {
                text.parse().unwrap_or(0)
            };
            Spanned { value: Token::UintLiteral(val), span }
        } else if is_hex {
            let val = i64::from_str_radix(&text[2..], 16).unwrap_or(0);
            Spanned { value: Token::IntLiteral(val), span }
        } else {
            let val: i64 = text.parse().unwrap_or(0);
            Spanned { value: Token::IntLiteral(val), span }
        }
    }

    /// Lex the next token from the source.
    pub fn next_token(&mut self) -> Spanned<Token> {
        self.skip_whitespace_and_comments();

        let start = self.pos;
        let Some(b) = self.advance() else {
            return Spanned {
                value: Token::Eof,
                span: Span { start: start as u32, end: start as u32 },
            };
        };

        let make = |tok: Token, end: usize| Spanned {
            value: tok,
            span: Span { start: start as u32, end: end as u32 },
        };

        match b {
            b'@' => make(Token::At, self.pos),
            b'(' => make(Token::LeftParen, self.pos),
            b')' => make(Token::RightParen, self.pos),
            b'{' => make(Token::LeftBrace, self.pos),
            b'}' => make(Token::RightBrace, self.pos),
            b'[' => make(Token::LeftBracket, self.pos),
            b']' => make(Token::RightBracket, self.pos),
            b';' => make(Token::Semicolon, self.pos),
            b':' => make(Token::Colon, self.pos),
            b',' => make(Token::Comma, self.pos),
            b'.' => make(Token::Period, self.pos),
            b'~' => make(Token::Tilde, self.pos),
            b'^' => {
                if self.peek_byte() == Some(b'=') {
                    self.pos += 1;
                    make(Token::CaretEqual, self.pos)
                } else {
                    make(Token::Caret, self.pos)
                }
            }
            b'+' => match self.peek_byte() {
                Some(b'=') => { self.pos += 1; make(Token::PlusEqual, self.pos) }
                Some(b'+') => { self.pos += 1; make(Token::PlusPlus, self.pos) }
                _ => make(Token::Plus, self.pos),
            },
            b'-' => match self.peek_byte() {
                Some(b'=') => { self.pos += 1; make(Token::MinusEqual, self.pos) }
                Some(b'-') => { self.pos += 1; make(Token::MinusMinus, self.pos) }
                Some(b'>') => { self.pos += 1; make(Token::Arrow, self.pos) }
                _ => make(Token::Minus, self.pos),
            },
            b'*' => {
                if self.peek_byte() == Some(b'=') {
                    self.pos += 1;
                    make(Token::StarEqual, self.pos)
                } else {
                    make(Token::Star, self.pos)
                }
            }
            b'/' => {
                if self.peek_byte() == Some(b'=') {
                    self.pos += 1;
                    make(Token::SlashEqual, self.pos)
                } else {
                    make(Token::Slash, self.pos)
                }
            }
            b'%' => {
                if self.peek_byte() == Some(b'=') {
                    self.pos += 1;
                    make(Token::PercentEqual, self.pos)
                } else {
                    make(Token::Percent, self.pos)
                }
            }
            b'&' => match self.peek_byte() {
                Some(b'&') => { self.pos += 1; make(Token::AmpersandAmpersand, self.pos) }
                Some(b'=') => { self.pos += 1; make(Token::AmpEqual, self.pos) }
                _ => make(Token::Ampersand, self.pos),
            },
            b'|' => match self.peek_byte() {
                Some(b'|') => { self.pos += 1; make(Token::PipePipe, self.pos) }
                Some(b'=') => { self.pos += 1; make(Token::PipeEqual, self.pos) }
                _ => make(Token::Pipe, self.pos),
            },
            b'!' => {
                if self.peek_byte() == Some(b'=') {
                    self.pos += 1;
                    make(Token::BangEqual, self.pos)
                } else {
                    make(Token::Bang, self.pos)
                }
            }
            b'=' => {
                if self.peek_byte() == Some(b'=') {
                    self.pos += 1;
                    make(Token::EqualEqual, self.pos)
                } else {
                    make(Token::Equal, self.pos)
                }
            }
            b'<' => match self.peek_byte() {
                Some(b'=') => { self.pos += 1; make(Token::LessEqual, self.pos) }
                Some(b'<') => {
                    self.pos += 1;
                    if self.peek_byte() == Some(b'=') {
                        self.pos += 1;
                        make(Token::ShiftLeftEqual, self.pos)
                    } else {
                        make(Token::ShiftLeft, self.pos)
                    }
                }
                _ => make(Token::LeftAngle, self.pos),
            },
            b'>' => match self.peek_byte() {
                Some(b'=') => { self.pos += 1; make(Token::GreaterEqual, self.pos) }
                Some(b'>') => {
                    self.pos += 1;
                    if self.peek_byte() == Some(b'=') {
                        self.pos += 1;
                        make(Token::ShiftRightEqual, self.pos)
                    } else {
                        make(Token::ShiftRight, self.pos)
                    }
                }
                _ => make(Token::RightAngle, self.pos),
            },
            b if b.is_ascii_alphabetic() || b == b'_' => {
                self.pos = start;
                self.pos += 1;
                self.read_ident_or_keyword(start)
            }
            b if b.is_ascii_digit() => {
                self.pos = start;
                self.pos += 1;
                self.read_number(start)
            }
            _ => make(Token::Ident(format!("<unknown:{}>", b as char)), self.pos),
        }
    }

    /// Lex all tokens from source.
    pub fn tokenize(source: &str) -> Vec<Spanned<Token>> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token();
            if tok.value == Token::Eof {
                tokens.push(tok);
                break;
            }
            tokens.push(tok);
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_simple_var_decl() {
        let tokens = Lexer::tokenize("var<storage, read> a: array<f32>;");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.value).collect();
        assert!(matches!(kinds[0], Token::Var));
        assert!(matches!(kinds[1], Token::LeftAngle));
        assert!(matches!(kinds[2], Token::Ident(s) if s == "storage"));
    }

    #[test]
    fn tokenize_compute_attribute() {
        let tokens = Lexer::tokenize("@compute @workgroup_size(1)");
        assert!(matches!(tokens[0].value, Token::At));
        assert!(matches!(tokens[1].value, Token::Compute));
        assert!(matches!(tokens[2].value, Token::At));
        assert!(matches!(tokens[3].value, Token::Ident(ref s) if s == "workgroup_size"));
    }

    #[test]
    fn tokenize_number_literals() {
        let tokens = Lexer::tokenize("42 3.14 0xFF 1u");
        assert!(matches!(tokens[0].value, Token::IntLiteral(42)));
        assert!(matches!(tokens[1].value, Token::FloatLiteral(f) if (f - 3.14).abs() < 1e-10));
        assert!(matches!(tokens[2].value, Token::IntLiteral(255)));
        assert!(matches!(tokens[3].value, Token::UintLiteral(1)));
    }

    #[test]
    fn tokenize_operators() {
        let tokens = Lexer::tokenize("a + b * c == d && e");
        assert!(matches!(tokens[1].value, Token::Plus));
        assert!(matches!(tokens[3].value, Token::Star));
        assert!(matches!(tokens[5].value, Token::EqualEqual));
        assert!(matches!(tokens[7].value, Token::AmpersandAmpersand));
    }

    #[test]
    fn tokenize_line_comment() {
        let tokens = Lexer::tokenize("a // comment\nb");
        let non_eof: Vec<_> = tokens.iter().filter(|t| t.value != Token::Eof).collect();
        assert_eq!(non_eof.len(), 2);
    }

    #[test]
    fn tokenize_block_comment() {
        let tokens = Lexer::tokenize("a /* block */ b");
        let non_eof: Vec<_> = tokens.iter().filter(|t| t.value != Token::Eof).collect();
        assert_eq!(non_eof.len(), 2);
    }

    #[test]
    fn tokenize_nested_block_comment() {
        let tokens = Lexer::tokenize("a /* outer /* inner */ end */ b");
        let non_eof: Vec<_> = tokens.iter().filter(|t| t.value != Token::Eof).collect();
        assert_eq!(non_eof.len(), 2);
    }

    #[test]
    fn tokenize_float_with_exponent() {
        let tokens = Lexer::tokenize("1e-5 2.5E3");
        assert!(matches!(tokens[0].value, Token::FloatLiteral(f) if (f - 1e-5).abs() < 1e-15));
        assert!(matches!(tokens[1].value, Token::FloatLiteral(f) if (f - 2500.0).abs() < 1e-10));
    }

    #[test]
    fn tokenize_compound_assign() {
        let tokens = Lexer::tokenize("+= -= *= /= %= &= |= ^= <<= >>=");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.value).filter(|t| **t != Token::Eof).collect();
        assert_eq!(kinds.len(), 10);
        assert!(matches!(kinds[0], Token::PlusEqual));
        assert!(matches!(kinds[8], Token::ShiftLeftEqual));
        assert!(matches!(kinds[9], Token::ShiftRightEqual));
    }

    #[test]
    fn tokenize_arrow() {
        let tokens = Lexer::tokenize("-> fn");
        assert!(matches!(tokens[0].value, Token::Arrow));
    }

    #[test]
    fn tokenize_increment_decrement() {
        let tokens = Lexer::tokenize("++ --");
        assert!(matches!(tokens[0].value, Token::PlusPlus));
        assert!(matches!(tokens[1].value, Token::MinusMinus));
    }
}
