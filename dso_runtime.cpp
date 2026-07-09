// DSO-AI Runtime (v1) — Qwen2 inference engine
// Layer-by-layer streaming from disk, static memory arenas, OpenMP GEMM.
// Compile: g++ -O3 -fopenmp -std=c++17 dso_runtime.cpp -o dso_runtime
//
// Design (per DSO manifest modules 2-3, made real):
//   * Weights live in the safetensors file, mmap'd read-only.
//   * Exactly ONE weight matrix is resident in RAM at a time (Active Weight
//     Arena, weight_buf). Each projection is streamed from disk, converted
//     BF16->FP32 on the fly, used, then overwritten. Peak RSS is independent
//     of model size: one layer's biggest weight + activations + KV-cache.
//   * Embedding / lm_head are tiled: logits are computed by streaming vocab
//     rows from disk in blocks (tiling instead of holding the whole matrix).
//   * KV-cache is a single static buffer (ring semantics: position index mod
//     MAX_SEQ).
//   * ASYNC STREAMING (manifest module 3): a background thread issues
//     MADV_WILLNEED on the NEXT layer while the current layer is computed, and
//     MADV_DONTNEED evicts a layer's pages from the OS page-cache right after
//     use. So the page-cache holds only a sliding window of ~1-2 layers + embed,
//     never the whole model. Reads are cheap (no SSD wear; only writes wear).
//   * No heap allocation inside the token generation loop.

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstdint>
#include <cmath>
#include <vector>
#include <thread>
#include <atomic>
#include <mutex>
#include <condition_variable>
#include <chrono>
#if defined(__x86_64__) || defined(__i386__)
#include <immintrin.h>
#endif
#include <string>
#include <unordered_map>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <omp.h>

// ---- model constants (Qwen2.5-0.5B-Instruct) ----
static const int HIDDEN    = 896;
static const int INTER     = 4864;
static const int N_LAYERS  = 24;
static const int N_HEADS   = 14;
static const int N_KV      = 2;
static const int HEAD_DIM  = HIDDEN / N_HEADS; // 64
static const int VOCAB     = 151936;
static const float EPS     = 1e-6f;
static const float ROPE_THETA = 1000000.0f;
static const int MAX_SEQ   = 2048;
static const int N_GROUPS  = N_HEADS / N_KV;    // 7

// ---- bf16 -> f32 ----
static inline float bf16_to_f32(uint16_t h) {
    uint32_t u = (uint32_t)h << 16;
    float f; std::memcpy(&f, &u, 4); return f;
}

// ---- mmap'd model ----
static int   g_fd = -1;
static char* g_map = nullptr;
static size_t g_map_size = 0;
static uint64_t g_data_start = 0; // = 8 + header_len, absolute file offset of tensor data

struct TInfo { size_t off; std::vector<int> shape; };
static std::unordered_map<std::string, TInfo> g_tmap;
static const uint16_t* g_emb = nullptr; // embed_tokens BF16 rows [VOCAB, HIDDEN]

// ---- int8 (DSO) model mode ----
// WT: a tensor in the .dso file. kind 0 = fp32 blob, 1 = int8 blob + per-row fp32 scale.
struct WT { int kind; const void* data; const float* scale; int N, K; };
static std::unordered_map<std::string, WT> g_w;
static int g_mode = 0; // 0 = BF16 safetensors, 1 = INT8 .dso
static const int8_t* g_emb_i8 = nullptr;     // embed rows (int8) [VOCAB, HIDDEN]
static const float* g_emb_scale = nullptr;   // per-row scale [VOCAB]

// active weight arena (BF16 mode): one FP32 weight matrix at a time (max = down_proj)
// In INT8 mode weights are read directly as int8 from the mmap (1 byte/param).
static int8_t g_qx[4864]; // per-row INT8 activation buffer for the SIMD GEMM

