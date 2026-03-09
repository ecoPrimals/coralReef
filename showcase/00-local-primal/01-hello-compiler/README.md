# 01 — Hello Compiler

The simplest coralReef demo: compile a WGSL compute shader to a native
GPU binary. No GPU hardware required — this is pure compilation.

## What it shows

- coralReef compiles WGSL to native machine code (not SPIR-V, not intermediate)
- The output is real GPU instructions: NVIDIA SASS or AMD GCN/RDNA
- Compilation metadata: register count, instruction count, workgroup size
- All targets available without any vendor SDK installed

## Run it

```bash
./demo.sh
```
