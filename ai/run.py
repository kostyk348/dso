import sys, subprocess, os

sys.path.insert(0, '/home/lain/dso')
from tok import build_chat, decode

# CPU throttle: OMP_NUM_THREADS caps how many cores the GEMM uses.
# 16 cores -> 4 threads ~= 25% load, 3 ~= 19%. Set via env or 2nd CLI arg style.
threads = os.environ.get('OMP_NUM_THREADS') or '4'
os.environ['OMP_NUM_THREADS'] = threads

prompt = sys.argv[1] if len(sys.argv) > 1 else "The capital of France is"
max_new = int(sys.argv[2]) if len(sys.argv) > 2 else 64

ids = build_chat(prompt)
tokfile = '/tmp/opencode/prompt.tok'
with open(tokfile, 'w') as f:
    f.write(' '.join(map(str, ids)))

r = subprocess.run(['/home/lain/dso/dso_runtime', tokfile, str(max_new)],
                   capture_output=True, text=True)
if r.returncode != 0:
    print("RUNTIME ERROR:", r.stderr, file=sys.stderr)
    sys.exit(1)

gen = [int(x) for x in r.stdout.split()]
print("=== PROMPT IDS:", len(ids), "===")
print("=== GENERATED", len(gen), "tokens ===")
# strip the leading chat prefix we fed? We only feed prompt; generated are new tokens.
text = decode(gen)
print(text)