// ---- static arenas (allocated ONCE at init, never inside the loop) ----
static float weight_buf[4864 * 896];          // one weight matrix at a time (max single = down_proj)
static float bias_buf[4864];                   // small bias scratch
static float x_buf[HIDDEN];                    // residual stream for current token
static float xn_buf[HIDDEN];                   // normalized
static float attn_out[HIDDEN];
static float ffn_out[HIDDEN];
static float q_buf[N_HEADS * HEAD_DIM];
static float k_buf[N_KV * HEAD_DIM];
static float v_buf[N_KV * HEAD_DIM];
static float scores[MAX_SEQ];
static float ctx_buf[N_HEADS * HEAD_DIM];
static float gate_buf[INTER];
static float up_buf[INTER];
static float act_buf[INTER];
static float logits[VOCAB];
// KV-cache: per layer, [MAX_SEQ][N_KV][HEAD_DIM]
static float* g_kcache; // [N_LAYERS*MAX_SEQ*N_KV*HEAD_DIM]
static float* g_vcache;

static float g_inv_freq[HEAD_DIM / 2];

// ---- parse safetensors header ----
static void parse_header() {
    uint64_t hlen;
    std::memcpy(&hlen, g_map, 8);
    g_data_start = 8 + hlen;
    std::string hdr(g_map + 8, hlen);

    size_t p = hdr.find('{') + 1;
    while (p < hdr.size()) {
        while (p < hdr.size() && (hdr[p]==' '||hdr[p]=='\n'||hdr[p]=='\r'||hdr[p]=='\t'||hdr[p]==',')) p++;
        if (p >= hdr.size() || hdr[p] == '}') break;
        if (hdr[p] != '"') { p++; continue; }
        p++;
        std::string key;
        while (p < hdr.size() && hdr[p] != '"') { key += hdr[p]; p++; }
        p++; // closing quote
        while (p < hdr.size() && hdr[p] != '{') p++;
        size_t obj_start = p;
        int depth = 0; size_t q = obj_start;
        for (; q < hdr.size(); q++) {
            if (hdr[q] == '{') depth++;
            else if (hdr[q] == '}') { depth--; if (depth == 0) break; }
        }
        std::string obj = hdr.substr(obj_start + 1, q - obj_start - 1);

        TInfo info; info.off = 0;
        size_t dpos = obj.find("\"data_offsets\"");
        if (dpos != std::string::npos) {
            size_t b1 = obj.find('[', dpos);
            size_t b2 = obj.find(',', b1);
            info.off = (size_t)strtoull(obj.substr(b1 + 1, b2 - b1 - 1).c_str(), nullptr, 10);
        }
        size_t spos = obj.find("\"shape\"");
        if (spos != std::string::npos) {
            size_t s1 = obj.find('[', spos);
            size_t s2 = obj.find(']', s1);
            std::string body = obj.substr(s1 + 1, s2 - s1 - 1);
            size_t i = 0;
            while (i < body.size()) {
                while (i < body.size() && (body[i] < '0' || body[i] > '9') && body[i] != '-') i++;
                if (i >= body.size()) break;
                size_t j = i;
                while (j < body.size() && ((body[j] >= '0' && body[j] <= '9') || body[j] == '-')) j++;
                info.shape.push_back(std::atoi(body.substr(i, j - i).c_str()));
                i = j;
            }
        }
        g_tmap[key] = info;
        p = q + 1;
    }
    auto it = g_tmap.find("model.embed_tokens.weight");
    if (it == g_tmap.end()) { fprintf(stderr, "embed_tokens not found\n"); exit(1); }
    g_emb = (const uint16_t*)(g_map + g_data_start + it->second.off);
}

