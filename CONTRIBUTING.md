# Contributing to coralReef

Thank you for your interest in coralReef — a sovereign Rust GPU compiler.

## Getting Started

```bash
# Prerequisites: Rust 1.85+ (edition 2024)
rustup update stable

# Clone and check
git clone https://github.com/ecoPrimals/coralReef.git
cd coralReef
cargo check --workspace
cargo test --workspace            # 1992 passing (+48 VFIO)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## Standards

coralReef follows ecoPrimals ecosystem conventions from `wateringHole/`.

- **License**: AGPL-3.0-only (see LICENSE). Upstream-derived files retain original attribution.
- **Linting**: `clippy::all` + `clippy::pedantic` + `missing_docs`, zero warnings
- **Formatting**: `cargo fmt` — no exceptions
- **Max file size**: 1000 lines
- **Test coverage**: 90%+ target (current: 57.54% line, measured with `cargo llvm-cov`; see `scripts/coverage.sh`)
- **Unsafe**: zero `unsafe` in new code
- **Error handling**: `Result<_, CompileError>` propagation; optimizer passes skip instead of panicking
- **No `panic!` in new production code**: use `?`, `.ok_or()`, `debug_assert!`, or graceful fallback
- **Naming**: Rust-idiomatic, vendor-neutral (see `CONVENTIONS.md`)

## Architecture

See `specs/CORALREEF_SPECIFICATION.md` and `START_HERE.md`.

Key module patterns:
- **Directory modules**: Large files are split into directories (`ir/`, `naga_translate/`, `lower_f64/`, `nv/sm70_encode/`)
- **Virtual ops**: f64 transcendentals use placeholder ops expanded by `lower_f64` before legalization
- **Pipeline**: `pipeline.rs` orchestrates the full compilation with `Result` propagation
- **Vendor backends**: `nv/` for NVIDIA, `amd/` for AMD, with `intel/` planned

## Commit Messages

Use conventional commits:

```
feat(compiler): add naga → codegen IR translation
fix(isa): correct SM70 DFMA latency table
refactor(stubs): replace BitSet HashSet with dense bitmap
test(core): add lifecycle error path coverage
docs(spec): update f64 lowering strategy
```

## Pull Requests

- One logical change per PR
- All checks must pass (`cargo check`, `clippy`, `fmt`, `test`)
- Coverage must not decrease
- New public APIs require doc comments with `# Errors` / `# Panics` sections

## IPC Standards

When implementing IPC endpoints, follow `wateringHole/SEMANTIC_METHOD_NAMING_STANDARD.md`:

- Method format: `{domain}.{operation}` (e.g. `shader.compile`, `shader.status`)
- JSON-RPC 2.0 as primary protocol
- tarpc (bincode serialization) as high-performance binary channel
