# coralReef fuzz targets

Coverage-guided fuzzing uses [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer). Install the tool once:

```sh
cargo install cargo-fuzz
```

Run each target from this directory (`fuzz/`) with a nightly toolchain (required by cargo-fuzz):

```sh
cargo +nightly fuzz run fuzz_wgsl
cargo +nightly fuzz run fuzz_spirv
cargo +nightly fuzz run fuzz_jsonrpc
```

## Targets

| Binary        | What it exercises |
|---------------|-------------------|
| `fuzz_wgsl`   | `coral_reef::compile_wgsl` on arbitrary bytes as lossy UTF-8, for NVIDIA `sm_70` and AMD `rdna2`. |
| `fuzz_spirv`  | `coral_reef::compile` on arbitrary bytes interpreted as little-endian SPIR-V words (length truncated to a multiple of four), same two architectures. |
| `fuzz_jsonrpc`| `coralreef_core::ipc::dispatch` with arbitrary JSON strings (parsed requests or fallback method routing). |

`fuzz_jsonrpc` exercises the same Unix JSON-RPC router as the newline-delimited socket server (`dispatch` is Unix-only in `coralreef-core`).

Panics inside the fuzz harness are caught so a single crash does not stop the fuzzer; libFuzzer still records inputs that trigger real bugs when you remove the catch (or use `-O` / sanitizers as appropriate).