static void load_weight(const std::string& name, float* dst) {
    auto it = g_tmap.find(name);
    if (it == g_tmap.end()) { fprintf(stderr, "MISSING tensor: %s\n", name.c_str()); exit(1); }
    const uint16_t* src = (const uint16_t*)(g_map + g_data_start + it->second.off);
    size_t n = 1; for (int s : it->second.shape) n *= (size_t)s;
    for (size_t i = 0; i < n; i++) dst[i] = bf16_to_f32(src[i]);
}
static void load_bias(const std::string& name, float* dst, int n) {
    auto it = g_tmap.find(name);
    if (it == g_tmap.end()) { fprintf(stderr, "MISSING bias: %s\n", name.c_str()); exit(1); }
    const uint16_t* src = (const uint16_t*)(g_map + g_data_start + it->second.off);
    for (int i = 0; i < n; i++) dst[i] = bf16_to_f32(src[i]);
}

// ---- parse the INT8 .dso file (header JSON: kind/shape/off/nbytes) ----
static long parse_int_after(const std::string& s, size_t pos) {
    size_t c = s.find(':', pos);
    if (c == std::string::npos) return 0;
    size_t i = c + 1;
    while (i < s.size() && (s[i] < '0' || s[i] > '9') && s[i] != '-') i++;
    size_t j = i;
    while (j < s.size() && ((s[j] >= '0' && s[j] <= '9') || s[j] == '-')) j++;
    return std::strtol(s.substr(i, j - i).c_str(), nullptr, 10);
}
static std::vector<int> parse_shape_in(const std::string& obj) {
    std::vector<int> sh;
    size_t sp = obj.find("\"shape\"");
    if (sp == std::string::npos) return sh;
    size_t s1 = obj.find('[', sp);
    size_t s2 = obj.find(']', s1);
    std::string body = obj.substr(s1 + 1, s2 - s1 - 1);
    size_t i = 0;
    while (i < body.size()) {
        while (i < body.size() && (body[i] < '0' || body[i] > '9') && body[i] != '-') i++;
        if (i >= body.size()) break;
        size_t j = i;
        while (j < body.size() && ((body[j] >= '0' && body[j] <= '9') || body[j] == '-')) j++;
        sh.push_back(std::atoi(body.substr(i, j - i).c_str()));
        i = j;
    }
    return sh;
}
static void parse_dso() {
    uint64_t hlen; std::memcpy(&hlen, g_map, 8);
    g_data_start = 8 + hlen;
    std::string hdr(g_map + 8, hlen);
    size_t p = hdr.find('{') + 1;
    while (p < hdr.size()) {
        while (p < hdr.size() && (hdr[p]==' '||hdr[p]=='\n'||hdr[p]=='\r'||hdr[p]=='\t'||hdr[p]==',')) p++;
        if (p >= hdr.size() || hdr[p] == '}') break;
        if (hdr[p] != '"') { p++; continue; }
        p++; std::string key; while (p < hdr.size() && hdr[p] != '"') { key += hdr[p]; p++; }
        p++;
        while (p < hdr.size() && hdr[p] != '{') p++;
        size_t obj_start = p;
        int depth = 0; size_t q = obj_start;
        for (; q < hdr.size(); q++) {
            if (hdr[q] == '{') depth++;
            else if (hdr[q] == '}') { depth--; if (depth == 0) break; }
        }
        std::string obj = hdr.substr(obj_start + 1, q - obj_start - 1);
        WT w; w.kind = 0; w.scale = nullptr; w.N = 0; w.K = 1;
        size_t kp = obj.find("\"kind\"");
        if (kp != std::string::npos) {
            size_t c1 = obj.find('"', kp + 6);
            size_t c2 = obj.find('"', c1 + 1);
            std::string kind = obj.substr(c1 + 1, c2 - c1 - 1);
            w.kind = (kind == "int8") ? 1 : 0;
        }
        long off = parse_int_after(obj, obj.find("\"off\""));
        std::vector<int> sh = parse_shape_in(obj);
        w.N = sh.empty() ? 0 : sh[0];
        w.K = sh.size() > 1 ? sh[1] : 1;
        w.data = g_map + (size_t)off;   // .dso offsets are absolute file offsets
        if (w.kind == 1) w.scale = (const float*)(g_map + (size_t)off + (size_t)w.N * w.K);
        g_w[key] = w;
        if (key == "model.embed_tokens.weight") { g_emb_i8 = (const int8_t*)w.data; g_emb_scale = w.scale; }
        p = q + 1;
    }
}

