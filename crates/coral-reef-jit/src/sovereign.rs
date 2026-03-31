// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign compilation path — pure Rust, no `cranelift-jit`/`libc`.
//!
//! Uses `cranelift-codegen` directly for code generation and
//! [`JitMemory`](crate::runtime::JitMemory) for executable memory, bypassing
//! `cranelift-jit` and its `libc` dependencies entirely. Math functions are
//! resolved through the pure-Rust `libm` crate.

use cranelift_codegen::ir::{AbiParam, ExternalName, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

use crate::builtins;
use crate::error::JitError;
use crate::translate::{CompiledBacking, CompiledKernel, FunctionTranslator};

/// Translate and compile a shader using the sovereign rustix-based runtime.
///
/// This path uses `cranelift-codegen` directly for code generation and
/// [`JitMemory`](crate::runtime::JitMemory) for executable memory, bypassing
/// `cranelift-jit` and its `libc` dependencies entirely.
///
/// # Errors
///
/// Returns [`JitError`] if compilation, relocation, or memory mapping fails.
pub fn translate_and_compile_sovereign(
    shader: &coral_reef::codegen::ir::Shader<'_>,
) -> Result<CompiledKernel, JitError> {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("use_colocated_libcalls", "false")
        .map_err(|e| JitError::Setup(e.to_string()))?;
    flag_builder
        .set("is_pic", "false")
        .map_err(|e| JitError::Setup(e.to_string()))?;
    let isa_builder = cranelift_native::builder()
        .map_err(|msg| JitError::Setup(format!("unsupported host: {msg}")))?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| JitError::Setup(e.to_string()))?;

    let ptr_type = isa.pointer_type();
    let call_conv = isa.default_call_conv();

    let mut func = cranelift_codegen::ir::Function::new();
    func.signature.call_conv = call_conv;
    func.signature.params.push(AbiParam::new(ptr_type));
    for _ in 1..builtins::params::PARAM_COUNT {
        func.signature.params.push(AbiParam::new(types::I32));
    }

    if shader.functions.is_empty() {
        return Err(JitError::Translation("shader has no functions".into()));
    }

    let mut libm_names: Vec<(&'static str, cranelift_codegen::ir::FuncRef)> = Vec::new();

    {
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut fb_ctx);
        let mut translator =
            FunctionTranslator::new_sovereign(&mut builder, ptr_type, call_conv, &mut libm_names);
        translator.translate_function(&shader.functions[0])?;
        builder.finalize();
    }

    let mut ctx = cranelift_codegen::Context::for_function(func);
    #[expect(clippy::default_trait_access, reason = "ControlPlane is not re-exported from cranelift-codegen")]
    let (mut code_buf, relocs) = {
        let compiled = ctx
            .compile(&*isa, &mut Default::default())
            .map_err(|e| JitError::Compilation(format!("{e:?}")))?;
        (compiled.code_buffer().to_vec(), compiled.buffer.relocs().to_vec())
    };

    apply_sovereign_relocations(&mut code_buf, &relocs, &ctx.func, &libm_names)?;

    let mut memory = crate::runtime::JitMemory::allocate(code_buf.len())?;
    memory.write(0, &code_buf)?;
    memory.make_executable()?;

    let fn_ptr = memory.code_ptr()?;

    Ok(CompiledKernel::new(CompiledBacking::Sovereign(memory), fn_ptr))
}

