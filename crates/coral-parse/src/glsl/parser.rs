// SPDX-License-Identifier: AGPL-3.0-only
//! GLSL 450/460 recursive descent parser — compute subset → sovereign AST.

use super::lexer::{Lexer, Span, Spanned, Token};
use crate::ast::*;
use crate::error::ParseError;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
struct LayoutQualifiers {
    local_size: [Option<u32>; 3],
    set: Option<u32>,
    binding: Option<u32>,
    location: Option<u32>,
}

struct Parser<'a> {
    tokens: Vec<Spanned<Token>>,
    pos: usize,
    source: &'a str,
    module: Module,
    workgroup_size: [u32; 3],
    scalar_cache: HashMap<Scalar, Handle<Type>>,
    bool_ty: Option<Handle<Type>>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        let tokens = Lexer::tokenize(source);
        Self {
            tokens,
            pos: 0,
            source,
            module: Module::new(),
            workgroup_size: [1, 1, 1],
            scalar_cache: HashMap::new(),
            bool_ty: None,
        }
    }

    fn bool_type(&mut self) -> Handle<Type> {
        if let Some(h) = self.bool_ty {
            return h;
        }
        let h = self.module.types.append(Type::Bool);
        self.bool_ty = Some(h);
        h
    }

    fn scalar_handle(&mut self, s: Scalar) -> Handle<Type> {
        if let Some(&h) = self.scalar_cache.get(&s) {
            return h;
        }
        let h = self.module.types.append(Type::Scalar(s));
        self.scalar_cache.insert(s, h);
        h
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).map_or(&Token::Eof, |t| &t.value)
    }

    fn peek_span(&self) -> Span {
        self.tokens.get(self.pos).map_or(
            Span {
                start: self.source.len() as u32,
                end: self.source.len() as u32,
            },
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

    fn error(&self, msg: impl Into<String>) -> ParseError {
        let span = self.peek_span();
        ParseError::Syntax {
            offset: span.start,
            message: msg.into(),
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            Err(self.error(format!("expected {expected:?}, got {:?}", self.peek())))
        }
    }

    fn eat(&mut self, tok: &Token) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(self.error(format!("expected identifier, got {other:?}"))),
        }
    }

    fn parse_const_uint(&mut self) -> Result<u32, ParseError> {
        match self.peek().clone() {
            Token::IntLiteral(n) => {
                self.advance();
                Ok(n as u32)
            }
            Token::UintLiteral(n) => {
                self.advance();
                Ok(n as u32)
            }
            other => Err(self.error(format!("expected integer constant, got {other:?}"))),
        }
    }

    fn glsl_builtin_function_arguments(&mut self) -> Vec<FunctionArgument> {
        let u32_ty = self.module.types.append(Type::Scalar(Scalar::U32));
        let uvec3_ty = self.module.types.append(Type::Vector {
            scalar: Scalar::U32,
            size: VectorSize::Tri,
        });
        vec![
            FunctionArgument {
                name: Some("gl_GlobalInvocationID".into()),
                ty: uvec3_ty,
                binding: Some(Binding::BuiltIn(BuiltIn::GlobalInvocationId)),
            },
            FunctionArgument {
                name: Some("gl_LocalInvocationID".into()),
                ty: uvec3_ty,
                binding: Some(Binding::BuiltIn(BuiltIn::LocalInvocationId)),
            },
            FunctionArgument {
                name: Some("gl_WorkGroupID".into()),
                ty: uvec3_ty,
                binding: Some(Binding::BuiltIn(BuiltIn::WorkGroupId)),
            },
            FunctionArgument {
                name: Some("gl_NumWorkGroups".into()),
                ty: uvec3_ty,
                binding: Some(Binding::BuiltIn(BuiltIn::NumWorkGroups)),
            },
            FunctionArgument {
                name: Some("gl_LocalInvocationIndex".into()),
                ty: u32_ty,
                binding: Some(Binding::BuiltIn(BuiltIn::LocalInvocationIndex)),
            },
            FunctionArgument {
                name: Some("gl_WorkGroupSize".into()),
                ty: uvec3_ty,
                binding: Some(Binding::BuiltIn(BuiltIn::WorkGroupSize)),
            },
        ]
    }

    fn parse_scalar_kind(&mut self) -> Result<Scalar, ParseError> {
        match self.peek().clone() {
            Token::Float => {
                self.advance();
                Ok(Scalar::F32)
            }
            Token::Double => {
                self.advance();
                Ok(Scalar::F64)
            }
            Token::Int => {
                self.advance();
                Ok(Scalar::I32)
            }
            Token::Uint => {
                self.advance();
                Ok(Scalar::U32)
            }
            Token::Bool => {
                self.advance();
                Ok(Scalar::BOOL)
            }
            other => Err(self.error(format!("expected scalar type, got {other:?}"))),
        }
    }

    fn parse_vector_size_from_token(vec_tok: &Token) -> Option<VectorSize> {
        Some(match vec_tok {
            Token::Vec2 | Token::Ivec2 | Token::Uvec2 => VectorSize::Bi,
            Token::Vec3 | Token::Ivec3 | Token::Uvec3 => VectorSize::Tri,
            Token::Vec4 | Token::Ivec4 | Token::Uvec4 => VectorSize::Quad,
            _ => return None,
        })
    }

    fn parse_type_inner(&mut self) -> Result<Handle<Type>, ParseError> {
        match self.peek().clone() {
            Token::Void => Err(self.error("void is not a valid value type")),
            Token::Bool => {
                self.advance();
                Ok(self.module.types.append(Type::Bool))
            }
            Token::Float | Token::Int | Token::Uint | Token::Double => {
                let s = self.parse_scalar_kind()?;
                Ok(self.module.types.append(Type::Scalar(s)))
            }
            Token::Vec2 | Token::Vec3 | Token::Vec4 => {
                let t = self.advance().value.clone();
                let size = Self::parse_vector_size_from_token(&t)
                    .ok_or_else(|| self.error("expected vector type"))?;
                let scalar = Scalar::F32;
                Ok(self.module.types.append(Type::Vector { scalar, size }))
            }
            Token::Ivec2 | Token::Ivec3 | Token::Ivec4 => {
                let t = self.advance().value.clone();
                let size = Self::parse_vector_size_from_token(&t)
                    .ok_or_else(|| self.error("expected ivec type"))?;
                Ok(self
                    .module
                    .types
                    .append(Type::Vector { scalar: Scalar::I32, size }))
            }
            Token::Uvec2 | Token::Uvec3 | Token::Uvec4 => {
                let t = self.advance().value.clone();
                let size = Self::parse_vector_size_from_token(&t)
                    .ok_or_else(|| self.error("expected uvec type"))?;
                Ok(self
                    .module
                    .types
                    .append(Type::Vector { scalar: Scalar::U32, size }))
            }
            Token::Mat2 => {
                self.advance();
                Ok(self.module.types.append(Type::Matrix {
                    scalar: Scalar::F32,
                    columns: VectorSize::Bi,
                    rows: VectorSize::Bi,
                }))
            }
            Token::Mat3 => {
                self.advance();
                Ok(self.module.types.append(Type::Matrix {
                    scalar: Scalar::F32,
                    columns: VectorSize::Tri,
                    rows: VectorSize::Tri,
                }))
            }
            Token::Mat4 => {
                self.advance();
                Ok(self.module.types.append(Type::Matrix {
                    scalar: Scalar::F32,
                    columns: VectorSize::Quad,
                    rows: VectorSize::Quad,
                }))
            }
            Token::Struct => self.parse_struct_type_decl(),
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                for (handle, ty) in self.module.types.iter() {
                    if let Type::Struct {
                        name: Some(n), ..
                    } = ty
                    {
                        if *n == name {
                            return Ok(handle);
                        }
                    }
                }
                Err(self.error(format!("unknown struct type: {name}")))
            }
            other => Err(self.error(format!("expected type, got {other:?}"))),
        }
    }

    /// GLSL array declarators follow the identifier: `float data[]`, `float smem[256]`.
    fn parse_array_suffix_after_declarator(
        &mut self,
        mut base: Handle<Type>,
    ) -> Result<Handle<Type>, ParseError> {
        while self.eat(&Token::LeftBracket) {
            if self.eat(&Token::RightBracket) {
                base = self.module.types.append(Type::Array {
                    base,
                    size: ArraySize::Dynamic,
                });
            } else {
                let n = self.parse_const_uint()?;
                self.expect(&Token::RightBracket)?;
                base = self.module.types.append(Type::Array {
                    base,
                    size: ArraySize::Constant(n),
                });
            }
        }
        Ok(base)
    }

    fn parse_struct_type_decl(&mut self) -> Result<Handle<Type>, ParseError> {
        self.expect(&Token::Struct)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LeftBrace)?;
        let mut members = Vec::new();
        while !self.eat(&Token::RightBrace) && !matches!(self.peek(), Token::Eof) {
            let base_ty = self.parse_type_inner()?;
            let mname = self.expect_ident()?;
            let mty = self.parse_array_suffix_after_declarator(base_ty)?;
            self.expect(&Token::Semicolon)?;
            members.push(StructMember {
                name: Some(mname),
                ty: mty,
                offset: None,
                binding: None,
            });
        }
        Ok(self.module.types.append(Type::Struct {
            name: Some(name),
            members,
        }))
    }

    fn parse_struct_body_anonymous(&mut self) -> Result<Handle<Type>, ParseError> {
        self.expect(&Token::LeftBrace)?;
        let mut members = Vec::new();
        while !self.eat(&Token::RightBrace) && !matches!(self.peek(), Token::Eof) {
            let base_ty = self.parse_type_inner()?;
            let mname = self.expect_ident()?;
            let mty = self.parse_array_suffix_after_declarator(base_ty)?;
            self.expect(&Token::Semicolon)?;
            members.push(StructMember {
                name: Some(mname),
                ty: mty,
                offset: None,
                binding: None,
            });
        }
        Ok(self.module.types.append(Type::Struct {
            name: None,
            members,
        }))
    }

    fn parse_layout_qualifiers(&mut self) -> Result<LayoutQualifiers, ParseError> {
        self.expect(&Token::Layout)?;
        self.expect(&Token::LeftParen)?;
        let mut q = LayoutQualifiers::default();
        loop {
            match self.peek().clone() {
                Token::Ident(name) => {
                    self.advance();
                    self.expect(&Token::Equal)?;
                    let v = self.parse_const_uint()?;
                    match name.as_str() {
                        "local_size_x" => q.local_size[0] = Some(v),
                        "local_size_y" => q.local_size[1] = Some(v),
                        "local_size_z" => q.local_size[2] = Some(v),
                        "set" => q.set = Some(v),
                        "binding" => q.binding = Some(v),
                        "location" => q.location = Some(v),
                        _ => {}
                    }
                }
                _ => {
                    self.advance();
                }
            }
            if self.eat(&Token::RightParen) {
                break;
            }
            self.eat(&Token::Comma);
        }
        Ok(q)
    }

    fn parse_buffer_storage_access(&self, readonly: bool, writeonly: bool) -> StorageAccess {
        match (readonly, writeonly) {
            (true, false) => StorageAccess::Load,
            (false, true) => StorageAccess::Store,
            _ => StorageAccess::LoadStore,
        }
    }

    fn parse_buffer_declaration_after_layout(
        &mut self,
        layout: LayoutQualifiers,
    ) -> Result<(), ParseError> {
        self.expect(&Token::Buffer)?;
        let mut readonly = false;
        let mut writeonly = false;
        loop {
            match self.peek().clone() {
                Token::Readonly => {
                    readonly = true;
                    self.advance();
                }
                Token::Writeonly => {
                    writeonly = true;
                    self.advance();
                }
                Token::Restrict | Token::Coherent | Token::Volatile => {
                    self.advance();
                }
                _ => break,
            }
        }
        let access = self.parse_buffer_storage_access(readonly, writeonly);

        let struct_ty = if matches!(self.peek(), Token::LeftBrace) {
            self.parse_struct_body_anonymous()?
        } else {
            let _ = self.expect_ident()?;
            self.parse_struct_body_anonymous()?
        };

        let instance = self.expect_ident()?;
        self.expect(&Token::Semicolon)?;

        let rb = match (layout.set, layout.binding) {
            (Some(g), Some(b)) => Some(ResourceBinding {
                group: g,
                binding: b,
            }),
            _ => None,
        };

        self.module.global_variables.push(GlobalVariable {
            name: Some(instance),
            space: AddressSpace::Storage { access },
            binding: rb,
            ty: struct_ty,
        });
        Ok(())
    }

    fn parse_uniform_declaration(&mut self, layout: LayoutQualifiers) -> Result<(), ParseError> {
        self.expect(&Token::Uniform)?;
        let struct_ty = if matches!(self.peek(), Token::LeftBrace) {
            self.parse_struct_body_anonymous()?
        } else {
            let _ = self.expect_ident()?;
            self.parse_struct_body_anonymous()?
        };
        let instance = self.expect_ident()?;
        self.expect(&Token::Semicolon)?;

        let rb = match (layout.set, layout.binding) {
            (Some(g), Some(b)) => Some(ResourceBinding {
                group: g,
                binding: b,
            }),
            _ => None,
        };

        self.module.global_variables.push(GlobalVariable {
            name: Some(instance),
            space: AddressSpace::Uniform,
            binding: rb,
            ty: struct_ty,
        });
        Ok(())
    }

    fn parse_shared_declaration(&mut self) -> Result<(), ParseError> {
        self.expect(&Token::Shared)?;
        let base_ty = self.parse_type_inner()?;
        let name = self.expect_ident()?;
        let ty = self.parse_array_suffix_after_declarator(base_ty)?;
        self.expect(&Token::Semicolon)?;
        self.module.global_variables.push(GlobalVariable {
            name: Some(name),
            space: AddressSpace::WorkGroup,
            binding: None,
            ty,
        });
        Ok(())
    }

    fn parse_global_declaration(&mut self) -> Result<(), ParseError> {
        if matches!(self.peek(), Token::Layout) {
            let layout = self.parse_layout_qualifiers()?;
            match self.peek().clone() {
                Token::In => {
                    for i in 0..3 {
                        if let Some(v) = layout.local_size[i] {
                            self.workgroup_size[i] = v;
                        }
                    }
                    self.advance();
                    self.expect(&Token::Semicolon)?;
                    return Ok(());
                }
                Token::Buffer => return self.parse_buffer_declaration_after_layout(layout),
                Token::Uniform => return self.parse_uniform_declaration(layout),
                other => {
                    return Err(self.error(format!(
                        "unsupported declaration after layout: {other:?}"
                    )))
                }
            }
        }
        match self.peek().clone() {
            Token::Struct => {
                let _ = self.parse_struct_type_decl()?;
                self.expect(&Token::Semicolon)?;
                Ok(())
            }
            Token::Buffer => {
                let layout = LayoutQualifiers::default();
                self.parse_buffer_declaration_after_layout(layout)
            }
            Token::Uniform => {
                let layout = LayoutQualifiers::default();
                self.parse_uniform_declaration(layout)
            }
            Token::Shared => self.parse_shared_declaration(),
            Token::Void => self.parse_main(),
            other => Err(self.error(format!("unexpected global token: {other:?}"))),
        }
    }

    fn parse_main(&mut self) -> Result<(), ParseError> {
        self.expect(&Token::Void)?;
        let name = self.expect_ident()?;
        if name != "main" {
            return Err(ParseError::Unsupported(format!(
                "only void main() is supported for GLSL compute (got {name})"
            )));
        }
        self.expect(&Token::LeftParen)?;
        self.expect(&Token::RightParen)?;
        self.expect(&Token::LeftBrace)?;

        let mut func = Function::new();
        func.name = Some("main".into());
        func.arguments = self.glsl_builtin_function_arguments();
        func.result = None;

        let body = self.parse_statement_block(&mut func)?;
        func.body = body;

        self.module.entry_points.push(EntryPoint {
            name: "main".into(),
            stage: ShaderStage::Compute,
            workgroup_size: self.workgroup_size,
            function: func,
        });
        Ok(())
    }

    fn parse_statement_block(&mut self, func: &mut Function) -> Result<Vec<Statement>, ParseError> {
        let mut stmts = Vec::new();
        while !self.eat(&Token::RightBrace) && !matches!(self.peek(), Token::Eof) {
            stmts.push(self.parse_statement(func)?);
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        match self.peek().clone() {
            Token::Return => self.parse_return(func),
            Token::If => self.parse_if(func),
            Token::For => self.parse_for(func),
            Token::While => self.parse_while(func),
            Token::Do => self.parse_do_while(func),
            Token::Switch => self.parse_switch(func),
            Token::Break => {
                self.advance();
                self.expect(&Token::Semicolon)?;
                Ok(Statement::Break)
            }
            Token::Continue => {
                self.advance();
                self.expect(&Token::Semicolon)?;
                Ok(Statement::Continue)
            }
            Token::LeftBrace => {
                self.advance();
                let b = self.parse_statement_block(func)?;
                Ok(Statement::Block(b))
            }
            Token::Barrier => self.parse_barrier_stmt(func),
            Token::MemoryBarrier | Token::MemoryBarrierBuffer | Token::MemoryBarrierShared => {
                self.parse_memory_barrier_stmt(func)
            }
            Token::GroupMemoryBarrier => self.parse_group_memory_barrier_stmt(func),
            Token::Const => self.parse_const_decl(func),
            _ => {
                if self.statement_starts_declaration() {
                    self.parse_local_declaration(func)
                } else {
                    let s = self.parse_assignment_or_call(func)?;
                    self.expect(&Token::Semicolon)?;
                    Ok(s)
                }
            }
        }
    }

    fn statement_starts_declaration(&self) -> bool {
        matches!(
            self.peek(),
            Token::Uint
                | Token::Int
                | Token::Float
                | Token::Double
                | Token::Bool
                | Token::Vec2
                | Token::Vec3
                | Token::Vec4
                | Token::Ivec2
                | Token::Ivec3
                | Token::Ivec4
                | Token::Uvec2
                | Token::Uvec3
                | Token::Uvec4
                | Token::Mat2
                | Token::Mat3
                | Token::Mat4
        )
    }

    fn parse_const_decl(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::Const)?;
        let base_ty = self.parse_type_inner()?;
        let name = self.expect_ident()?;
        let ty = self.parse_array_suffix_after_declarator(base_ty)?;
        self.expect(&Token::Equal)?;
        let init = self.parse_expression(func)?;
        self.expect(&Token::Semicolon)?;
        let idx = func.local_variables.len() as u32;
        func.local_variables.push(LocalVariable {
            name: Some(name),
            ty,
            init: Some(init),
        });
        Ok(Statement::LocalDecl {
            local_var_index: idx,
        })
    }

    fn parse_local_declaration(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        let base_ty = self.parse_type_inner()?;
        let name = self.expect_ident()?;
        let ty = self.parse_array_suffix_after_declarator(base_ty)?;
        let init = if self.eat(&Token::Equal) {
            Some(self.parse_expression(func)?)
        } else {
            None
        };
        self.expect(&Token::Semicolon)?;
        let idx = func.local_variables.len() as u32;
        func.local_variables.push(LocalVariable {
            name: Some(name),
            ty,
            init,
        });
        Ok(Statement::LocalDecl {
            local_var_index: idx,
        })
    }

    fn parse_barrier_stmt(&mut self, _func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::Barrier)?;
        self.expect(&Token::LeftParen)?;
        self.expect(&Token::RightParen)?;
        self.expect(&Token::Semicolon)?;
        Ok(Statement::ControlBarrier(Barrier::WORK_GROUP))
    }

    fn parse_memory_barrier_stmt(&mut self, _func: &mut Function) -> Result<Statement, ParseError> {
        self.advance(); // memoryBarrier*
        self.expect(&Token::LeftParen)?;
        self.expect(&Token::RightParen)?;
        self.expect(&Token::Semicolon)?;
        Ok(Statement::MemoryBarrier(Barrier::STORAGE))
    }

    fn parse_group_memory_barrier_stmt(
        &mut self,
        _func: &mut Function,
    ) -> Result<Statement, ParseError> {
        self.expect(&Token::GroupMemoryBarrier)?;
        self.expect(&Token::LeftParen)?;
        self.expect(&Token::RightParen)?;
        self.expect(&Token::Semicolon)?;
        Ok(Statement::ControlBarrier(Barrier::ALL))
    }

    fn parse_return(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::Return)?;
        if self.eat(&Token::Semicolon) {
            return Ok(Statement::Return { value: None });
        }
        let value = Some(self.parse_expression(func)?);
        self.expect(&Token::Semicolon)?;
        Ok(Statement::Return { value })
    }

    fn parse_if(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::If)?;
        self.expect(&Token::LeftParen)?;
        let cond = self.parse_expression(func)?;
        self.expect(&Token::RightParen)?;
        let accept = if matches!(self.peek(), Token::LeftBrace) {
            self.advance();
            self.parse_statement_block(func)?
        } else {
            vec![self.parse_statement(func)?]
        };
        let reject = if self.eat(&Token::Else) {
            if matches!(self.peek(), Token::If) {
                vec![self.parse_if(func)?]
            } else if matches!(self.peek(), Token::LeftBrace) {
                self.advance();
                self.parse_statement_block(func)?
            } else {
                vec![self.parse_statement(func)?]
            }
        } else {
            Vec::new()
        };
        Ok(Statement::If {
            condition: cond,
            accept,
            reject,
        })
    }

    fn parse_for(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::For)?;
        self.expect(&Token::LeftParen)?;
        let init = if self.eat(&Token::Semicolon) {
            None
        } else {
            let i = self.parse_for_init(func)?;
            self.expect(&Token::Semicolon)?;
            Some(Box::new(i))
        };
        let condition = if self.eat(&Token::Semicolon) {
            None
        } else {
            let c = self.parse_expression(func)?;
            self.expect(&Token::Semicolon)?;
            Some(c)
        };
        let update = if self.eat(&Token::RightParen) {
            None
        } else {
            let u = self.parse_for_update(func)?;
            self.expect(&Token::RightParen)?;
            Some(Box::new(u))
        };
        let body = if matches!(self.peek(), Token::LeftBrace) {
            self.advance();
            self.parse_statement_block(func)?
        } else {
            vec![self.parse_statement(func)?]
        };
        Ok(Statement::ForLoop {
            init,
            condition,
            update,
            body,
        })
    }

    fn parse_for_init(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        if self.statement_starts_declaration() {
            self.parse_local_declaration(func)
        } else {
            self.parse_assignment_or_call(func)
        }
    }

    fn parse_for_update(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.parse_assignment_or_call(func)
    }

    fn parse_while(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::While)?;
        self.expect(&Token::LeftParen)?;
        let condition = self.parse_expression(func)?;
        self.expect(&Token::RightParen)?;
        let body = if matches!(self.peek(), Token::LeftBrace) {
            self.advance();
            self.parse_statement_block(func)?
        } else {
            vec![self.parse_statement(func)?]
        };
        Ok(Statement::WhileLoop { condition, body })
    }

    fn parse_do_while(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::Do)?;
        let body = if matches!(self.peek(), Token::LeftBrace) {
            self.advance();
            self.parse_statement_block(func)?
        } else {
            vec![self.parse_statement(func)?]
        };
        self.expect(&Token::While)?;
        self.expect(&Token::LeftParen)?;
        let condition = self.parse_expression(func)?;
        self.expect(&Token::RightParen)?;
        self.expect(&Token::Semicolon)?;
        let while_part = Statement::WhileLoop {
            condition,
            body: body.clone(),
        };
        Ok(Statement::Block(vec![Statement::Block(body), while_part]))
    }

    fn parse_switch(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        self.expect(&Token::Switch)?;
        self.expect(&Token::LeftParen)?;
        let selector = self.parse_expression(func)?;
        self.expect(&Token::RightParen)?;
        self.expect(&Token::LeftBrace)?;
        let mut cases = Vec::new();
        while !self.eat(&Token::RightBrace) && !matches!(self.peek(), Token::Eof) {
            if self.eat(&Token::Case) {
                let value = match self.peek().clone() {
                    Token::IntLiteral(n) => {
                        self.advance();
                        SwitchValue::I32(n as i32)
                    }
                    Token::UintLiteral(n) => {
                        self.advance();
                        SwitchValue::U32(n as u32)
                    }
                    _ => return Err(self.error("expected case constant")),
                };
                self.expect(&Token::Colon)?;
                let mut body = Vec::new();
                while !matches!(self.peek(), Token::Case | Token::Default | Token::RightBrace) {
                    body.push(self.parse_statement(func)?);
                }
                cases.push(SwitchCase {
                    value,
                    body,
                    fall_through: false,
                });
            } else if self.eat(&Token::Default) {
                self.expect(&Token::Colon)?;
                let mut body = Vec::new();
                while !matches!(self.peek(), Token::Case | Token::RightBrace) {
                    body.push(self.parse_statement(func)?);
                }
                cases.push(SwitchCase {
                    value: SwitchValue::Default,
                    body,
                    fall_through: false,
                });
            } else {
                return Err(self.error("expected case or default"));
            }
        }
        Ok(Statement::Switch { selector, cases })
    }

    fn parse_assignment_or_call(&mut self, func: &mut Function) -> Result<Statement, ParseError> {
        let lhs = self.parse_expression(func)?;
        match self.peek().clone() {
            Token::Equal => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::Store {
                    pointer: lhs,
                    value,
                })
            }
            Token::PlusEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign {
                    pointer: lhs,
                    op: BinaryOp::Add,
                    value,
                })
            }
            Token::MinusEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign {
                    pointer: lhs,
                    op: BinaryOp::Subtract,
                    value,
                })
            }
            Token::StarEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign {
                    pointer: lhs,
                    op: BinaryOp::Multiply,
                    value,
                })
            }
            Token::SlashEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign {
                    pointer: lhs,
                    op: BinaryOp::Divide,
                    value,
                })
            }
            Token::PercentEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign {
                    pointer: lhs,
                    op: BinaryOp::Modulo,
                    value,
                })
            }
            Token::AmpEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign {
                    pointer: lhs,
                    op: BinaryOp::BitwiseAnd,
                    value,
                })
            }
            Token::PipeEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign {
                    pointer: lhs,
                    op: BinaryOp::BitwiseOr,
                    value,
                })
            }
            Token::CaretEqual => {
                self.advance();
                let value = self.parse_expression(func)?;
                Ok(Statement::CompoundAssign {
                    pointer: lhs,
                    op: BinaryOp::BitwiseXor,
                    value,
                })
            }
            Token::PlusPlus => {
                self.advance();
                Ok(Statement::Increment { pointer: lhs })
            }
            Token::MinusMinus => {
                self.advance();
                Ok(Statement::Decrement { pointer: lhs })
            }
            _ => Ok(Statement::Phony { value: lhs }),
        }
    }

    fn parse_expression(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        self.parse_conditional_expr(func)
    }

    fn parse_conditional_expr(
        &mut self,
        func: &mut Function,
    ) -> Result<Handle<Expression>, ParseError> {
        let cond = self.parse_or_expr(func)?;
        if self.eat(&Token::Question) {
            let accept = self.parse_expression(func)?;
            self.expect(&Token::Colon)?;
            let reject = self.parse_conditional_expr(func)?;
            return Ok(func.expressions.append(Expression::Select {
                condition: cond,
                accept,
                reject,
            }));
        }
        Ok(cond)
    }

    fn parse_or_expr(&mut self, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
        let mut left = self.parse_and_expr(func)?;
        while self.eat(&Token::PipePipe) {
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
        while self.eat(&Token::AmpersandAmpersand) {
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
        while self.eat(&Token::Pipe) {
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
        while self.eat(&Token::Caret) {
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
        while self.eat(&Token::Ampersand) {
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
                Token::LessAngle => BinaryOp::Less,
                Token::GreaterAngle => BinaryOp::Greater,
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

    fn parse_multiplicative_expr(
        &mut self,
        func: &mut Function,
    ) -> Result<Handle<Expression>, ParseError> {
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
                Ok(func
                    .expressions
                    .append(Expression::Unary { op: UnaryOp::Negate, expr }))
            }
            Token::Bang => {
                self.advance();
                let expr = self.parse_unary_expr(func)?;
                Ok(func
                    .expressions
                    .append(Expression::Unary { op: UnaryOp::LogicalNot, expr }))
            }
            Token::Tilde => {
                self.advance();
                let expr = self.parse_unary_expr(func)?;
                Ok(func.expressions.append(Expression::Unary {
                    op: UnaryOp::BitwiseNot,
                    expr,
                }))
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
                    let field = match self.peek().clone() {
                        Token::Ident(s) => {
                            self.advance();
                            s
                        }
                        other => return Err(self.error(format!("expected field name, got {other:?}"))),
                    };
                    expr = self.parse_swizzle_or_field(&field, expr, func)?;
                }
                Token::PlusPlus => {
                    self.advance();
                    let one = func
                        .expressions
                        .append(Expression::Literal(Literal::I32(1)));
                    expr = func.expressions.append(Expression::Binary {
                        op: BinaryOp::Add,
                        left: expr,
                        right: one,
                    });
                }
                Token::MinusMinus => {
                    self.advance();
                    let one = func
                        .expressions
                        .append(Expression::Literal(Literal::I32(1)));
                    expr = func.expressions.append(Expression::Binary {
                        op: BinaryOp::Subtract,
                        left: expr,
                        right: one,
                    });
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_swizzle_or_field(
        &mut self,
        field: &str,
        base: Handle<Expression>,
        func: &mut Function,
    ) -> Result<Handle<Expression>, ParseError> {
        if field.len() >= 1 && self.swizzle_chars(field) {
            return self.parse_swizzle_expr(field, base, func);
        }
        let base_ty = self.infer_expr_type(base, func)?;
        if let Type::Struct { members, .. } = &self.module.types[base_ty] {
            for (i, m) in members.iter().enumerate() {
                if m.name.as_deref() == Some(field) {
                    return Ok(func.expressions.append(Expression::AccessIndex {
                        base,
                        index: i as u32,
                    }));
                }
            }
        }
        Err(self.error(format!("unknown field `{field}`")))
    }

    fn swizzle_chars(&self, field: &str) -> bool {
        field.chars().all(|c| matches!(c, 'x' | 'y' | 'z' | 'w' | 'r' | 'g' | 'b' | 'a'))
    }

    fn parse_swizzle_expr(
        &mut self,
        field: &str,
        base: Handle<Expression>,
        func: &mut Function,
    ) -> Result<Handle<Expression>, ParseError> {
        let mut indices = Vec::new();
        for ch in field.chars() {
            let idx = match ch {
                'x' | 'r' => 0u32,
                'y' | 'g' => 1,
                'z' | 'b' => 2,
                'w' | 'a' => 3,
                _ => return Err(self.error("invalid swizzle")),
            };
            indices.push(idx);
        }
        if indices.len() == 1 {
            return Ok(func.expressions.append(Expression::AccessIndex {
                base,
                index: indices[0],
            }));
        }
        let size = match indices.len() {
            2 => VectorSize::Bi,
            3 => VectorSize::Tri,
            4 => VectorSize::Quad,
            _ => return Err(self.error("invalid swizzle length")),
        };
        let mut pattern = [0u32; 4];
        for (i, &s) in indices.iter().enumerate() {
            pattern[i] = s;
        }
        Ok(func.expressions.append(Expression::Swizzle {
            vector: base,
            pattern,
            size,
        }))
    }

    fn infer_expr_type(
        &mut self,
        expr: Handle<Expression>,
        func: &Function,
    ) -> Result<Handle<Type>, ParseError> {
        match &func.expressions[expr] {
            Expression::GlobalVariable(i) => Ok(self.module.global_variables[*i as usize].ty),
            Expression::LocalVariable(i) => Ok(func.local_variables[*i as usize].ty),
            Expression::FunctionArgument(i) => Ok(func.arguments[*i as usize].ty),
            Expression::Access { base, .. } => {
                let bt = self.infer_expr_type(*base, func)?;
                match &self.module.types[bt] {
                    Type::Array { base, .. } => Ok(*base),
                    Type::Vector { scalar, .. } => Ok(self.scalar_handle(*scalar)),
                    Type::Matrix { .. } => Ok(self.scalar_handle(Scalar::F32)),
                    _ => Err(self.error("cannot infer indexed type")),
                }
            }
            Expression::AccessIndex { base, index } => {
                let bt = self.infer_expr_type(*base, func)?;
                match &self.module.types[bt] {
                    Type::Struct { members, .. } => Ok(members[*index as usize].ty),
                    Type::Vector { scalar, .. } => Ok(self.scalar_handle(*scalar)),
                    Type::Matrix { .. } => Ok(self.scalar_handle(Scalar::F32)),
                    _ => Err(self.error("AccessIndex: invalid base type")),
                }
            }
            Expression::Swizzle { .. } => Err(self.error("swizzle type inference")),
            Expression::Literal(Literal::F32(_)) => Ok(self.scalar_handle(Scalar::F32)),
            Expression::Literal(Literal::I32(_)) => Ok(self.scalar_handle(Scalar::I32)),
            Expression::Literal(Literal::U32(_)) => Ok(self.scalar_handle(Scalar::U32)),
            Expression::Literal(Literal::Bool(_)) => Ok(self.bool_type()),
            Expression::Literal(Literal::F64(_)) => Ok(self.scalar_handle(Scalar::F64)),
            _ => Err(self.error("expression type inference not implemented")),
        }
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
                Ok(func
                    .expressions
                    .append(Expression::Literal(Literal::F32(f as f32))))
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
            Token::GlGlobalInvocationId
            | Token::GlLocalInvocationId
            | Token::GlWorkGroupId
            | Token::GlNumWorkGroups
            | Token::GlLocalInvocationIndex
            | Token::GlWorkGroupSize => {
                let name = match self.peek().clone() {
                    Token::GlGlobalInvocationId => "gl_GlobalInvocationID",
                    Token::GlLocalInvocationId => "gl_LocalInvocationID",
                    Token::GlWorkGroupId => "gl_WorkGroupID",
                    Token::GlNumWorkGroups => "gl_NumWorkGroups",
                    Token::GlLocalInvocationIndex => "gl_LocalInvocationIndex",
                    Token::GlWorkGroupSize => "gl_WorkGroupSize",
                    _ => unreachable!(),
                };
                self.advance();
                self.resolve_name(name, func)
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                if self.eat(&Token::LeftParen) {
                    self.parse_function_call(&name, func)
                } else {
                    self.resolve_name(&name, func)
                }
            }
            _ => Err(self.error(format!("unexpected token in expression: {:?}", self.peek()))),
        }
    }

    fn resolve_name(&mut self, name: &str, func: &mut Function) -> Result<Handle<Expression>, ParseError> {
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
        Err(self.error(format!("undefined name: {name}")))
    }

    fn parse_function_call(
        &mut self,
        name: &str,
        func: &mut Function,
    ) -> Result<Handle<Expression>, ParseError> {
        let mut args = Vec::new();
        if !self.eat(&Token::RightParen) {
            loop {
                args.push(self.parse_expression(func)?);
                if self.eat(&Token::RightParen) {
                    break;
                }
                self.expect(&Token::Comma)?;
            }
        }
        self.resolve_math_or_builtin(name, &args, func)
    }

    fn resolve_math_or_builtin(
        &mut self,
        name: &str,
        args: &[Handle<Expression>],
        func: &mut Function,
    ) -> Result<Handle<Expression>, ParseError> {
        if let Some(mf) = Self::glsl_math_function(name) {
            return match (args.len(), mf) {
                (1, m) => Ok(func.expressions.append(Expression::Math {
                    fun: m,
                    arg: args[0],
                    arg1: None,
                    arg2: None,
                })),
                (2, MathFunction::Min
                    | MathFunction::Max
                    | MathFunction::Pow
                    | MathFunction::Atan2
                    | MathFunction::Distance
                    | MathFunction::Dot
                    | MathFunction::Step) => Ok(func.expressions.append(Expression::Math {
                    fun: mf,
                    arg: args[0],
                    arg1: Some(args[1]),
                    arg2: None,
                })),
                (3, MathFunction::Clamp | MathFunction::Fma | MathFunction::Mix | MathFunction::Cross) => {
                    Ok(func.expressions.append(Expression::Math {
                        fun: mf,
                        arg: args[0],
                        arg1: Some(args[1]),
                        arg2: Some(args[2]),
                    }))
                }
                (3, MathFunction::SmoothStep) => Ok(func.expressions.append(Expression::Math {
                    fun: mf,
                    arg: args[0],
                    arg1: Some(args[1]),
                    arg2: Some(args[2]),
                })),
                _ => Err(self.error(format!("wrong arg count for {name}"))),
            };
        }
        Err(self.error(format!("unknown function: {name}")))
    }

    fn glsl_math_function(name: &str) -> Option<MathFunction> {
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
            "inversesqrt" => MathFunction::InverseSqrt,
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
            _ => return None,
        })
    }

    fn parse_module(&mut self) -> Result<(), ParseError> {
        while !matches!(self.peek(), Token::Eof) && !matches!(self.peek(), Token::Void) {
            self.parse_global_declaration()?;
        }
        if !matches!(self.peek(), Token::Void) {
            return Err(self.error("expected void main() entry point"));
        }
        self.parse_main()?;
        if !matches!(self.peek(), Token::Eof) {
            return Err(self.error("unexpected tokens after main"));
        }
        Ok(())
    }
}

pub fn parse(source: &str) -> Result<Module, ParseError> {
    let mut p = Parser::new(source);
    p.parse_module()?;
    Ok(p.module)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parse_compute_shader_sample() {
        let src = r#"
#version 460
layout(local_size_x = 256) in;
layout(set = 0, binding = 0) buffer readonly InputBuffer { float data[]; } input_buf;
layout(set = 0, binding = 1) buffer OutputBuffer { float data[]; } output_buf;
layout(set = 0, binding = 2) uniform Params { uint n; } params;
shared float smem[256];

void main() {
    uint gid = gl_GlobalInvocationID.x;
    if (gid >= params.n) return;
    output_buf.data[gid] = input_buf.data[gid] + 1.0;
    barrier();
}
"#;
        let m = parse(src).expect("parse");
        assert_eq!(m.entry_points.len(), 1);
        assert_eq!(m.entry_points[0].workgroup_size[0], 256);
        assert_eq!(m.global_variables.len(), 4);
        assert_eq!(m.entry_points[0].function.arguments.len(), 6);
    }
}