// ---- INT8 GEMM: Y[i,j] = scale[j] * sum_k X[i,k] * Wq[j,k]  (Wq int8) ----
static void linear_i8(const float* X, int M, int K, const int8_t* W, const float* S, int N, float* Y, int nt) {
    if (M == 1) {
        #pragma omp parallel for schedule(static) num_threads(nt)
        for (int j = 0; j < N; j++) {
            const int8_t* wj = W + (size_t)j * K;
            float acc = 0.0f;
            for (int k = 0; k < K; k++) acc += (float)X[k] * (float)wj[k];
            Y[j] = acc * S[j];
        }
    } else {
        #pragma omp parallel for schedule(static) num_threads(nt)
        for (int i = 0; i < M; i++) {
            const float* xi = X + (size_t)i * K;
            float* yi = Y + (size_t)i * N;
            for (int j = 0; j < N; j++) {
                const int8_t* wj = W + (size_t)j * K;
                float acc = 0.0f;
                for (int k = 0; k < K; k++) acc += (float)xi[k] * (float)wj[k];
                yi[j] = acc * S[j];
            }
        }
    }
}

// ---- unified weight accessors (both modes) ----
static void linear(const float* X, int M, int K, const float* W, int N, float* Y, int nt); // fwd decl
#if defined(__x86_64__) || defined(__i386__)
__attribute__((target("avx2")))
#endif
static void linear_i8_avx2(const float* X, int M, int K, const int8_t* W, const float* S, int N, float* Y, int nt); // fwd decl
static const float* norm_weight(const std::string& name) {
    if (g_mode == 0) { load_weight(name, weight_buf); return weight_buf; }
    auto it = g_w.find(name);
    if (it == g_w.end()) { fprintf(stderr, "MISSING %s\n", name.c_str()); exit(1); }
    return (const float*)it->second.data;
}
static void proj(const std::string& wname, const float* X, int M, int K, int N, float* Y, int nt,
                 const std::string& bname = "") {
    if (g_mode == 0) {
        load_weight(wname, weight_buf);
        linear(X, M, K, weight_buf, N, Y, nt);
        if (!bname.empty()) {
            auto sh = g_tmap[bname].shape; int n = 1; for (int s : sh) n *= s;
            load_bias(bname, bias_buf, n);
            for (int i = 0; i < N; i++) Y[i] += bias_buf[i];
        }
    } else {
        WT& w = g_w[wname];
#if defined(__x86_64__) || defined(__i386__)
        linear_i8_avx2(X, M, K, (const int8_t*)w.data, w.scale, N, Y, nt);
#else
        linear_i8(X, M, K, (const int8_t*)w.data, w.scale, N, Y, nt);
#endif
        if (!bname.empty()) {
            WT& b = g_w[bname];
            const float* bp = (const float*)b.data;
            for (int i = 0; i < N; i++) Y[i] += bp[i];
        }
    }
}