/// Resolve relocations in compiled code, patching in actual function addresses.
#[expect(clippy::cast_possible_wrap, reason = "pointer arithmetic for relocations")]
fn apply_sovereign_relocations(
    code: &mut [u8],
    relocs: &[cranelift_codegen::FinalizedMachReloc],
    func: &cranelift_codegen::ir::Function,
    libm_names: &[(&str, cranelift_codegen::ir::FuncRef)],
) -> Result<(), JitError> {
    use cranelift_codegen::FinalizedRelocTarget;

    for reloc in relocs {
        let target_name = match &reloc.target {
            FinalizedRelocTarget::ExternalName(ExternalName::User(user_ref)) => {
                let user_name = &func.params.user_named_funcs()[*user_ref];
                libm_names
                    .iter()
                    .find(|(_, fr)| {
                        let data = &func.dfg.ext_funcs[*fr];
                        if let ExternalName::User(ur) = data.name {
                            func.params.user_named_funcs()[ur].index == user_name.index
                        } else {
                            false
                        }
                    })
                    .map(|(name, _)| *name)
            }
            FinalizedRelocTarget::ExternalName(ExternalName::LibCall(lc)) => {
                Some(libcall_name(*lc))
            }
            other => {
                return Err(JitError::Compilation(format!(
                    "unresolvable relocation target: {other:?}",
                )));
            }
        };

        let Some(name) = target_name else {
            return Err(JitError::Compilation(
                "unresolvable user relocation in sovereign pipeline".into(),
            ));
        };

        let target_addr = resolve_libm_address(name)?;

        #[expect(unsafe_code, reason = "pointer arithmetic for code patching")]
        let call_site = unsafe { code.as_ptr().add(reloc.offset as usize) } as usize;

        match reloc.kind {
            cranelift_codegen::binemit::Reloc::Abs8 => {
                let off = reloc.offset as usize;
                if off + 8 <= code.len() {
                    code[off..off + 8]
                        .copy_from_slice(&(target_addr as u64).to_le_bytes());
                }
            }
            cranelift_codegen::binemit::Reloc::X86PCRel4 => {
                let pc = call_site + 4;
                let rel = (target_addr as isize) - (pc as isize);
                #[expect(clippy::cast_possible_truncation, reason = "x86 PC-relative relocations are 32-bit by definition")]
                let rel32 = rel as i32;
                let off = reloc.offset as usize;
                if off + 4 <= code.len() {
                    code[off..off + 4].copy_from_slice(&rel32.to_le_bytes());
                }
            }
            _ => {
                return Err(JitError::Compilation(format!(
                    "unsupported relocation kind: {:?}",
                    reloc.kind
                )));
            }
        }
    }
    Ok(())
}

const fn libcall_name(lc: cranelift_codegen::ir::LibCall) -> &'static str {
    use cranelift_codegen::ir::LibCall;
    match lc {
        LibCall::CeilF32 => "ceilf",
        LibCall::CeilF64 => "ceil",
        LibCall::FloorF32 => "floorf",
        LibCall::FloorF64 => "floor",
        LibCall::NearestF32 => "nearbyintf",
        LibCall::NearestF64 => "nearbyint",
        LibCall::TruncF32 => "truncf",
        LibCall::TruncF64 => "trunc",
        _ => "unknown_libcall",
    }
}

/// Resolve a libm function name to its actual address in the process.
///
/// Uses the pure-Rust `libm` crate instead of system libc for all math functions.
fn resolve_libm_address(name: &str) -> Result<usize, JitError> {
    let addr = match name {
        "sinf" => libm::sinf as *const () as usize,
        "cosf" => libm::cosf as *const () as usize,
        "tanhf" => libm::tanhf as *const () as usize,
        "exp2f" => libm::exp2f as *const () as usize,
        "log2f" => libm::log2f as *const () as usize,
        "sin" => libm::sin as *const () as usize,
        "cos" => libm::cos as *const () as usize,
        "exp2" => libm::exp2 as *const () as usize,
        "log2" => libm::log2 as *const () as usize,
        "ceilf" => libm::ceilf as *const () as usize,
        "ceil" => libm::ceil as *const () as usize,
        "floorf" => libm::floorf as *const () as usize,
        "floor" => libm::floor as *const () as usize,
        "truncf" => libm::truncf as *const () as usize,
        "trunc" => libm::trunc as *const () as usize,
        "nearbyintf" => libm::roundf as *const () as usize,
        "nearbyint" => libm::round as *const () as usize,
        _ => {
            return Err(JitError::Compilation(format!(
                "unknown libm function: {name}"
            )));
        }
    };
    Ok(addr)
}
