# DSO-AI Runtime

Zero-malloc LLM inference engine: streaming from disk, INT8 quantization, AVX2 GEMM, static arenas.

See [root README](../README.md) for details.

## Quick start

```bash
g++ -O3 -fopenmp -std=c++17 dso_runtime.cpp -o dso_runtime
python3 quantize.py
OMP_NUM_THREADS=4 DSO_MODEL=model/model.dso python3 run.py "Hello" 64
```