// ---- async streaming (manifest module 3): madvise-driven page-cache window ----
static void madvise_tensor(const std::string& name, int advice) {
    if (g_mode == 0) {
        auto it = g_tmap.find(name);
        if (it == g_tmap.end()) return;
        size_t nbytes = 2; // BF16
        for (int s : it->second.shape) nbytes *= (size_t)s;
        madvise(g_map + g_data_start + it->second.off, nbytes, advice);
    } else {
        auto it = g_w.find(name);
        if (it == g_w.end()) return;
        const WT& w = it->second;
        size_t nbytes = (size_t)w.N * w.K;
        if (w.kind == 1) nbytes += (size_t)w.N * sizeof(float); // + scales
        madvise(const_cast<void*>(w.data), nbytes, advice);
    }
}
static void layer_madvise(int L, int advice) {
    const char* parts[] = {
        "input_layernorm.weight", "post_attention_layernorm.weight",
        "self_attn.q_proj.weight", "self_attn.q_proj.bias",
        "self_attn.k_proj.weight", "self_attn.k_proj.bias",
        "self_attn.v_proj.weight", "self_attn.v_proj.bias",
        "self_attn.o_proj.weight",
        "mlp.gate_proj.weight", "mlp.up_proj.weight", "mlp.down_proj.weight"
    };
    for (auto p : parts) {
        std::string nm = "model.layers." + std::to_string(L) + "." + p;
        madvise_tensor(nm, advice);
    }
}
static std::mutex g_sm;
static std::condition_variable g_cv;
static int g_pending = -1;     // layer index the worker should WILLNEED (-1 = idle)
static bool g_stop = false;
static std::thread g_worker;
static void stream_worker() {
    while (true) {
        std::unique_lock<std::mutex> lk(g_sm);
        g_cv.wait(lk, [] { return g_stop || g_pending != -1; });
        if (g_stop) return;
        int L = g_pending; g_pending = -1;
        lk.unlock();
        if (L >= 0 && L < N_LAYERS) layer_madvise(L, MADV_WILLNEED);
    }
}
static void request_prefetch(int L) {
    std::lock_guard<std::mutex> lk(g_sm);
    g_pending = L; g_cv.notify_one();
}

// ---- linear: Y[M,N] = X[M,K] @ W^T, W row-major [N,K] ----
static void linear(const float* X, int M, int K, const float* W, int N, float* Y, int nt) {
    if (M == 1) {
        #pragma omp parallel for schedule(static) num_threads(nt)
        for (int j = 0; j < N; j++) {
            const float* wj = W + (size_t)j * K;
            float s = 0.0f;
            for (int k = 0; k < K; k++) s += X[k] * wj[k];
            Y[j] = s;
        }
    } else {
        // Row-block partitioning over M (used for prefill with M>1)
        #pragma omp parallel for schedule(static) num_threads(nt)
        for (int i = 0; i < M; i++) {
            const float* xi = X + (size_t)i * K;
            float* yi = Y + (size_t)i * N;
            for (int j = 0; j < N; j++) {
                const float* wj = W + (size_t)j * K;
                float s = 0.0f;
                for (int k = 0; k < K; k++) s += xi[k] * wj[k];
                yi[j] = s;
            }
        }
    }
}

// ---- AVX2 INT8 GEMM: X (float) @ Wq (int8)^T, per-row dynamic activation quant ----
// Y[i,j] = sx_i * S[j] * sum_k qx_i[k] * Wq[j,k],  qx = round(X / sx), sx = maxabs(X)/127
#if defined(__x86_64__) || defined(__i386__)
__attribute__((target("avx2")))
static void linear_i8_avx2(const float* X, int M, int K, const int8_t* W, const float* S, int N, float* Y, int nt) {
    int K16 = K - (K % 16);
    for (int i = 0; i < M; i++) {
        const float* xi = X + (size_t)i * K;
        float mx = 0.0f;
        for (int k = 0; k < K; k++) { float a = fabsf(xi[k]); if (a > mx) mx = a; }
        float sx = (mx > 0.0f) ? mx / 127.0f : 1.0f;
        float inv_sx = 1.0f / sx;
        for (int k = 0; k < K; k++) {
            int v = (int)lrintf(xi[k] * inv_sx);
            v = v < -127 ? -127 : (v > 127 ? 127 : v);
            g_qx[k] = (int8_t)v;
        }
        float* yi = Y + (size_t)i * N;
        #pragma omp parallel for schedule(static) num_threads(nt)
        for (int j = 0; j < N; j++) {
            const int8_t* wj = W + (size_t)j * K;
            __m256i acc = _mm256_setzero_si256();
            int k = 0;
            for (; k < K16; k += 16) {
                __m256i a = _mm256_cvtepi8_epi16(_mm_loadu_si128((const __m128i*)&g_qx[k]));
                __m256i b = _mm256_cvtepi8_epi16(_mm_loadu_si128((const __m128i*)&wj[k]));
                acc = _mm256_add_epi32(acc, _mm256_madd_epi16(a, b));
            }
            int32_t s32 = 0;
            for (int t = 0; t < 8; t++) s32 += _mm256_extract_epi32(acc, t);
            for (; k < K; k++) s32 += (int32_t)g_qx[k] * (int32_t)wj[k];
            yi[j] = sx * S[j] * (float)s32;
        }
    }
}
#endif

