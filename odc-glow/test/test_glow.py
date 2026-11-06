#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
"""End-to-end tests for ODC Glow through photocraft-cli (headless PhotoCraft).
Run ./build.sh first, then: python3 test/test_glow.py [path/to/photo.jpg]

The reference is written here in numpy (an independent implementation of the same formulas with an exact,
edge-clamped triple box blur), so a match proves the sandbox maths. Blur radii up to 8 px are exact; larger ones
are blurred at reduced resolution inside the plug-in, so those checks allow a few levels at the picture's edge and
require a small mean difference. The big photo is cut into bands by the host: matching the whole-image reference
proves there are no seams."""
import json, os, shutil, subprocess, sys, tempfile, time
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
WASM = os.path.join(HERE, '..', 'dist', 'odc-glow.wasm')
CLI = os.environ.get('PHOTOCRAFT_CLI') or shutil.which('photocraft-cli') or '/Applications/PhotoCraft.app/Contents/MacOS/photocraft-cli'
PHOTO = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, '..', 'examples', 'original.jpg')
TMP = tempfile.mkdtemp(prefix='odc-glow-')
ID = 'org.opendesignercloud.glow'
fails = []
BLEND = {'Screen': 0, 'Soft Light': 1, 'Overlay': 2, 'Lighten': 3, 'Normal': 4}
LOOKS = {  # name: (blend, radius, amount, threshold, contrast, saturation, warmth, brightness, protect)
    'Orton Classic': ('Overlay', 22, 50, 0, 25, 18, 0, 6, 0), 'Custom': ('Screen', 20, 0, 0, 0, 0, 0, 0, 0),
    'Glamour Glow': ('Screen', 14, 38, 0, -25, -10, 15, 0, 35), 'Soft Focus': ('Normal', 9, 55, 0, 0, 0, 0, 0, 0),
    'Pro-Mist': ('Screen', 30, 70, 52, 0, 0, 10, 0, 0), 'Dreamy Haze': ('Screen', 45, 45, 20, -30, -15, -18, 10, 20),
    'Golden Glow': ('Soft Light', 26, 70, 40, 0, 18, 55, 6, 0)}


def run(src, params, out):
    cmd = [CLI, 'run', src, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
           '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': params}), '--out', out]
    t = time.time()
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0: raise SystemExit('photocraft-cli failed:\n' + r.stdout[-300:] + r.stderr)
    return np.asarray(Image.open(out).convert('RGB'), dtype=np.float64) / 255, time.time() - t


def box1(a, r, axis):
    n = a.shape[axis]
    a = np.moveaxis(a, axis, 0)
    pad = np.concatenate([np.repeat(a[:1], r, 0), a, np.repeat(a[-1:], r, 0)], 0)
    c = np.cumsum(pad, 0); c = np.concatenate([np.zeros_like(c[:1]), c], 0)
    out = (c[2 * r + 1:2 * r + 1 + n] - c[:n]) / (2 * r + 1)
    return np.moveaxis(out, 0, axis)


def blur3(a, r):
    for _ in range(3):
        a = box1(box1(a, r, 1), r, 0)
    return a


