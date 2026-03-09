# 02 — Multi-Target Compile

Compile the same WGSL shader to every supported GPU target. No hardware
required — this demonstrates coralReef's cross-vendor compilation.

## What it shows

- One WGSL source → multiple native binaries (NVIDIA SM70–SM89, AMD RDNA2–RDNA4)
- Different instruction sets, different binary sizes, same semantics
- Known limitations documented inline (e.g. RDNA2 global_invocation_id)
- Cross-vendor parity as a continuous testing opportunity

## Run it

```bash
./demo.sh
```