static void rmsnorm(float* o, const float* x, const float* w, int n) {
    float ss = 0.0f;
    #pragma omp parallel for reduction(+:ss)
    for (int i = 0; i < n; i++) ss += x[i] * x[i];
    float r = 1.0f / sqrtf(ss / n + EPS);
    #pragma omp parallel for
    for (int i = 0; i < n; i++) o[i] = x[i] * r * w[i];
}

static inline float silu(float z) { return z / (1.0f + expf(-z)); }

static void rope(float* vec, int n_heads, int pos) {
    for (int h = 0; h < n_heads; h++) {
        float* base = vec + h * HEAD_DIM;
        for (int i = 0; i < HEAD_DIM / 2; i++) {
            float ang = g_inv_freq[i] * (float)pos;
            float c = cosf(ang), s = sinf(ang);
            float x0 = base[i];
            float x1 = base[i + HEAD_DIM / 2];
            base[i] = x0 * c - x1 * s;
            base[i + HEAD_DIM / 2] = x0 * s + x1 * c;
        }
    }
}

// forward one token at position `pos`; updates x_buf in place and appends KV.
static void forward_token(int token, int pos, int nt) {
    // embed
    if (g_mode == 0) {
        const uint16_t* row = g_emb + (size_t)token * HIDDEN;
        for (int i = 0; i < HIDDEN; i++) x_buf[i] = bf16_to_f32(row[i]);
    } else {
        const int8_t* row = g_emb_i8 + (size_t)token * HIDDEN;
        float sv = g_emb_scale[token];
        for (int i = 0; i < HIDDEN; i++) x_buf[i] = sv * (float)row[i];
    }

    for (int L = 0; L < N_LAYERS; L++) {
        // async: while we compute layer L, the worker prefetches layer L+1
        request_prefetch(L + 1);
        // --- self-attention ---
        rmsnorm(xn_buf, x_buf, norm_weight("model.layers." + std::to_string(L) + ".input_layernorm.weight"), HIDDEN);
        proj("model.layers." + std::to_string(L) + ".self_attn.q_proj.weight",
             xn_buf, 1, HIDDEN, N_HEADS * HEAD_DIM, q_buf, nt,
             "model.layers." + std::to_string(L) + ".self_attn.q_proj.bias");
        proj("model.layers." + std::to_string(L) + ".self_attn.k_proj.weight",
             xn_buf, 1, HIDDEN, N_KV * HEAD_DIM, k_buf, nt,
             "model.layers." + std::to_string(L) + ".self_attn.k_proj.bias");
        proj("model.layers." + std::to_string(L) + ".self_attn.v_proj.weight",
             xn_buf, 1, HIDDEN, N_KV * HEAD_DIM, v_buf, nt,
             "model.layers." + std::to_string(L) + ".self_attn.v_proj.bias");
        rope(q_buf, N_HEADS, pos);
        rope(k_buf, N_KV, pos);

        // append K,V to cache
        float* kc = g_kcache + ((size_t)L * MAX_SEQ + pos) * N_KV * HEAD_DIM;
        float* vc = g_vcache + ((size_t)L * MAX_SEQ + pos) * N_KV * HEAD_DIM;
        std::memcpy(kc, k_buf, N_KV * HEAD_DIM * sizeof(float));
        std::memcpy(vc, v_buf, N_KV * HEAD_DIM * sizeof(float));

        // attention
        float scale = 1.0f / sqrtf((float)HEAD_DIM);
        for (int h = 0; h < N_HEADS; h++) {
            int g = h / N_GROUPS;
            float* qh = q_buf + h * HEAD_DIM;
            float* kc_g = g_kcache + ((size_t)L * MAX_SEQ) * N_KV * HEAD_DIM + (size_t)g * HEAD_DIM;
            float* vc_g = g_vcache + ((size_t)L * MAX_SEQ) * N_KV * HEAD_DIM + (size_t)g * HEAD_DIM;
            float maxs = -1e30f;
            for (int p = 0; p <= pos; p++) {
                const float* kp = kc_g + (size_t)p * N_KV * HEAD_DIM;
                float s = 0.0f;
                for (int d = 0; d < HEAD_DIM; d++) s += qh[d] * kp[d];
                s *= scale;
                scores[p] = s;
                if (s > maxs) maxs = s;
            }
            float sum = 0.0f;
            for (int p = 0; p <= pos; p++) { scores[p] = expf(scores[p] - maxs); sum += scores[p]; }
            float inv = 1.0f / sum;
            float* ch = ctx_buf + h * HEAD_DIM;
            for (int d = 0; d < HEAD_DIM; d++) ch[d] = 0.0f;
            for (int p = 0; p <= pos; p++) {
                const float* vp = vc_g + (size_t)p * N_KV * HEAD_DIM;
                float w = scores[p] * inv;
                for (int d = 0; d < HEAD_DIM; d++) ch[d] += w * vp[d];
            }
        }
        proj("model.layers." + std::to_string(L) + ".self_attn.o_proj.weight",
             ctx_buf, 1, HIDDEN, HIDDEN, attn_out, nt);
        for (int i = 0; i < HIDDEN; i++) x_buf[i] += attn_out[i];

        // --- mlp ---
        rmsnorm(xn_buf, x_buf, norm_weight("model.layers." + std::to_string(L) + ".post_attention_layernorm.weight"), HIDDEN);
        proj("model.layers." + std::to_string(L) + ".mlp.gate_proj.weight",
             xn_buf, 1, HIDDEN, INTER, gate_buf, nt);
        proj("model.layers." + std::to_string(L) + ".mlp.up_proj.weight",
             xn_buf, 1, HIDDEN, INTER, up_buf, nt);
        for (int i = 0; i < INTER; i++) act_buf[i] = silu(gate_buf[i]) * up_buf[i];
        proj("model.layers." + std::to_string(L) + ".mlp.down_proj.weight",
             act_buf, 1, INTER, HIDDEN, ffn_out, nt);
        for (int i = 0; i < HIDDEN; i++) x_buf[i] += ffn_out[i];

        // async: release this layer's pages from the OS page-cache (sliding window)
        layer_madvise(L, MADV_DONTNEED);
    }

    // final norm
    rmsnorm(xn_buf, x_buf, norm_weight("model.norm.weight"), HIDDEN);

    // lm_head = tie(embed_tokens): logits[v] = dot(xn, emb[v])  (tiled streaming)
    const int BLOCK = 4096;
    #pragma omp parallel for schedule(dynamic) num_threads(nt)
    for (int v0 = 0; v0 < VOCAB; v0 += BLOCK) {
        int b = (v0 + BLOCK > VOCAB) ? (VOCAB - v0) : BLOCK;
        if (g_mode == 0) {
            for (int j = 0; j < b; j++) {
                const uint16_t* r = g_emb + (size_t)(v0 + j) * HIDDEN;
                float s = 0.0f;
                for (int k = 0; k < HIDDEN; k++) s += xn_buf[k] * bf16_to_f32(r[k]);
                logits[v0 + j] = s;
            }
        } else {
            for (int j = 0; j < b; j++) {
                const int8_t* r = g_emb_i8 + (size_t)(v0 + j) * HIDDEN;
                float sv = g_emb_scale[v0 + j];
                float s = 0.0f;
                for (int k = 0; k < HIDDEN; k++) s += xn_buf[k] * (float)r[k];
                logits[v0 + j] = sv * s;
            }
        }
    }
}

