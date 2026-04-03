// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign shader AST — the typed layer between source parsing and CoralIR lowering.
//!
//! All three frontends (WGSL, SPIR-V, GLSL) target this shared AST.
//! A single lowering pass converts it to `coral_reef::codegen::ir::Shader`.

mod types;
pub use types::*;

mod expr;
pub use expr::*;

mod stmt;
pub use stmt::*;

mod builtins;
pub use builtins::*;

use std::fmt;

/// Arena-backed index handle. Lightweight alternative to `Rc`/`Arc`.
#[derive(PartialEq, Eq, Hash)]
pub struct Handle<T> {
    index: u32,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> Handle<T> {
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self {
            index,
            _marker: std::marker::PhantomData,
        }
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({})", self.index)
    }
}

/// Simple typed arena for AST nodes.
#[derive(Debug)]
pub struct Arena<T> {
    data: Vec<T>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Arena<T> {
    #[must_use]
    pub const fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn append(&mut self, value: T) -> Handle<T> {
        let index = self.data.len() as u32;
        self.data.push(value);
        Handle::new(index)
    }

    #[must_use]
    pub fn get(&self, handle: Handle<T>) -> &T {
        &self.data[handle.index() as usize]
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> &mut T {
        &mut self.data[handle.index() as usize]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.data
            .iter()
            .enumerate()
            .map(|(i, v)| (Handle::new(i as u32), v))
    }
}

impl<T> std::ops::Index<Handle<T>> for Arena<T> {
    type Output = T;
    fn index(&self, handle: Handle<T>) -> &T {
        self.get(handle)
    }
}

impl<T> std::ops::IndexMut<Handle<T>> for Arena<T> {
    fn index_mut(&mut self, handle: Handle<T>) -> &mut T {
        self.get_mut(handle)
    }
}

/// A parsed shader module.
#[derive(Debug)]
pub struct Module {
    pub types: Arena<Type>,
    pub global_variables: Vec<GlobalVariable>,
    pub functions: Vec<Function>,
    pub entry_points: Vec<EntryPoint>,
}

impl Module {
    #[must_use]
    pub fn new() -> Self {
        Self {
            types: Arena::new(),
            global_variables: Vec::new(),
            functions: Vec::new(),
            entry_points: Vec::new(),
        }
    }
}

impl Default for Module {
    fn default() -> Self {
        Self::new()
    }
}

/// A global variable declaration (storage, uniform, or workgroup buffer).
#[derive(Debug)]
pub struct GlobalVariable {
    pub name: Option<String>,
    pub space: AddressSpace,
    pub binding: Option<ResourceBinding>,
    pub ty: Handle<Type>,
}

/// Resource binding location.
#[derive(Debug, Clone, Copy)]
pub struct ResourceBinding {
    pub group: u32,
    pub binding: u32,
}

/// Address space for global variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressSpace {
    Storage { access: StorageAccess },
    Uniform,
    WorkGroup,
    Private,
    Function,
    Handle,
}

/// Access mode for storage buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageAccess {
    Load,
    Store,
    LoadStore,
}

/// Shader stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    Compute,
    Vertex,
    Fragment,
}

/// An entry point into the shader.
#[derive(Debug)]
pub struct EntryPoint {
    pub name: String,
    pub stage: ShaderStage,
    pub workgroup_size: [u32; 3],
    pub function: Function,
}

/// A function (entry point body or called function).
#[derive(Debug)]
pub struct Function {
    pub name: Option<String>,
    pub arguments: Vec<FunctionArgument>,
    pub result: Option<Handle<Type>>,
    pub local_variables: Vec<LocalVariable>,
    pub expressions: Arena<Expression>,
    pub body: Vec<Statement>,
}

impl Function {
    #[must_use]
    pub fn new() -> Self {
        Self {
            name: None,
            arguments: Vec::new(),
            result: None,
            local_variables: Vec::new(),
            expressions: Arena::new(),
            body: Vec::new(),
        }
    }
}

impl Default for Function {
    fn default() -> Self {
        Self::new()
    }
}

/// An argument to a function / entry point.
#[derive(Debug)]
pub struct FunctionArgument {
    pub name: Option<String>,
    pub ty: Handle<Type>,
    pub binding: Option<Binding>,
}

/// Binding for a function argument or struct member (builtins or location).
#[derive(Debug, Clone)]
pub enum Binding {
    BuiltIn(BuiltIn),
    Location {
        location: u32,
        interpolation: Option<Interpolation>,
        sampling: Option<Sampling>,
    },
}

/// A local variable inside a function.
#[derive(Debug)]
pub struct LocalVariable {
    pub name: Option<String>,
    pub ty: Handle<Type>,
    pub init: Option<Handle<Expression>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_basic_operations() {
        let mut arena: Arena<u32> = Arena::new();
        assert!(arena.is_empty());
        let h1 = arena.append(42);
        let h2 = arena.append(99);
        assert_eq!(arena.len(), 2);
        assert_eq!(arena[h1], 42);
        assert_eq!(arena[h2], 99);
    }

    #[test]
    fn handle_index_roundtrip() {
        let h: Handle<u32> = Handle::new(7);
        assert_eq!(h.index(), 7);
    }

    #[test]
    fn module_default_empty() {
        let m = Module::new();
        assert!(m.types.is_empty());
        assert!(m.global_variables.is_empty());
        assert!(m.entry_points.is_empty());
    }
}
