import json, struct, numpy as np

SRC = '/home/lain/dso/model/model.safetensors'
DST = '/home/lain/dso/model/model.dso'

f = open(SRC, 'rb')
hlen = struct.unpack('<Q', f.read(8))[0]
hdr = json.loads(f.read(hlen))
dstart = 8 + hlen
tensors = {k: v for k, v in hdr.items() if k != '__metadata__'}

def read_f32(name):
    v = tensors[name]
    off = v['data_offsets'][0]
    n = 1
    for s in v['shape']:
        n *= s
    raw = np.memmap(SRC, dtype=np.uint16, mode='r', offset=dstart + off, shape=(n,))
    u = raw.astype(np.uint32) << 16
    return u.view(np.float32).reshape(v['shape'])

INT8 = set()
for k, v in tensors.items():
    if v['dtype'] != 'BF16':
        continue
    if len(v['shape']) == 2 and ('proj' in k or 'mlp' in k or 'embed' in k):
        INT8.add(k)

# build blobs + per-tensor metadata
meta = []  # (name, kind, shape, blob_bytes)
for k, v in tensors.items():
    shp = v['shape']
    W = read_f32(k)
    if k in INT8:
        N, K = shp[0], shp[1]
        Wr = W.reshape(N, K)
        maxabs = np.max(np.abs(Wr), axis=1)
        scale = np.where(maxabs > 0, maxabs / 127.0, 1.0).astype(np.float32)
        q = np.clip(np.round(Wr / scale[:, None]), -127, 127).astype(np.int8)
        blob = q.tobytes() + scale.tobytes()
        meta.append((k, 'int8', shp, blob))
    else:
        blob = W.astype(np.float32).tobytes()
        meta.append((k, 'fp32', shp, blob))

# stable offset assignment (header length depends on off digit counts)
header = {k: {'kind': kind, 'shape': shp, 'off': 0, 'nbytes': len(blob)}
          for (k, kind, shp, blob) in meta}
for _ in range(6):
    hlen = len(json.dumps(header).encode())
    off = 8 + hlen
    for (k, kind, shp, blob) in meta:
        header[k]['off'] = off
        off += len(blob)
hdr_json = json.dumps(header).encode()

with open(DST, 'wb') as o:
    o.write(struct.pack('<Q', len(hdr_json)))
    o.write(hdr_json)
    for (k, kind, shp, blob) in meta:
        o.write(blob)

# self-check: first blob must start exactly at 8 + len(hdr_json)
import os
first_off = header[meta[0][0]]['off']
assert first_off == 8 + len(hdr_json), (first_off, 8 + len(hdr_json))
assert os.path.getsize(DST) == first_off + sum(len(b) for (_, _, _, b) in meta)

import os
print(f"wrote {DST} ({os.path.getsize(DST)/1024/1024:.1f} MB)")
print(f"int8 tensors: {len(INT8)} / {len(tensors)}")
print(f"BF16 safetensors: {os.path.getsize(SRC)/1024/1024:.1f} MB")
print(f"INT8 .dso:        {os.path.getsize(DST)/1024/1024:.1f} MB")