def smooth(a, b, x):
    t = np.clip((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t)


def blend(a, b, m):
    if m == 'Screen': return 1 - (1 - a) * (1 - b)
    if m == 'Soft Light': return (1 - 2 * b) * a * a + 2 * a * b
    if m == 'Overlay': return np.where(a < 0.5, 2 * a * b, 1 - 2 * (1 - a) * (1 - b))
    if m == 'Lighten': return np.maximum(a, b)
    return b


def luma(c): return 0.299 * c[..., 0] + 0.587 * c[..., 1] + 0.114 * c[..., 2]


def reference(img8, look, **adj):
    bl, rad, amt, thr, con, sat, warm, bri, prot = LOOKS[look]
    rad = np.clip(rad + adj.get('radius', 0), 1, 120); amt = np.clip(amt + adj.get('amount', 0), 0, 100) / 100
    thr = np.clip(thr + adj.get('threshold', 0), 0, 100) / 100
    con = 1 + np.clip(con + adj.get('contrast', 0), -100, 100) / 100; sat = 1 + np.clip(sat + adj.get('saturation', 0), -100, 100) / 100
    warm = np.clip(warm + adj.get('warmth', 0), -100, 100) / 100 * 0.12; bri = 1 + np.clip(bri + adj.get('brightness', 0), -100, 100) / 100
    prot = np.clip(prot + adj.get('protect', 0), 0, 100) / 100
    bl = adj.get('blend', bl)
    a = img8 / 255.0; H, W = a.shape[:2]
    r = int(np.clip(np.floor(rad * min(W, H) / 1000 + 0.5), 1, 85))
    wgt = smooth(thr, max(min(thr + 0.25, 1.0), thr + 0.01), luma(a)) if thr > 0 else np.ones((H, W))
    g = blur3(a * wgt[..., None], r)
    gl = luma(g)[..., None] * bri
    v = g * bri; v = gl + (v - gl) * sat
    v[..., 0] += warm; v[..., 2] -= warm
    g = np.clip((v - 0.5) * con + 0.5, 0, 1)
    k = amt * (1 - prot * (1 - smooth(0, 0.5, luma(a))))[..., None]
    out = a + (blend(a, g, bl) - a) * k
    s = adj.get('strength', 100) / 100
    return np.clip(a + (out - a) * s, 0, 1)


def check(name, ok, detail=''):
    print(('PASS ' if ok else 'FAIL ') + name + (' ' + detail if detail else ''))
    if not ok: fails.append(name)


def compare(name, src, img8, look='Custom', tol=1.0, mean_tol=None, **adj):
    params = dict(look=look, **adj)
    o, secs = run(src, params, os.path.join(TMP, 'o.png'))
    d = np.abs(o - reference(img8, look, **adj)) * 255
    ok = d.max() <= tol and (mean_tol is None or d.mean() <= mean_tol)
    check(name, ok, f'max diff {d.max():.2f} levels, mean {d.mean():.3f} ({secs:.1f}s)')
    return o


photo = Image.open(PHOTO).convert('RGB')
psrc = os.path.join(TMP, 'photo.png'); photo.save(psrc)
p8 = np.asarray(photo); H, W = p8.shape[:2]
print(f'photo {W}x{H}, cli {CLI}')
small = photo.resize((420, 280), Image.LANCZOS)
ssrc = os.path.join(TMP, 'small.png'); small.save(ssrc); s8 = np.asarray(small)

# 1. no glow: output equals input
o, _ = run(ssrc, dict(look='Custom'), os.path.join(TMP, 'id.png'))
check('identity when amount is 0', np.abs(o * 255 - s8).max() <= 1)

# 2. exact-blur path (radius 8 px or less on this picture: 280 px short side, so radius per mille <= 28)
for bl in BLEND:
    compare(f'blend {bl} (radius 20 = 6 px)', ssrc, s8, blend=bl, amount=60, radius=0)
compare('threshold weights the highlights', ssrc, s8, amount=70, threshold=50, radius=0)
compare('contrast, saturation, brightness of the glow', ssrc, s8, amount=70, contrast=40, saturation=-50, brightness=20)
compare('warmth', ssrc, s8, amount=70, warmth=60)
compare('protect shadows', ssrc, s8, amount=80, protect=70)
compare('everything at once', ssrc, s8, amount=75, threshold=30, contrast=-20, saturation=25, warmth=-30, brightness=15, protect=40, blend='Soft Light')

# 3. every look, against the reference (the larger blurs are reduced-resolution: edge tolerance, small mean)
for lk in [k for k in LOOKS if k != 'Custom']:
    compare(f'look "{lk}"', ssrc, s8, look=lk, tol=8.0, mean_tol=0.8)

# 4. strength mixes with the original
o50, _ = run(ssrc, dict(look='Soft Focus', strength=50), os.path.join(TMP, 's50.png'))
o100, _ = run(ssrc, dict(look='Soft Focus'), os.path.join(TMP, 's100.png'))
check('strength 50 is halfway to the original', np.abs(o50 - (o100 + s8 / 255) / 2).max() * 255 <= 1.5)

# 5. the big photo is cut into bands: must match the whole-image reference (no seams)
for lk in ['Pro-Mist', 'Orton Classic']:
    compare(f'seamless across bands ("{lk}", full photo)', psrc, p8, look=lk, tol=8.0, mean_tol=0.8)

# 6. alpha is kept
rgba = Image.open(ssrc).convert('RGBA'); a = np.asarray(rgba).copy(); a[:, :, 3] = np.linspace(255, 40, a.shape[1]).astype(np.uint8)[None, :]
Image.fromarray(a).save(os.path.join(TMP, 'rgba.png'))
run(os.path.join(TMP, 'rgba.png'), dict(look='Glamour Glow'), os.path.join(TMP, 'rgba_o.png'))
oa = np.asarray(Image.open(os.path.join(TMP, 'rgba_o.png')).convert('RGBA'))
check('alpha channel is kept', np.abs(oa[:, :, 3].astype(int) - a[:, :, 3]).max() <= 1)

# 7. default look, grayscale, speed on a big picture
od, _ = run(ssrc, {}, os.path.join(TMP, 'def.png')); ov, _ = run(ssrc, dict(look='Orton Classic'), os.path.join(TMP, 'oc.png'))
check('default parameters = Orton Classic', np.abs(od - ov).max() == 0)
Image.fromarray(np.asarray(small.convert('L'))).save(os.path.join(TMP, 'gray.png'))
run(os.path.join(TMP, 'gray.png'), dict(look='Dreamy Haze'), os.path.join(TMP, 'gray_o.png'))
check('grayscale document runs', True)
big = photo.resize((W * 2, H * 2), Image.LANCZOS); bsrc = os.path.join(TMP, 'big.png'); big.save(bsrc)
_, secs = run(bsrc, dict(look='Dreamy Haze'), os.path.join(TMP, 'big_o.png'))
check('2800x1854 with Dreamy Haze finishes in time', secs < 55, f'{secs:.1f}s')

print(f'\nwork files: {TMP}')
print('ALL PASSED' if not fails else 'FAILED: ' + ', '.join(fails))
sys.exit(1 if fails else 0)
