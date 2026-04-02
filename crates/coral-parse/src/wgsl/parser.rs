// SPDX-License-Identifier: AGPL-3.0-only
//! Recursive descent WGSL parser — produces sovereign AST.
//!
//! Targets the compute shader subset for Evolution 1:
//! - `@compute @workgroup_size(x,y,z)` entry points
//! - `@group(n) @binding(n) var<storage|uniform> name: type;`
//! - `var<workgroup> name: type;`
//! - Scalar, vector, matrix, array, struct types
//! - Full expression grammar (math, comparison, logical, bitwise)
//! - Control flow: if/else, for, while, loop, switch, break, continue
//! - Barriers: `workgroupBarrier()`, `storageBarrier()`
//! - Atomics via built-in functions
//! - Shared memory via `var<workgroup>`

use super::lexer::{Lexer, Span, Spanned, Token};
use crate::ast::*;
use crate::error::ParseError;
use std::collections::HashMap;

struct Parser<'a> {
    tokens: Vec<Spanned<Token>>,
    pos: usize,
    source: &'a str,
    module: Module,
    expr_types: HashMap<u32, Handle<Type>>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        let tokens = Lexer::tokenize(source);
        Self {
            tokens,
            pos: 0,
            source,
            module: Module::new(),
            expr_types: HashMap::new(),
        }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).map_or(&Token::Eof, |t| &t.value)
    }

    fn peek_span(&self) -> Span {
        self.tokens.get(self.pos).map_or(
            Span { start: self.source.len() as u32, end: self.source.len() as u32 },
            |t| t.span,
        )
    }

    fn advance(&mut self) -> &Spanned<Token> {
        let tok = &self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<Span, ParseError> {
        if self.peek() == expected {
            let s = self.peek_span();
            self.advance();
            Ok(s)
        } else {
            Err(self.error(format!("expected {expected:?}, got {:?}", self.peek())))
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Ident(s) => { self.advance(); Ok(s) }
            other => Err(self.error(format!("expected identifier, got {other:?}"))),
        }
    }

    fn error(&self, msg: String) -> ParseError {
        let span = self.peek_span();
        ParseError::Syntax { offset: span.start, message: msg }
    }

    fn check(&self, tok: &Token) -> bool {
        self.peek() == tok
    }

    fn eat(&mut self, tok: &Token) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    // ---- Type parsing ----

    fn parse_type(&mut self) -> Result<Handle<Type>, ParseError> {
        match self.peek().clone() {
            Token::F32 => { self.advance(); Ok(self.module.types.append(Type::Scalar(Scalar::F32))) }
            Token::F64 => { self.advance(); Ok(self.module.types.append(Type::Scalar(Scalar::F64))) }
            Token::I32 => { self.advance(); Ok(self.module.types.append(Type::Scalar(Scalar::I32))) }
            Token::U32 => { self.advance(); Ok(self.module.types.append(Type::Scalar(Scalar::U32))) }
            Token::Bool => { self.advance(); Ok(self.module.types.append(Type::Bool)) }
            Token::Vec2 => self.parse_vector_type(VectorSize::Bi),
            Token::Vec3 => self.parse_vector_type(VectorSize::Tri),
            Token::Vec4 => self.parse_vector_type(VectorSize::Quad),
            Token::Mat2x2 => self.parse_matrix_type(VectorSize::Bi, VectorSize::Bi),
            Token::Mat3x3 => self.parse_matrix_type(VectorSize::Tri, VectorSize::Tri),
            Token::Mat4x4 => self.parse_matrix_type(VectorSize::Quad, VectorSize::Quad),
            Token::Mat2x3 => self.parse_matrix_type(VectorSize::Bi, VectorSize::Tri),
            Token::Mat2x4 => self.parse_matrix_type(VectorSize::Bi, VectorSize::Quad),
            Token::Mat3x2 => self.parse_matrix_type(VectorSize::Tri, VectorSize::Bi),
            Token::Mat3x4 => self.parse_matrix_type(VectorSize::Tri, VectorSize::Quad),
            Token::Mat4x2 => self.parse_matrix_type(VectorSize::Quad, VectorSize::Bi),
            Token::Mat4x3 => self.parse_matrix_type(VectorSize::Quad, VectorSize::Tri),
            Token::Array => self.parse_array_type(),
            Token::Atomic => self.parse_atomic_type(),
            Token::Struct => self.parse_struct_decl(),
            Token::Ident(name) => {
                let name = name.clone();
                match name.as_str() {
                    "sampler" => {
                        self.advance();
                        Ok(self.module.types.append(Type::Sampler { comparison: false }))
                    }
                    "sampler_comparison" => {
                        self.advance();
                        Ok(self.module.types.append(Type::Sampler { comparison: true }))
                    }
                    s if s.starts_with("texture_") => {
                        self.advance();
                        self.parse_texture_type_after_keyword(s)
                    }
                    _ => {
                        self.advance();
                        for (handle, ty) in self.module.types.iter() {
                            if let Type::Struct { name: Some(n), .. } = ty {
                                if *n == name {
                                    return Ok(handle);
                                }
                            }
                        }
                        Err(self.error(format!("unknown type: {name}")))
                    }
                }
            }
            other => Err(self.error(format!("expected type, got {other:?}"))),
        }
    }

    fn parse_texture_sample_type_param(&mut self) -> Result<TextureSampleType, ParseError> {
        match self.peek() {
            Token::F32 => {
                self.advance();
                Ok(TextureSampleType::Float { filterable: true })
            }
            Token::I32 => {
                self.advance();
                Ok(TextureSampleType::Sint)
            }
            Token::U32 => {
                self.advance();
                Ok(TextureSampleType::Uint)
            }
            other => Err(self.error(format!("expected texture sample type (f32, i32, u32), got {other:?}"))),
        }
    }

    fn parse_storage_format_from_ident(&mut self) -> Result<StorageFormat, ParseError> {
        let s = self.expect_ident()?;
        match s.as_str() {
            "r32float" => Ok(StorageFormat::R32Float),
            "r32sint" => Ok(StorageFormat::R32Sint),
            "r32uint" => Ok(StorageFormat::R32Uint),
            "rg32float" => Ok(StorageFormat::Rg32Float),
            "rg32sint" => Ok(StorageFormat::Rg32Sint),
            "rg32uint" => Ok(StorageFormat::Rg32Uint),
            "rgba8unorm" => Ok(StorageFormat::Rgba8Unorm),
            "rgba8snorm" => Ok(StorageFormat::Rgba8Snorm),
            "rgba8uint" => Ok(StorageFormat::Rgba8Uint),
            "rgba8sint" => Ok(StorageFormat::Rgba8Sint),
            "rgba16float" => Ok(StorageFormat::Rgba16Float),
            "rgba16sint" => Ok(StorageFormat::Rgba16Sint),
            "rgba16uint" => Ok(StorageFormat::Rgba16Uint),
            "rgba32float" => Ok(StorageFormat::Rgba32Float),
            "rgba32sint" => Ok(StorageFormat::Rgba32Sint),
            "rgba32uint" => Ok(StorageFormat::Rgba32Uint),
            "bgra8unorm" => Ok(StorageFormat::Bgra8Unorm),
            other => Err(self.error(format!("unknown storage texture format: {other}"))),
        }
    }

    fn parse_storage_texture_access(&mut self) -> Result<StorageAccess, ParseError> {
        match self.peek().clone() {
            Token::Ident(s) if s == "read" => {
                self.advance();
                Ok(StorageAccess::Load)
            }
            Token::Ident(s) if s == "write" => {
                self.advance();
                Ok(StorageAccess::Store)
            }
            Token::Ident(s) if s == "read_write" => {
                self.advance();
                Ok(StorageAccess::LoadStore)
            }
            Token::Read => {
                self.advance();
                Ok(StorageAccess::Load)
            }
            Token::Write => {
                self.advance();
                Ok(StorageAccess::Store)
            }
            Token::ReadWrite => {
                self.advance();
                Ok(StorageAccess::LoadStore)
            }
            other => Err(self.error(format!("expected storage texture access, got {other:?}"))),
        }
    }

    fn parse_texture_type_after_keyword(&mut self, kw: &str) -> Result<Handle<Type>, ParseError> {
        match kw {
            "texture_1d" => {
                self.expect(&Token::LeftAngle)?;
                let sample_type = self.parse_texture_sample_type_param()?;
                self.expect(&Token::RightAngle)?;
                Ok(self.module.types.append(Type::Texture {
                    dim: ImageDimension::D1,
                    arrayed: false,
                    multisampled: false,
                    sample_type,
                }))
            }
            "texture_2d" => {
                self.expect(&Token::LeftAngle)?;
                let sample_type = self.parse_texture_sample_type_param()?;
                self.expect(&Token::RightAngle)?;
                Ok(self.module.types.append(Type::Texture {
                    dim: ImageDimension::D2,
                    arrayed: false,
                    multisampled: false,
                    sample_type,
                }))
            }
            "texture_2d_array" => {
                self.expect(&Token::LeftAngle)?;
                let sample_type = self.parse_texture_sample_type_param()?;
                self.expect(&Token::RightAngle)?;
                Ok(self.module.types.append(Type::Texture {
                    dim: ImageDimension::D2,
                    arrayed: true,
                    multisampled: false,
                    sample_type,
                }))
            }
            "texture_3d" => {
                self.expect(&Token::LeftAngle)?;
                let sample_type = self.parse_texture_sample_type_param()?;
                self.expect(&Token::RightAngle)?;
                Ok(self.module.types.append(Type::Texture {
                    dim: ImageDimension::D3,
                    arrayed: false,
                    multisampled: false,
                    sample_type,
                }))
            }
            "texture_cube" => {
                self.expect(&Token::LeftAngle)?;
                let sample_type = self.parse_texture_sample_type_param()?;
                self.expect(&Token::RightAngle)?;
                Ok(self.module.types.append(Type::Texture {
                    dim: ImageDimension::Cube,
                    arrayed: false,
                    multisampled: false,
                    sample_type,
                }))
            }
            "texture_cube_array" => {
                self.expect(&Token::LeftAngle)?;
                let sample_type = self.parse_texture_sample_type_param()?;
                self.expect(&Token::RightAngle)?;
                Ok(self.module.types.append(Type::Texture {
                    dim: ImageDimension::Cube,
                    arrayed: true,
                    multisampled: false,
                    sample_type,
                }))
            }
            "texture_multisampled_2d" => {
                self.expect(&Token::LeftAngle)?;
                let sample_type = self.parse_texture_sample_type_param()?;
                self.expect(&Token::RightAngle)?;
                Ok(self.module.types.append(Type::Texture {
                    dim: ImageDimension::D2,
                    arrayed: false,
                    multisampled: true,
                    sample_type,
                }))
            }
            "texture_depth_2d" => Ok(self.module.types.append(Type::DepthTexture {
                dim: ImageDimension::D2,
                arrayed: false,
                multisampled: false,
            })),
            "texture_depth_2d_array" => Ok(self.module.types.append(Type::DepthTexture {
                dim: ImageDimension::D2,
                arrayed: true,
                multisampled: false,
            })),
            "texture_depth_cube" => Ok(self.module.types.append(Type::DepthTexture {
                dim: ImageDimension::Cube,
                arrayed: false,
                multisampled: false,
            })),
            "texture_depth_cube_array" => Ok(self.module.types.append(Type::DepthTexture {
                dim: ImageDimension::Cube,
                arrayed: true,
                multisampled: false,
            })),
            "texture_depth_multisampled_2d" => Ok(self.module.types.append(Type::DepthTexture {
                dim: ImageDimension::D2,
                arrayed: false,
                multisampled: true,
            })),
            "texture_storage_1d" | "texture_storage_2d" | "texture_storage_2d_array" | "texture_storage_3d" => {
                self.expect(&Token::LeftAngle)?;
                let format = self.parse_storage_format_from_ident()?;
                self.expect(&Token::Comma)?;
                let access = self.parse_storage_texture_access()?;
                self.expect(&Token::RightAngle)?;
                let (dim, arrayed) = match kw {
                    "texture_storage_1d" => (ImageDimension::D1, false),
                    "texture_storage_2d" => (ImageDimension::D2, false),
                    "texture_storage_2d_array" => (ImageDimension::D2, true),
                    "texture_storage_3d" => (ImageDimension::D3, false),
                    _ => unreachable!(),
                };
                Ok(self.module.types.append(Type::StorageTexture {
                    dim,
                    arrayed,
                    format,
                    access,
                }))
            }
            other => Err(self.error(format!("unsupported texture type: {other}"))),
        }
    }

    fn parse_vector_type(&mut self, size: VectorSize) -> Result<Handle<Type>, ParseError> {
        self.advance();
        let scalar = if self.eat(&Token::LeftAngle) {
            let s = self.parse_scalar_type()?;
            self.expect(&Token::RightAngle)?;
            s
        } else {
            Scalar::F32
        };
        Ok(self.module.types.append(Type::Vector { scalar, size }))
    }

    fn parse_matrix_type(&mut self, columns: VectorSize, rows: VectorSize) -> Result<Handle<Type>, ParseError> {
        self.advance();
        let scalar = if self.eat(&Token::LeftAngle) {
            let s = self.parse_scalar_type()?;
            self.expect(&Token::RightAngle)?;
            s
        } else {
            Scalar::F32
        };
        Ok(self.module.types.append(Type::Matrix { scalar, columns, rows }))
    }

    fn parse_scalar_type(&mut self) -> Result<Scalar, ParseError> {
        match self.peek() {
            Token::F32 => { self.advance(); Ok(Scalar::F32) }
            Token::F64 => { self.advance(); Ok(Scalar::F64) }
            Token::I32 => { self.advance(); Ok(Scalar::I32) }
            Token::U32 => { self.advance(); Ok(Scalar::U32) }
            other => Err(self.error(format!("expected scalar type, got {other:?}"))),
        }
    }

    fn parse_array_type(&mut self) -> Result<Handle<Type>, ParseError> {
        self.advance(); // 'array'
        self.expect(&Token::LeftAngle)?;
        let elem_ty = self.parse_type()?;
        let size = if self.eat(&Token::Comma) {
            let n = self.parse_const_int()?;
            ArraySize::Constant(n)
        } else {
            ArraySize::Dynamic
        };
        self.expect(&Token::RightAngle)?;
        Ok(self.module.types.append(Type::Array { base: elem_ty, size }))
    }

    fn parse_atomic_type(&mut self) -> Result<Handle<Type>, ParseError> {
        self.advance(); // 'atomic'
        self.expect(&Token::LeftAngle)?;
        let scalar = self.parse_scalar_type()?;
        self.expect(&Token::RightAngle)?;
        Ok(self.module.types.append(Type::Atomic(scalar)))
    }

    fn parse_struct_decl(&mut self) -> Result<Handle<Type>, ParseError> {
        self.advance(); // 'struct'
        let name = self.expect_ident()?;
        self.expect(&Token::LeftBrace)?;
        let mut members = Vec::new();
        while !self.check(&Token::RightBrace) && !self.check(&Token::Eof) {
            let binding = self.parse_member_attributes()?;
            let member_name = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let ty = self.parse_type()?;
            members.push(StructMember {
                name: Some(member_name),
                ty,
                offset: None,
                binding,
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RightBrace)?;
        Ok(self.module.types.append(Type::Struct { name: Some(name), members }))
    }

    fn parse_member_attributes(&mut self) -> Result<Option<Binding>, ParseError> {
        let mut location_num: Option<u32> = None;
        let mut interpolation: Option<Interpolation> = None;
        let mut sampling: Option<Sampling> = None;
        while self.check(&Token::At) {
            self.advance(); // @
            match self.peek().clone() {
                Token::Ident(s) if s == "location" => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    let loc = self.parse_const_int()?;
                    self.expect(&Token::RightParen)?;
                    location_num = Some(loc);
                }
                Token::Ident(s) if s == "builtin" => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    let name = self.expect_ident()?;
                    self.expect(&Token::RightParen)?;
                    let bi = Self::parse_builtin_name(&name)
                        .ok_or_else(|| self.error(format!("unknown builtin: {name}")))?;
                    return Ok(Some(Binding::BuiltIn(bi)));
                }
                Token::Ident(s) if s == "interpolate" => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    let itype = self.expect_ident()?;
                    interpolation = Some(match itype.as_str() {
                        "flat" => Interpolation::Flat,
                        "linear" => Interpolation::Linear,
                        "perspective" => Interpolation::Perspective,
                        other => return Err(self.error(format!("unknown interpolation: {other}"))),
                    });
                    if self.eat(&Token::Comma) {
                        let sname = self.expect_ident()?;
                        sampling = Some(match sname.as_str() {
                            "center" => Sampling::Center,
                            "centroid" => Sampling::Centroid,
                            "sample" => Sampling::Sample,
                            other => return Err(self.error(format!("unknown sampling: {other}"))),
                        });
                    }
                    self.expect(&Token::RightParen)?;
                }
                _ => {
                    self.advance();
                    if self.eat(&Token::LeftParen) {
                        let mut depth = 1;
                        while depth > 0 {
                            match self.peek() {
                                Token::LeftParen => {
                                    self.advance();
                                    depth += 1;
                                }
                                Token::RightParen => {
                                    self.advance();
                                    depth -= 1;
                                }
                                Token::Eof => break,
                                _ => {
                                    self.advance();
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(loc) = location_num {
            Ok(Some(Binding::Location {
                location: loc,
                interpolation,
                sampling,
            }))
        } else {
            Ok(None)
        }
    }

    fn parse_const_int(&mut self) -> Result<u32, ParseError> {
        match self.peek().clone() {
            Token::IntLiteral(n) => { self.advance(); Ok(n as u32) }
            Token::UintLiteral(n) => { self.advance(); Ok(n as u32) }
            other => Err(self.error(format!("expected integer constant, got {other:?}"))),
        }
    }

    // ---- Global declarations ----

    fn parse_global_var(&mut self) -> Result<(), ParseError> {
        let (group, binding) = self.parse_binding_attributes()?;
        self.expect(&Token::Var)?;
        let space = if self.eat(&Token::LeftAngle) {
            let space = self.parse_address_space()?;
            self.expect(&Token::RightAngle)?;
            space
        } else {
            AddressSpace::Private
        };
        let name = self.expect_ident()?;
        self.expect(&Token::Colon)?;
        let ty = self.parse_type()?;
        self.expect(&Token::Semicolon)?;

        let rb = match (group, binding) {
            (Some(g), Some(b)) => Some(ResourceBinding { group: g, binding: b }),
            _ => None,
        };

        self.module.global_variables.push(GlobalVariable {
            name: Some(name),
            space,
            binding: rb,
            ty,
        });
        Ok(())
    }

    fn parse_workgroup_var(&mut self) -> Result<(), ParseError> {
        self.expect(&Token::Var)?;
        self.expect(&Token::LeftAngle)?;
        // expect "workgroup" identifier
        match self.peek().clone() {
            Token::Ident(s) if s == "workgroup" => { self.advance(); }
            other => return Err(self.error(format!("expected 'workgroup', got {other:?}"))),
        }
        self.expect(&Token::RightAngle)?;
        let name = self.expect_ident()?;
        self.expect(&Token::Colon)?;
        let ty = self.parse_type()?;
        self.expect(&Token::Semicolon)?;

        self.module.global_variables.push(GlobalVariable {
            name: Some(name),
            space: AddressSpace::WorkGroup,
            binding: None,
            ty,
        });
        Ok(())
    }

    fn parse_binding_attributes(&mut self) -> Result<(Option<u32>, Option<u32>), ParseError> {
        let mut group = None;
        let mut binding = None;
        while self.check(&Token::At) {
            self.advance();
            match self.peek().clone() {
                Token::Ident(s) if s == "group" => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    group = Some(self.parse_const_int()?);
                    self.expect(&Token::RightParen)?;
                }
                Token::Ident(s) if s == "binding" => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    binding = Some(self.parse_const_int()?);
                    self.expect(&Token::RightParen)?;
                }
                _ => break,
            }
        }
        Ok((group, binding))
    }

    fn parse_address_space(&mut self) -> Result<AddressSpace, ParseError> {
        match self.peek().clone() {
            Token::Ident(s) if s == "storage" => {
                self.advance();
                let access = if self.eat(&Token::Comma) {
                    match self.peek() {
                        Token::Read => { self.advance(); StorageAccess::Load }
                        Token::ReadWrite => { self.advance(); StorageAccess::LoadStore }
                        Token::Write => { self.advance(); StorageAccess::Store }
                        _ => StorageAccess::Load,
                    }
                } else {
                    StorageAccess::Load
                };
                Ok(AddressSpace::Storage { access })
            }
            Token::Ident(s) if s == "uniform" => { self.advance(); Ok(AddressSpace::Uniform) }
            Token::Ident(s) if s == "workgroup" => { self.advance(); Ok(AddressSpace::WorkGroup) }
            Token::Ident(s) if s == "private" => { self.advance(); Ok(AddressSpace::Private) }
            Token::Ident(s) if s == "function" => { self.advance(); Ok(AddressSpace::Function) }
            other => Err(self.error(format!("expected address space, got {other:?}"))),
        }
    }

    // ---- Entry point / function parsing ----

    fn parse_entry_point(&mut self) -> Result<(), ParseError> {
        let mut workgroup_size = [1u32, 1, 1];
        let mut stage = ShaderStage::Compute;

        while self.check(&Token::At) {
            self.advance();
            match self.peek().clone() {
                Token::Compute => { self.advance(); stage = ShaderStage::Compute; }
                Token::Vertex => { self.advance(); stage = ShaderStage::Vertex; }
                Token::Fragment => { self.advance(); stage = ShaderStage::Fragment; }
                Token::Ident(s) if s == "workgroup_size" => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    workgroup_size[0] = self.parse_const_int()?;
                    if self.eat(&Token::Comma) {
                        workgroup_size[1] = self.parse_const_int()?;
                        if self.eat(&Token::Comma) {
                            workgroup_size[2] = self.parse_const_int()?;
                        }
                    }
                    self.expect(&Token::RightParen)?;
                }
                _ => {
                    self.advance();
                    if self.eat(&Token::LeftParen) {
                        while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
                            self.advance();
                        }
                        self.eat(&Token::RightParen);
                    }
                }
            }
        }

        self.expect(&Token::Fn)?;
        let name = self.expect_ident()?;
        let mut function = Function::new();
        function.name = Some(name.clone());

        self.expect(&Token::LeftParen)?;
        while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
            let arg = self.parse_fn_argument(&mut function)?;
            function.arguments.push(arg);
            self.eat(&Token::Comma);
        }
        self.expect(&Token::RightParen)?;

        if self.eat(&Token::Arrow) {
            let ret_ty = self.parse_type()?;
            function.result = Some(ret_ty);
        }

        let body = self.parse_block_body(&mut function)?;
        function.body = body;

        self.module.entry_points.push(EntryPoint {
            name,
            stage,
            workgroup_size,
            function,
        });
        Ok(())
    }

    fn parse_fn_argument(&mut self, func: &mut Function) -> Result<FunctionArgument, ParseError> {
        let mut builtin = None;
        let mut location = None;
        while self.check(&Token::At) {
            self.advance();
            match self.peek().clone() {
                Token::Ident(s) if s == "builtin" => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    builtin = Some(self.parse_builtin()?);
                    self.expect(&Token::RightParen)?;
                }
                Token::Ident(s) if s == "location" => {
                    self.advance();
                    self.expect(&Token::LeftParen)?;
                    location = Some(self.parse_const_int()?);
                    self.expect(&Token::RightParen)?;
                }
                _ => {
                    self.advance();
                    if self.eat(&Token::LeftParen) {
                        while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
                            self.advance();
                        }
                        self.eat(&Token::RightParen);
                    }
                }
            }
        }

        let name = self.expect_ident()?;
        self.expect(&Token::Colon)?;
        let ty = self.parse_type()?;

        let binding = match (builtin, location) {
            (Some(b), _) => Some(Binding::BuiltIn(b)),
            (None, Some(loc)) => Some(Binding::Location {
                location: loc,
                interpolation: None,
                sampling: None,
            }),
            _ => None,
        };

        let arg_ty = match &self.module.types[ty] {
            _ => ty,
        };

        let _ = func;
        Ok(FunctionArgument {
            name: Some(name),
            ty: arg_ty,
            binding,
        })
    }

    fn parse_builtin_name(name: &str) -> Option<BuiltIn> {
        Some(match name {
            "global_invocation_id" => BuiltIn::GlobalInvocationId,
            "local_invocation_id" => BuiltIn::LocalInvocationId,
            "local_invocation_index" => BuiltIn::LocalInvocationIndex,
            "workgroup_id" => BuiltIn::WorkGroupId,
            "num_workgroups" => BuiltIn::NumWorkGroups,
            "vertex_index" => BuiltIn::VertexIndex,
            "instance_index" => BuiltIn::InstanceIndex,
            "position" => BuiltIn::Position,
            "front_facing" => BuiltIn::FrontFacing,
            "frag_depth" => BuiltIn::FragDepth,
            "sample_index" => BuiltIn::SampleIndex,
            "sample_mask" => BuiltIn::SampleMask,
            _ => return None,
        })
    }

    fn parse_builtin(&mut self) -> Result<BuiltIn, ParseError> {
        let name = self.expect_ident()?;
        Self::parse_builtin_name(&name).ok_or_else(|| self.error(format!("unknown builtin: {name}")))
    }

    // ---- Statement parsing ----

    fn parse_block_body(&mut self, func: &mut Function) -> Result<Vec<Statement>, ParseError> {
        self.expect(&Token::LeftBrace)?;
        let mut stmts = Vec::new();
        while !self.check(&Token::RightBrace) && !self.check(&Token::Eof) {
            stmts.push(self.parse_statement(func)?);
        }
        self.expect(&Token::RightBrace)?;
        Ok(stmts)
    }

    fn parse_statement(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        match self.peek().clone() {
            Token::Return => self.parse_return(func),
            Token::If => self.parse_if(func),
            Token::For => self.parse_for(func),
            Token::While => self.parse_while(func),
            Token::Loop => self.parse_loop(func),
            Token::Switch => self.parse_switch(func),
            Token::Break => { self.advance(); self.expect(&Token::Semicolon)?; Ok(Statement::Break) }
            Token::Continue => { self.advance(); self.expect(&Token::Semicolon)?; Ok(Statement::Continue) }
            Token::Let | Token::Const => self.parse_let_decl(func),
            Token::Var => self.parse_var_decl_stmt(func),
            Token::LeftBrace => {
                let body = self.parse_block_body(func)?;
                Ok(Statement::Block(body))
            }
            Token::Underscore => {
                self.advance();
                self.expect(&Token::Equal)?;
                let val = self.parse_expression(func)?;
                self.expect(&Token::Semicolon)?;
                Ok(Statement::Phony { value: val })
            }
            Token::WorkgroupBarrier => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                self.expect(&Token::Semicolon)?;
                Ok(Statement::ControlBarrier(Barrier::WORK_GROUP))
            }
            Token::StorageBarrier => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                self.expect(&Token::Semicolon)?;
                Ok(Statement::ControlBarrier(Barrier::STORAGE))
            }
            Token::Ident(name) if name.as_str() == "textureStore" => {
                self.advance();
                self.parse_texture_store_statement(func)
            }
            _ => self.parse_assignment_or_call(func),
        }
    }

    fn parse_texture_store_statement(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::LeftParen)?;
        let texture = self.parse_expression(func)?;
        self.expect(&Token::Comma)?;
        let coordinate = self.parse_expression(func)?;
        self.expect(&Token::Comma)?;
        let value = self.parse_expression(func)?;
        self.expect(&Token::RightParen)?;
        self.expect(&Token::Semicolon)?;
        Ok(Statement::TextureStore {
            texture,
            coordinate,
            array_index: None,
            value,
        })
    }

    fn parse_return(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // 'return'
        let value = if !self.check(&Token::Semicolon) {
            Some(self.parse_expression(func)?)
        } else {
            None
        };
        self.expect(&Token::Semicolon)?;
        Ok(Statement::Return { value })
    }

    fn parse_if(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // 'if'
        let condition = self.parse_expression(func)?;
        let accept = self.parse_block_body(func)?;
        let reject = if self.eat(&Token::Else) {
            if self.check(&Token::If) {
                vec![self.parse_if(func)?]
            } else {
                self.parse_block_body(func)?
            }
        } else {
            Vec::new()
        };
        Ok(Statement::If { condition, accept, reject })
    }

    fn parse_for(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // 'for'
        self.expect(&Token::LeftParen)?;

        let init = if !self.check(&Token::Semicolon) {
            Some(Box::new(self.parse_for_init(func)?))
        } else {
            None
        };
        self.expect(&Token::Semicolon)?;

        let condition = if !self.check(&Token::Semicolon) {
            Some(self.parse_expression(func)?)
        } else {
            None
        };
        self.expect(&Token::Semicolon)?;

        let update = if !self.check(&Token::RightParen) {
            Some(Box::new(self.parse_for_update(func)?))
        } else {
            None
        };
        self.expect(&Token::RightParen)?;

        let body = self.parse_block_body(func)?;

        Ok(Statement::ForLoop { init, condition, update, body })
    }

    fn parse_for_init(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        match self.peek().clone() {
            Token::Var => self.parse_var_decl_stmt_no_semi(func),
            Token::Let | Token::Const => self.parse_let_decl_no_semi(func),
            _ => self.parse_assignment_or_call_no_semi(func),
        }
    }

    fn parse_for_update(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.parse_assignment_or_call_no_semi(func)
    }

    fn parse_while(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // 'while'
        let condition = self.parse_expression(func)?;
        let body = self.parse_block_body(func)?;
        Ok(Statement::WhileLoop { condition, body })
    }

    fn parse_loop(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // 'loop'
        self.expect(&Token::LeftBrace)?;
        let mut body = Vec::new();
        let mut continuing = Vec::new();
        let mut break_if = None;
        while !self.check(&Token::RightBrace) && !self.check(&Token::Eof) {
            if self.check(&Token::Ident(String::new())) || {
                matches!(self.peek(), Token::Ident(s) if s == "continuing")
            } {
                if let Token::Ident(s) = self.peek() {
                    if s == "continuing" {
                        self.advance();
                        self.expect(&Token::LeftBrace)?;
                        while !self.check(&Token::RightBrace) && !self.check(&Token::Eof) {
                            if matches!(self.peek(), Token::Ident(s) if s == "break_if") || self.check(&Token::Break) {
                                if matches!(self.peek(), Token::Ident(s) if s == "break_if") {
                                    self.advance();
                                } else {
                                    self.advance(); // break
                                    self.expect(&Token::If)?;
                                }
                                break_if = Some(self.parse_expression(func)?);
                                self.expect(&Token::Semicolon)?;
                            } else {
                                continuing.push(self.parse_statement(func)?);
                            }
                        }
                        self.expect(&Token::RightBrace)?;
                        continue;
                    }
                }
                body.push(self.parse_statement(func)?);
            } else {
                body.push(self.parse_statement(func)?);
            }
        }
        self.expect(&Token::RightBrace)?;
        Ok(Statement::Loop { body, continuing, break_if })
    }

    fn parse_switch(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // 'switch'
        let selector = self.parse_expression(func)?;
        self.expect(&Token::LeftBrace)?;
        let mut cases = Vec::new();
        while !self.check(&Token::RightBrace) && !self.check(&Token::Eof) {
            if self.eat(&Token::Case) {
                let value = match self.peek().clone() {
                    Token::IntLiteral(n) => { self.advance(); SwitchValue::I32(n as i32) }
                    Token::UintLiteral(n) => { self.advance(); SwitchValue::U32(n as u32) }
                    _ => return Err(self.error("expected case value".into())),
                };
                self.expect(&Token::Colon)?;
                let body = if self.check(&Token::LeftBrace) {
                    self.parse_block_body(func)?
                } else {
                    let mut stmts = Vec::new();
                    while !self.check(&Token::Case) && !self.check(&Token::Default) && !self.check(&Token::RightBrace) && !self.check(&Token::Eof) {
                        stmts.push(self.parse_statement(func)?);
                    }
                    stmts
                };
                cases.push(SwitchCase { value, body, fall_through: false });
            } else if self.eat(&Token::Default) {
                {
                    self.eat(&Token::Colon);
                    let body = if self.check(&Token::LeftBrace) {
                        self.parse_block_body(func)?
                    } else {
                        let mut stmts = Vec::new();
                        while !self.check(&Token::Case) && !self.check(&Token::RightBrace) && !self.check(&Token::Eof) {
                            stmts.push(self.parse_statement(func)?);
                        }
                        stmts
                    };
                    cases.push(SwitchCase { value: SwitchValue::Default, body, fall_through: false });
                }
            } else {
                return Err(self.error("expected 'case' or 'default'".into()));
            }
        }
        self.expect(&Token::RightBrace)?;
        Ok(Statement::Switch { selector, cases })
    }

    fn parse_let_decl(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        let stmt = self.parse_let_decl_no_semi(func)?;
        self.expect(&Token::Semicolon)?;
        Ok(stmt)
    }

    fn parse_let_decl_no_semi(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // 'let' or 'const'
        let name = self.expect_ident()?;
        let ty = if self.eat(&Token::Colon) {
            self.parse_type()?
        } else {
            self.module.types.append(Type::Scalar(Scalar::F32))
        };
        self.expect(&Token::Equal)?;
        let init_expr = self.parse_expression(func)?;

        let lv_index = func.local_variables.len() as u32;
        func.local_variables.push(LocalVariable {
            name: Some(name),
            ty,
            init: Some(init_expr),
        });
        Ok(Statement::LocalDecl { local_var_index: lv_index })
    }

    fn parse_var_decl_stmt(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        let stmt = self.parse_var_decl_stmt_no_semi(func)?;
        self.expect(&Token::Semicolon)?;
        Ok(stmt)
    }

    fn parse_var_decl_stmt_no_semi(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // 'var'
        let name = self.expect_ident()?;
        let ty = if self.eat(&Token::Colon) {
            self.parse_type()?
        } else {
            self.module.types.append(Type::Scalar(Scalar::F32))
        };
        let init = if self.eat(&Token::Equal) {
            Some(self.parse_expression(func)?)
        } else {
            None
        };

        let lv_index = func.local_variables.len() as u32;
        func.local_variables.push(LocalVariable {
            name: Some(name),
            ty,
            init,
        });
        Ok(Statement::LocalDecl { local_var_index: lv_index })
    }

    fn parse_assignment_or_call(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        let stmt = self.parse_assignment_or_call_no_semi(func)?;
        self.expect(&Token::Semicolon)?;
        Ok(stmt)
    }

    fn parse_assignment_or_call_no_semi(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        let lhs = self.parse_expression(func)?;

        match self.peek().clone() {
            Token::Equal => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::Store { pointer: lhs, value })
            }
            Token::PlusEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign { pointer: lhs, op: BinaryOp::Add, value })
            }
            Token::MinusEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign { pointer: lhs, op: BinaryOp::Subtract, value })
            }
            Token::StarEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign { pointer: lhs, op: BinaryOp::Multiply, value })
            }
            Token::SlashEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign { pointer: lhs, op: BinaryOp::Divide, value })
            }
            Token::PercentEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign { pointer: lhs, op: BinaryOp::Modulo, value })
            }
            Token::AmpEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign { pointer: lhs, op: BinaryOp::BitwiseAnd, value })
            }
            Token::PipeEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign { pointer: lhs, op: BinaryOp::BitwiseOr, value })
            }
            Token::CaretEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign { pointer: lhs, op: BinaryOp::BitwiseXor, value })
            }
            Token::PlusPlus => {
                self.advance();
                Ok(Statement::Increment { pointer: lhs })
            }
            Token::MinusMinus => {
                self.advance();
                Ok(Statement::Decrement { pointer: lhs })
            }
            _ => {
                Ok(Statement::Phony { value: lhs })
            }
        }
    }

    // ---- Expression parsing (precedence climbing) ----

    fn parse_expression(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        self.parse_or_expr(func)
    }

    fn parse_or_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_and_expr(func)?;
        while self.check(&Token::PipePipe) {
            self.advance();
            let right = self.parse_and_expr(func)?;
            left = func.expressions.append(Expression::Binary {
                op: BinaryOp::Or,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_bitor_expr(func)?;
        while self.check(&Token::AmpersandAmpersand) {
            self.advance();
            let right = self.parse_bitor_expr(func)?;
            left = func.expressions.append(Expression::Binary {
                op: BinaryOp::And,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_bitor_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_bitxor_expr(func)?;
        while self.check(&Token::Pipe) {
            self.advance();
            let right = self.parse_bitxor_expr(func)?;
            left = func.expressions.append(Expression::Binary {
                op: BinaryOp::BitwiseOr,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_bitxor_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_bitand_expr(func)?;
        while self.check(&Token::Caret) {
            self.advance();
            let right = self.parse_bitand_expr(func)?;
            left = func.expressions.append(Expression::Binary {
                op: BinaryOp::BitwiseXor,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_bitand_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_equality_expr(func)?;
        while self.check(&Token::Ampersand) {
            self.advance();
            let right = self.parse_equality_expr(func)?;
            left = func.expressions.append(Expression::Binary {
                op: BinaryOp::BitwiseAnd,
                left,
                right,
            });
        }
        Ok(left)
    }

    fn parse_equality_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_relational_expr(func)?;
        loop {
            let op = match self.peek() {
                Token::EqualEqual => BinaryOp::Equal,
                Token::BangEqual => BinaryOp::NotEqual,
                _ => break,
            };
            self.advance();
            let right = self.parse_relational_expr(func)?;
            left = func.expressions.append(Expression::Binary { op, left, right });
        }
        Ok(left)
    }

    fn parse_relational_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_shift_expr(func)?;
        loop {
            let op = match self.peek() {
                Token::LeftAngle => BinaryOp::Less,
                Token::RightAngle => BinaryOp::Greater,
                Token::LessEqual => BinaryOp::LessEqual,
                Token::GreaterEqual => BinaryOp::GreaterEqual,
                _ => break,
            };
            self.advance();
            let right = self.parse_shift_expr(func)?;
            left = func.expressions.append(Expression::Binary { op, left, right });
        }
        Ok(left)
    }

    fn parse_shift_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_additive_expr(func)?;
        loop {
            let op = match self.peek() {
                Token::ShiftLeft => BinaryOp::ShiftLeft,
                Token::ShiftRight => BinaryOp::ShiftRight,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive_expr(func)?;
            left = func.expressions.append(Expression::Binary { op, left, right });
        }
        Ok(left)
    }

    fn parse_additive_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_multiplicative_expr(func)?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Subtract,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative_expr(func)?;
            left = func.expressions.append(Expression::Binary { op, left, right });
        }
        Ok(left)
    }

    fn parse_multiplicative_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_unary_expr(func)?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOp::Multiply,
                Token::Slash => BinaryOp::Divide,
                Token::Percent => BinaryOp::Modulo,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary_expr(func)?;
            left = func.expressions.append(Expression::Binary { op, left, right });
        }
        Ok(left)
    }

    fn parse_unary_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        match self.peek().clone() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary_expr(func)?;
                Ok(func.expressions.append(Expression::Unary { op: UnaryOp::Negate, expr }))
            }
            Token::Bang => {
                self.advance();
                let expr = self.parse_unary_expr(func)?;
                Ok(func.expressions.append(Expression::Unary { op: UnaryOp::LogicalNot, expr }))
            }
            Token::Tilde => {
                self.advance();
                let expr = self.parse_unary_expr(func)?;
                Ok(func.expressions.append(Expression::Unary { op: UnaryOp::BitwiseNot, expr }))
            }
            _ => self.parse_postfix_expr(func),
        }
    }

    fn parse_postfix_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut expr = self.parse_primary_expr(func)?;
        loop {
            match self.peek().clone() {
                Token::LeftBracket => {
                    self.advance();
                    let index = self.parse_expression(func)?;
                    self.expect(&Token::RightBracket)?;
                    expr = func.expressions.append(Expression::Access { base: expr, index });
                }
                Token::Period => {
                    self.advance();
                    match self.peek().clone() {
                        Token::Ident(field) => {
                            let field = field.clone();
                            self.advance();
                            let idx = self.resolve_field_index(expr, &field, func);
                            match idx {
                                Some(i) => {
                                    expr = func.expressions.append(Expression::AccessIndex { base: expr, index: i });
                                }
                                None => {
                                    let swizzle = self.parse_swizzle(&field)?;
                                    let size = match swizzle.len() {
                                        2 => VectorSize::Bi,
                                        3 => VectorSize::Tri,
                                        4 => VectorSize::Quad,
                                        1 => {
                                            expr = func.expressions.append(Expression::AccessIndex { base: expr, index: swizzle[0] });
                                            continue;
                                        }
                                        _ => return Err(self.error("invalid swizzle".into())),
                                    };
                                    let mut pattern = [0u32; 4];
                                    for (i, &s) in swizzle.iter().enumerate() {
                                        pattern[i] = s;
                                    }
                                    expr = func.expressions.append(Expression::Swizzle { vector: expr, pattern, size });
                                }
                            }
                        }
                        _ => return Err(self.error("expected field name after '.'".into())),
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn resolve_field_index(&self, base: Handle<Expression>, field: &str, _func: &Function) -> Option<u32> {
        let ty_handle = self.expr_types.get(&base.index())?;
        let ty = &self.module.types[*ty_handle];
        if let Type::Struct { members, .. } = ty {
            for (i, member) in members.iter().enumerate() {
                if member.name.as_deref() == Some(field) {
                    return Some(i as u32);
                }
            }
        }
        None
    }

    fn parse_swizzle(&self, text: &str) -> Result<Vec<u32>, ParseError> {
        let mut indices = Vec::new();
        for ch in text.chars() {
            let idx = match ch {
                'x' | 'r' => 0,
                'y' | 'g' => 1,
                'z' | 'b' => 2,
                'w' | 'a' => 3,
                _ => return Err(ParseError::Syntax {
                    offset: 0,
                    message: format!("invalid swizzle character: {ch}"),
                }),
            };
            indices.push(idx);
        }
        Ok(indices)
    }

    fn parse_primary_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        match self.peek().clone() {
            Token::IntLiteral(n) => {
                self.advance();
                Ok(func.expressions.append(Expression::Literal(Literal::I32(n as i32))))
            }
            Token::UintLiteral(n) => {
                self.advance();
                Ok(func.expressions.append(Expression::Literal(Literal::U32(n as u32))))
            }
            Token::FloatLiteral(f) => {
                self.advance();
                Ok(func.expressions.append(Expression::Literal(Literal::F32(f as f32))))
            }
            Token::BoolLiteral(b) => {
                self.advance();
                Ok(func.expressions.append(Expression::Literal(Literal::Bool(b))))
            }
            Token::LeftParen => {
                self.advance();
                let e = self.parse_expression(func)?;
                self.expect(&Token::RightParen)?;
                Ok(e)
            }
            Token::Vec2 | Token::Vec3 | Token::Vec4 => self.parse_type_constructor(func),
            Token::F32 | Token::F64 | Token::I32 | Token::U32 => self.parse_scalar_constructor(func),
            Token::Array => self.parse_array_constructor(func),
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                if self.check(&Token::LeftParen) {
                    self.parse_function_call(&name, func)
                } else {
                    self.resolve_name(&name, func)
                }
            }
            other => Err(self.error(format!("expected expression, got {other:?}"))),
        }
    }

    fn parse_type_constructor(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let ty = self.parse_type()?;
        self.expect(&Token::LeftParen)?;
        let mut args = Vec::new();
        while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
            args.push(self.parse_expression(func)?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RightParen)?;

        if args.len() == 1 {
            let size = match &self.module.types[ty] {
                Type::Vector { size, .. } => Some(*size),
                _ => None,
            };
            if let Some(size) = size {
                return Ok(func.expressions.append(Expression::Splat { size, value: args[0] }));
            }
        }

        Ok(func.expressions.append(Expression::Compose { ty, components: args }))
    }

    fn parse_scalar_constructor(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let kind = match self.peek() {
            Token::F32 => ScalarKind::Float,
            Token::F64 => ScalarKind::Float,
            Token::I32 => ScalarKind::Sint,
            Token::U32 => ScalarKind::Uint,
            _ => return Err(self.error("expected scalar type".into())),
        };
        let width = match self.peek() {
            Token::F64 => 8,
            _ => 4,
        };
        self.advance();
        self.expect(&Token::LeftParen)?;
        let arg = self.parse_expression(func)?;
        self.expect(&Token::RightParen)?;
        Ok(func.expressions.append(Expression::As { expr: arg, kind, convert: Some(width) }))
    }

    fn parse_array_constructor(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let ty = self.parse_type()?;
        self.expect(&Token::LeftParen)?;
        let mut args = Vec::new();
        while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
            args.push(self.parse_expression(func)?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RightParen)?;
        Ok(func.expressions.append(Expression::Compose { ty, components: args }))
    }

    fn parse_function_call(&mut self, name: &str, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        self.expect(&Token::LeftParen)?;
        let mut args = Vec::new();
        while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
            args.push(self.parse_expression(func)?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RightParen)?;

        if let Some(math_fn) = self.resolve_math_builtin(name) {
            let arg = args.first().copied().ok_or_else(|| self.error(format!("math function {name} requires at least 1 argument")))?;
            let arg1 = args.get(1).copied();
            let arg2 = args.get(2).copied();
            return Ok(func.expressions.append(Expression::Math { fun: math_fn, arg, arg1, arg2 }));
        }

        if name == "select" && args.len() == 3 {
            return Ok(func.expressions.append(Expression::Select {
                reject: args[0],
                accept: args[1],
                condition: args[2],
            }));
        }

        if name == "arrayLength" && args.len() == 1 {
            return Ok(func.expressions.append(Expression::ArrayLength(args[0])));
        }

        if name == "bitcast" && args.len() == 1 {
            // bitcast<T>(expr) — bit-preserving reinterpretation; convert=None means no conversion
            return Ok(func.expressions.append(Expression::As {
                expr: args[0],
                kind: ScalarKind::Uint,
                convert: None,
            }));
        }

        if name == "textureStore" {
            return Err(self.error(
                "textureStore must be used as a statement, not as an expression".into(),
            ));
        }

        if name == "textureSample" {
            if args.len() < 3 {
                return Err(self.error("textureSample requires at least 3 arguments".into()));
            }
            return Ok(func.expressions.append(Expression::TextureSample {
                texture: args[0],
                sampler: args[1],
                coordinate: args[2],
                array_index: None,
                offset: args.get(3).copied(),
            }));
        }

        if name == "textureSampleLevel" {
            if args.len() < 4 {
                return Err(self.error("textureSampleLevel requires at least 4 arguments".into()));
            }
            return Ok(func.expressions.append(Expression::TextureSampleLevel {
                texture: args[0],
                sampler: args[1],
                coordinate: args[2],
                level: args[3],
                array_index: None,
                offset: args.get(4).copied(),
            }));
        }

        if name == "textureSampleBias" {
            if args.len() < 4 {
                return Err(self.error("textureSampleBias requires at least 4 arguments".into()));
            }
            return Ok(func.expressions.append(Expression::TextureSampleBias {
                texture: args[0],
                sampler: args[1],
                coordinate: args[2],
                bias: args[3],
                array_index: None,
                offset: args.get(4).copied(),
            }));
        }

        if name == "textureSampleCompare" {
            if args.len() < 4 {
                return Err(self.error("textureSampleCompare requires at least 4 arguments".into()));
            }
            return Ok(func.expressions.append(Expression::TextureSampleCompare {
                texture: args[0],
                sampler: args[1],
                coordinate: args[2],
                depth_ref: args[3],
                array_index: None,
                offset: args.get(4).copied(),
            }));
        }

        if name == "textureLoad" {
            if args.len() < 3 {
                return Err(self.error("textureLoad requires at least 3 arguments".into()));
            }
            return Ok(func.expressions.append(Expression::TextureLoad {
                texture: args[0],
                coordinate: args[1],
                array_index: None,
                level: Some(args[2]),
                sample_index: None,
            }));
        }

        if name == "textureDimensions" {
            if args.is_empty() {
                return Err(self.error("textureDimensions requires at least 1 argument".into()));
            }
            return Ok(func.expressions.append(Expression::TextureDimensions {
                texture: args[0],
                level: args.get(1).copied(),
            }));
        }

        if name == "textureNumLayers" {
            if args.len() != 1 {
                return Err(self.error("textureNumLayers requires exactly 1 argument".into()));
            }
            return Ok(func.expressions.append(Expression::TextureNumLayers { texture: args[0] }));
        }

        if name == "textureNumLevels" {
            if args.len() != 1 {
                return Err(self.error("textureNumLevels requires exactly 1 argument".into()));
            }
            return Ok(func.expressions.append(Expression::TextureNumLevels { texture: args[0] }));
        }

        if name == "textureNumSamples" {
            if args.len() != 1 {
                return Err(self.error("textureNumSamples requires exactly 1 argument".into()));
            }
            return Ok(func.expressions.append(Expression::TextureNumSamples { texture: args[0] }));
        }

        Err(self.error(format!("unknown function: {name}")))
    }

    fn resolve_math_builtin(&self, name: &str) -> Option<MathFunction> {
        Some(match name {
            "abs" => MathFunction::Abs,
            "min" => MathFunction::Min,
            "max" => MathFunction::Max,
            "clamp" => MathFunction::Clamp,
            "floor" => MathFunction::Floor,
            "ceil" => MathFunction::Ceil,
            "round" => MathFunction::Round,
            "trunc" => MathFunction::Trunc,
            "fract" => MathFunction::Fract,
            "sqrt" => MathFunction::Sqrt,
            "inverseSqrt" => MathFunction::InverseSqrt,
            "sin" => MathFunction::Sin,
            "cos" => MathFunction::Cos,
            "tan" => MathFunction::Tan,
            "asin" => MathFunction::Asin,
            "acos" => MathFunction::Acos,
            "atan" => MathFunction::Atan,
            "atan2" => MathFunction::Atan2,
            "exp" => MathFunction::Exp,
            "exp2" => MathFunction::Exp2,
            "log" => MathFunction::Log,
            "log2" => MathFunction::Log2,
            "pow" => MathFunction::Pow,
            "dot" => MathFunction::Dot,
            "cross" => MathFunction::Cross,
            "normalize" => MathFunction::Normalize,
            "length" => MathFunction::Length,
            "distance" => MathFunction::Distance,
            "fma" => MathFunction::Fma,
            "mix" => MathFunction::Mix,
            "step" => MathFunction::Step,
            "smoothstep" => MathFunction::SmoothStep,
            "sign" => MathFunction::Sign,
            "countOneBits" => MathFunction::CountOneBits,
            "reverseBits" => MathFunction::ReverseBits,
            "firstLeadingBit" => MathFunction::FirstLeadingBit,
            "firstTrailingBit" => MathFunction::FirstTrailingBit,
            "extractBits" => MathFunction::ExtractBits,
            "insertBits" => MathFunction::InsertBits,
            _ => return None,
        })
    }

    fn resolve_name(&self, name: &str, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        for (i, lv) in func.local_variables.iter().enumerate().rev() {
            if lv.name.as_deref() == Some(name) {
                return Ok(func.expressions.append(Expression::LocalVariable(i as u32)));
            }
        }

        for (i, arg) in func.arguments.iter().enumerate() {
            if arg.name.as_deref() == Some(name) {
                return Ok(func.expressions.append(Expression::FunctionArgument(i as u32)));
            }
        }

        for (i, gv) in self.module.global_variables.iter().enumerate() {
            if gv.name.as_deref() == Some(name) {
                return Ok(func.expressions.append(Expression::GlobalVariable(i as u32)));
            }
        }

        Err(self.error(format!("undefined variable: {name}")))
    }

    // ---- Top-level module parsing ----

    fn parse_module(&mut self) -> Result<(), ParseError> {
        while !self.check(&Token::Eof) {
            match self.peek().clone() {
                Token::Struct => {
                    let _ = self.parse_struct_decl()?;
                }
                Token::At => {
                    let saved = self.pos;
                    let mut has_binding = false;
                    let mut is_entry = false;
                    let probe_pos = self.pos;
                    while self.check(&Token::At) {
                        self.advance();
                        match self.peek().clone() {
                            Token::Ident(s) if s == "group" || s == "binding" => {
                                has_binding = true;
                                self.advance();
                                if self.eat(&Token::LeftParen) {
                                    while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
                                        self.advance();
                                    }
                                    self.eat(&Token::RightParen);
                                }
                            }
                            Token::Compute | Token::Vertex | Token::Fragment => {
                                is_entry = true;
                                self.advance();
                            }
                            Token::Ident(s) if s == "workgroup_size" => {
                                is_entry = true;
                                self.advance();
                                if self.eat(&Token::LeftParen) {
                                    while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
                                        self.advance();
                                    }
                                    self.eat(&Token::RightParen);
                                }
                            }
                            _ => {
                                self.advance();
                                if self.eat(&Token::LeftParen) {
                                    while !self.check(&Token::RightParen) && !self.check(&Token::Eof) {
                                        self.advance();
                                    }
                                    self.eat(&Token::RightParen);
                                }
                            }
                        }
                    }
                    self.pos = saved;
                    let _ = probe_pos;

                    if is_entry || self.peek_is_fn_after_attrs() {
                        self.parse_entry_point()?;
                    } else if has_binding {
                        self.parse_global_var()?;
                    } else {
                        self.parse_entry_point()?;
                    }
                }
                Token::Var => {
                    let saved = self.pos;
                    self.advance();
                    if self.eat(&Token::LeftAngle) {
                        let is_wg = matches!(self.peek(), Token::Ident(s) if s == "workgroup");
                        self.pos = saved;
                        if is_wg {
                            self.parse_workgroup_var()?;
                        } else {
                            self.parse_global_var()?;
                        }
                    } else {
                        self.pos = saved;
                        self.parse_global_var()?;
                    }
                }
                Token::Fn => {
                    self.parse_entry_point()?;
                }
                _ => {
                    return Err(self.error(format!("unexpected token at module level: {:?}", self.peek())));
                }
            }
        }
        Ok(())
    }

    fn peek_is_fn_after_attrs(&self) -> bool {
        let mut pos = self.pos;
        while pos < self.tokens.len() {
            match &self.tokens[pos].value {
                Token::At => { pos += 1; }
                Token::Fn => return true,
                Token::Ident(_) | Token::Compute | Token::Vertex | Token::Fragment => { pos += 1; }
                Token::LeftParen => {
                    pos += 1;
                    let mut depth = 1u32;
                    while pos < self.tokens.len() && depth > 0 {
                        match &self.tokens[pos].value {
                            Token::LeftParen => depth += 1,
                            Token::RightParen => depth -= 1,
                            _ => {}
                        }
                        pos += 1;
                    }
                }
                _ => return false,
            }
        }
        false
    }
}

/// Parse WGSL source text into a sovereign AST [`Module`].
///
/// # Errors
///
/// Returns [`ParseError`] if the source contains syntax errors or
/// unsupported constructs.
pub fn parse(source: &str) -> Result<Module, ParseError> {
    let mut parser = Parser::new(source);
    parser.parse_module()?;
    Ok(parser.module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_elementwise_add() {
        let src = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = a[gid.x] + b[gid.x];
}
"#;
        let module = parse(src).expect("parse failed");
        assert_eq!(module.global_variables.len(), 3);
        assert_eq!(module.entry_points.len(), 1);
        assert_eq!(module.entry_points[0].name, "main");
        assert_eq!(module.entry_points[0].workgroup_size, [1, 1, 1]);
    }

    #[test]
    fn parse_layer_norm() {
        let src = r#"
var<workgroup> smem: array<f32, 4>;
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(4)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let val = data[lid.x];
    smem[lid.x] = val;
    workgroupBarrier();
    if lid.x < 2u { smem[lid.x] = smem[lid.x] + smem[lid.x + 2u]; }
    workgroupBarrier();
    if lid.x == 0u { smem[0u] = smem[0u] + smem[1u]; }
    workgroupBarrier();
    let mean = smem[0u] / 4.0;
    let diff = val - mean;
    smem[lid.x] = diff * diff;
    workgroupBarrier();
    if lid.x < 2u { smem[lid.x] = smem[lid.x] + smem[lid.x + 2u]; }
    workgroupBarrier();
    if lid.x == 0u { smem[0u] = smem[0u] + smem[1u]; }
    workgroupBarrier();
    let variance = smem[0u] / 4.0;
    let std_dev = sqrt(variance + 1e-5);
    data[lid.x] = (val - mean) / std_dev;
}
"#;
        let module = parse(src).expect("parse failed");
        assert_eq!(module.global_variables.len(), 2);
        assert_eq!(module.entry_points.len(), 1);
        assert_eq!(module.entry_points[0].workgroup_size, [4, 1, 1]);
    }

    #[test]
    fn parse_tiled_matmul() {
        let src = r#"
var<workgroup> tile_a: array<f32, 4>;
var<workgroup> tile_b: array<f32, 4>;

@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;

@compute @workgroup_size(2, 2)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let row = lid.y;
    let col = lid.x;
    let linear = row * 2u + col;
    tile_a[linear] = a[linear];
    tile_b[linear] = b[linear];
    workgroupBarrier();
    var sum = 0.0;
    for (var k = 0u; k < 2u; k = k + 1u) {
        sum = sum + tile_a[row * 2u + k] * tile_b[k * 2u + col];
    }
    c[linear] = sum;
}
"#;
        let module = parse(src).expect("parse failed");
        assert_eq!(module.global_variables.len(), 5);
        assert_eq!(module.entry_points.len(), 1);
        assert_eq!(module.entry_points[0].workgroup_size, [2, 2, 1]);
    }

    #[test]
    fn parse_relu() {
        let src = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = max(input[gid.x], 0.0);
}
"#;
        let module = parse(src).expect("parse failed");
        assert_eq!(module.entry_points.len(), 1);
    }

    #[test]
    fn parse_sigmoid() {
        let src = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    output[gid.x] = 1.0 / (1.0 + exp(-x));
}
"#;
        let module = parse(src).expect("parse failed");
        assert_eq!(module.entry_points.len(), 1);
    }
}