static int argmax(const float* a, int n) {
    int best = 0; float bv = a[0];
    for (int i = 1; i < n; i++) if (a[i] > bv) { bv = a[i]; best = i; }
    return best;
}

int main(int argc, char** argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s <prompt_tokens_file> [max_new_tokens]\n", argv[0]);
        fprintf(stderr, "  env DSO_MODEL: path to .safetensors (BF16) or .dso (INT8)\n");
        return 1;
    }
    const char* tok_path = argv[1];
    int max_new = (argc > 2) ? atoi(argv[2]) : 64;

    // mmap model
    const char* model_path = "/home/lain/dso/model/model.safetensors";
    const char* env_model = getenv("DSO_MODEL");
    if (env_model) model_path = env_model;
    g_mode = (strstr(model_path, ".dso") != nullptr) ? 1 : 0;
    g_fd = open(model_path, O_RDONLY);
    if (g_fd < 0) { perror("open model"); return 1; }
    struct stat st; fstat(g_fd, &st); g_map_size = st.st_size;
    g_map = (char*)mmap(nullptr, g_map_size, PROT_READ, MAP_PRIVATE, g_fd, 0);
    if (g_map == MAP_FAILED) { perror("mmap"); return 1; }
    if (g_mode == 0) parse_header(); else parse_dso();

    // async streaming: prefetch embedding matrix into page-cache, start worker
    madvise_tensor("model.embed_tokens.weight", MADV_WILLNEED);
    g_worker = std::thread(stream_worker);

    // alloc static KV cache once
    size_t kv_elems = (size_t)N_LAYERS * MAX_SEQ * N_KV * HEAD_DIM;
    g_kcache = (float*)malloc(kv_elems * sizeof(float));
    g_vcache = (float*)malloc(kv_elems * sizeof(float));

    // precompute rope inv_freq
    for (int i = 0; i < HEAD_DIM / 2; i++)
        g_inv_freq[i] = 1.0f / powf(ROPE_THETA, (2.0f * i) / HEAD_DIM);

    // read prompt token ids (space separated)
    FILE* pf = fopen(tok_path, "r");
    if (!pf) { perror("open prompt tokens"); return 1; }
    std::vector<int> prompt;
    int t;
    while (fscanf(pf, "%d", &t) == 1) prompt.push_back(t);
    fclose(pf);
    if (prompt.empty()) { fprintf(stderr, "empty prompt\n"); return 1; }

    int nt = omp_get_max_threads();
    fprintf(stderr, "[dso] threads=%d prompt_len=%zu max_new=%d\n", nt, prompt.size(), max_new);

    // ---- prefill (process prompt, build KV) ----
    int pos = 0;
    for (size_t i = 0; i < prompt.size(); i++) {
        forward_token(prompt[i], pos, nt);
        pos++;
    }
    // first generated token from last logits
    int next = argmax(logits, VOCAB);
    std::vector<int> out;
    out.push_back(next);

    // ---- decode loop ----
    for (int step = 1; step < max_new; step++) {
        forward_token(next, pos, nt);
        pos++;
        next = argmax(logits, VOCAB);
        out.push_back(next);
        if (next == 151645 && !getenv("DSO_NOEOS")) break; // eos_token_id (unless benchmarking)
    }

    // stop async worker
    {
        std::lock_guard<std::mutex> lk(g_sm);
        g_stop = true; g_cv.notify_one();
    }
    if (g_worker.joinable()) g_worker.join();

    // emit generated token ids
    for (size_t i = 0; i < out.size(); i++) printf("%d%c", out[i], (i+1<out.size()?' ':'\n'));
    return 0;
}
