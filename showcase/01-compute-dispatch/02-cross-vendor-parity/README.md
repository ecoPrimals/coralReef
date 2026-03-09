# 02 — Cross-Vendor Parity

The same WGSL shader compiled and dispatched on every GPU found on the
system. Verifies that different hardware produces identical compute results.

## What it shows

- Multi-GPU enumeration
- Same shader → different ISAs → same results
- Parity validation: the correctness contract holds across vendors
- Documents which targets succeed and which hit known compiler limitations

## Run it

Requires at least one GPU. Best on multi-GPU systems (e.g. AMD + NVIDIA):

```bash
./demo.sh
```
