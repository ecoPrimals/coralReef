// SPDX-License-Identifier: AGPL-3.0-only
//! GLSL tokenizer — tokenizes GLSL 450/460 source into a token stream (zero dependencies).

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

/// GLSL tokens.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    IntLiteral(i64),
    UintLiteral(u64),
    FloatLiteral(f64),
    BoolLiteral(bool),

    Ident(String),

    // Keywords — types
    Void,
    Int,
    Uint,
    Float,
    Double,
    Bool,
    Vec2,
    Vec3,
    Vec4,
    Ivec2,
    Ivec3,
    Ivec4,
    Uvec2,
    Uvec3,
    Uvec4,
    Mat2,
    Mat3,
    Mat4,
    Struct,
    Layout,
    Buffer,
    Uniform,
    Shared,
    In,
    Out,
    If,
    Else,
    For,
    While,
    Do,
    Switch,
    Case,
    Default,
    Break,
    Continue,
    Return,
    Const,

    // Qualifiers
    Readonly,
    Writeonly,
    Restrict,
    Coherent,
    Volatile,

    // GLSL compute builtins / intrinsics (also lexed as idents; we normalize in keyword map)
    GlGlobalInvocationId,
    GlLocalInvocationId,
    GlWorkGroupId,
    GlNumWorkGroups,
    GlLocalInvocationIndex,
    GlWorkGroupSize,
    Barrier,
    MemoryBarrier,
    MemoryBarrierBuffer,
    MemoryBarrierShared,
    GroupMemoryBarrier,

    // Punctuation
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Semicolon,
    Comma,
    Period,
    Colon,
    Question,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    Bang,
    AmpersandAmpersand,
    PipePipe,
    ShiftLeft,
    ShiftRight,
    EqualEqual,
    BangEqual,
    LessEqual,
    GreaterEqual,
    LessAngle,
    GreaterAngle,
    Equal,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    PercentEqual,
    AmpEqual,
    PipeEqual,
    CaretEqual,
    ShiftLeftEqual,
    ShiftRightEqual,
    PlusPlus,
    MinusMinus,

    Eof,
}

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

    fn skip_whitespace_comments_preproc(&mut self) {
        loop {
            match self.peek_byte() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => {
                    self.pos += 1;
                }
                Some(b'#') => {
                    while let Some(b) = self.peek_byte() {
                        self.pos += 1;
                        if b == b'\n' {
                            break;
                        }
                    }
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

    fn keyword(&self, text: &str) -> Option<Token> {
        Some(match text {
            "void" => Token::Void,
            "int" => Token::Int,
            "uint" => Token::Uint,
            "float" => Token::Float,
            "double" => Token::Double,
            "bool" => Token::Bool,
            "vec2" => Token::Vec2,
            "vec3" => Token::Vec3,
            "vec4" => Token::Vec4,
            "ivec2" => Token::Ivec2,
            "ivec3" => Token::Ivec3,
            "ivec4" => Token::Ivec4,
            "uvec2" => Token::Uvec2,
            "uvec3" => Token::Uvec3,
            "uvec4" => Token::Uvec4,
            "mat2" => Token::Mat2,
            "mat3" => Token::Mat3,
            "mat4" => Token::Mat4,
            "struct" => Token::Struct,
            "layout" => Token::Layout,
            "buffer" => Token::Buffer,
            "uniform" => Token::Uniform,
            "shared" => Token::Shared,
            "in" => Token::In,
            "out" => Token::Out,
            "if" => Token::If,
            "else" => Token::Else,
            "for" => Token::For,
            "while" => Token::While,
            "do" => Token::Do,
            "switch" => Token::Switch,
            "case" => Token::Case,
            "default" => Token::Default,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "return" => Token::Return,
            "const" => Token::Const,
            "readonly" => Token::Readonly,
            "writeonly" => Token::Writeonly,
            "restrict" => Token::Restrict,
            "coherent" => Token::Coherent,
            "volatile" => Token::Volatile,
            "gl_GlobalInvocationID" => Token::GlGlobalInvocationId,
            "gl_LocalInvocationID" => Token::GlLocalInvocationId,
            "gl_WorkGroupID" => Token::GlWorkGroupId,
            "gl_NumWorkGroups" => Token::GlNumWorkGroups,
            "gl_LocalInvocationIndex" => Token::GlLocalInvocationIndex,
            "gl_WorkGroupSize" => Token::GlWorkGroupSize,
            "barrier" => Token::Barrier,
            "memoryBarrier" => Token::MemoryBarrier,
            "memoryBarrierBuffer" => Token::MemoryBarrierBuffer,
            "memoryBarrierShared" => Token::MemoryBarrierShared,
            "groupMemoryBarrier" => Token::GroupMemoryBarrier,
            "true" => Token::BoolLiteral(true),
            "false" => Token::BoolLiteral(false),
            _ => return None,
        })
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
        let token = self
            .keyword(text)
            .unwrap_or_else(|| Token::Ident(text.to_string()));
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
            if self.peek_byte() == Some(b'.')
                && self
                    .bytes
                    .get(self.pos + 1)
                    .map_or(false, |b| b.is_ascii_digit())
            {
                is_float = true;
                self.pos = start;
                self.advance(); // '.'
                while let Some(b) = self.peek_byte() {
                    if b.is_ascii_digit() || b == b'_' {
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
                if self.peek_byte() == Some(b'.')
                    && self
                        .bytes
                        .get(self.pos + 1)
                        .map_or(false, |b| b.is_ascii_digit())
                {
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

        let is_uint = self.peek_byte() == Some(b'u') || self.peek_byte() == Some(b'U');
        if is_uint {
            self.pos += 1;
        }

        let span = Span {
            start: start as u32,
            end: self.pos as u32,
        };

        if is_float {
            let val: f64 = text.parse().unwrap_or(0.0);
            return Spanned {
                value: Token::FloatLiteral(val),
                span,
            };
        }
        if is_uint {
            let val = if is_hex {
                u64::from_str_radix(&text[2..], 16).unwrap_or(0)
            } else {
                text.parse().unwrap_or(0)
            };
            return Spanned {
                value: Token::UintLiteral(val),
                span,
            };
        }
        if is_hex {
            let val = i64::from_str_radix(&text[2..], 16).unwrap_or(0);
            return Spanned {
                value: Token::IntLiteral(val),
                span,
            };
        }
        let val: i64 = text.parse().unwrap_or(0);
        Spanned {
            value: Token::IntLiteral(val),
            span,
        }
    }

    pub fn next_token(&mut self) -> Spanned<Token> {
        self.skip_whitespace_comments_preproc();

        let start = self.pos;
        let Some(b) = self.advance() else {
            return Spanned {
                value: Token::Eof,
                span: Span {
                    start: start as u32,
                    end: start as u32,
                },
            };
        };

        let make = |tok: Token, end: usize| Spanned {
            value: tok,
            span: Span {
                start: start as u32,
                end: end as u32,
            },
        };

        match b {
            b'(' => make(Token::LeftParen, self.pos),
            b')' => make(Token::RightParen, self.pos),
            b'{' => make(Token::LeftBrace, self.pos),
            b'}' => make(Token::RightBrace, self.pos),
            b'[' => make(Token::LeftBracket, self.pos),
            b']' => make(Token::RightBracket, self.pos),
            b';' => make(Token::Semicolon, self.pos),
            b',' => make(Token::Comma, self.pos),
            b':' => make(Token::Colon, self.pos),
            b'?' => make(Token::Question, self.pos),
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
                Some(b'=') => {
                    self.pos += 1;
                    make(Token::PlusEqual, self.pos)
                }
                Some(b'+') => {
                    self.pos += 1;
                    make(Token::PlusPlus, self.pos)
                }
                _ => make(Token::Plus, self.pos),
            },
            b'-' => match self.peek_byte() {
                Some(b'=') => {
                    self.pos += 1;
                    make(Token::MinusEqual, self.pos)
                }
                Some(b'-') => {
                    self.pos += 1;
                    make(Token::MinusMinus, self.pos)
                }
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
                Some(b'&') => {
                    self.pos += 1;
                    make(Token::AmpersandAmpersand, self.pos)
                }
                Some(b'=') => {
                    self.pos += 1;
                    make(Token::AmpEqual, self.pos)
                }
                _ => make(Token::Ampersand, self.pos),
            },
            b'|' => match self.peek_byte() {
                Some(b'|') => {
                    self.pos += 1;
                    make(Token::PipePipe, self.pos)
                }
                Some(b'=') => {
                    self.pos += 1;
                    make(Token::PipeEqual, self.pos)
                }
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
                Some(b'=') => {
                    self.pos += 1;
                    make(Token::LessEqual, self.pos)
                }
                Some(b'<') => {
                    self.pos += 1;
                    if self.peek_byte() == Some(b'=') {
                        self.pos += 1;
                        make(Token::ShiftLeftEqual, self.pos)
                    } else {
                        make(Token::ShiftLeft, self.pos)
                    }
                }
                _ => make(Token::LessAngle, self.pos),
            },
            b'>' => match self.peek_byte() {
                Some(b'=') => {
                    self.pos += 1;
                    make(Token::GreaterEqual, self.pos)
                }
                Some(b'>') => {
                    self.pos += 1;
                    if self.peek_byte() == Some(b'=') {
                        self.pos += 1;
                        make(Token::ShiftRightEqual, self.pos)
                    } else {
                        make(Token::ShiftRight, self.pos)
                    }
                }
                _ => make(Token::GreaterAngle, self.pos),
            },
            b if b.is_ascii_alphabetic() || b == b'_' => {
                self.pos = start;
                self.pos += 1;
                self.read_ident_or_keyword(start)
            }
            b if b.is_ascii_digit() => {
                self.pos = start;
                self.read_number(start)
            }
            b'.' => {
                if self
                    .bytes
                    .get(self.pos)
                    .map_or(false, |x| x.is_ascii_digit())
                {
                    self.pos = start;
                    self.read_number(start)
                } else {
                    make(Token::Period, self.pos)
                }
            }
            _ => make(
                Token::Ident(format!("<unknown:{}>", b as char)),
                self.pos,
            ),
        }
    }

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
