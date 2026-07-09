import json, unicodedata, sys

with open('/home/lain/dso/model/tokenizer.json') as f:
    T = json.load(f)

vocab = T['model']['vocab']
merges = T['model']['merges']
added = {a['id']: a['content'] for a in T.get('added_tokens', [])}
all_vocab = dict(vocab)
for a in T.get('added_tokens', []):
    all_vocab[a['content']] = a['id']
id2tok = {v: k for k, v in vocab.items()}
id2tok.update(added)

ranks = {}
for i, m in enumerate(merges):
    if isinstance(m, list):
        a, b = m[0], m[1]
    else:
        a, b = m.split(' ')
    ranks[(a, b)] = i

PAT = r"(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+"

import regex as re

def bytes_to_unicode():
    bs = list(range(33, 127)) + list(range(161, 173)) + list(range(174, 256))
    cs = bs[:]
    n = 0
    for b in range(256):
        if b not in bs:
            bs.append(b); cs.append(256 + n); n += 1
    return {b: chr(c) for b, c in zip(bs, cs)}

B2U = bytes_to_unicode()
U2B = {v: k for k, v in B2U.items()}

def bpe(token_chars):
    word = list(token_chars)
    while True:
        best = None; bi = -1
        for i in range(len(word) - 1):
            r = ranks.get((word[i], word[i + 1]))
            if r is not None and (best is None or r < best):
                best = r; bi = i
        if bi == -1:
            break
        word[bi:bi + 2] = [word[bi] + word[bi + 1]]
    return word

def encode(text):
    text = unicodedata.normalize('NFC', text)
    ids = []
    for piece in re.findall(PAT, text):
        bl = ''.join(B2U[b] for b in piece.encode('utf-8'))
        for tok in bpe(bl):
            ids.append(vocab[tok])
    return ids

def decode(ids):
    out = []
    for i in ids:
        if i in added:
            out.append(added[i])
        else:
            out.append(id2tok.get(i, ''))
    s = ''.join(out)
    # byte-level decode
    bs = bytearray()
    for ch in s:
        if ch in U2B:
            bs.append(U2B[ch])
        else:
            bs.extend(ch.encode('utf-8'))
    return bs.decode('utf-8', errors='replace')

def build_chat(prompt, system="You are a helpful assistant."):
    ids = []
    for role, content in [("system", system), ("user", prompt)]:
        ids.append(all_vocab.get("<|im_start|>"))
        ids += encode(role + "\n" + content)
        ids.append(all_vocab.get("<|im_end|>"))
    ids.append(all_vocab.get("<|im_start|>"))
    ids += encode("assistant\n")
    return ids

if __name__ == '__main__':
    # standalone: tokenize stdin text -> ids
    text = sys.stdin.read()
    print(' '.join(map(str, encode(text))))
