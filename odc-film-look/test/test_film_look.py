#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
"""End-to-end tests for ODC Film Look through photocraft-cli (headless PhotoCraft).
Run ./build.sh first, then: python3 test/test_film_look.py [path/to/photo.jpg]
Checks the maths against a reference computed here in numpy, determinism, the grain strength and
that the grain has no seams where the host cuts the layer into bands."""
import json, os, subprocess, sys, tempfile
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
WASM = os.path.join(HERE, '..', 'dist', 'odc-film-look.wasm')
CLI = os.environ.get('PHOTOCRAFT_CLI', '/Applications/PhotoCraft.app/Contents/MacOS/photocraft-cli')
TMP = tempfile.mkdtemp(prefix='odc-film-look-')
ID = 'org.opendesignercloud.film-look'
fails = []

def run(src, params, out):
    cmd = [CLI, 'run', src, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
           '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': params}), '--out', out]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0: raise SystemExit('photocraft-cli failed:\n' + r.stdout + r.stderr)
    return np.asarray(Image.open(out).convert('RGB'), dtype=np.float64) / 255

def save(a, name):
    p = os.path.join(TMP, name); Image.fromarray(np.clip(a * 255 + 0.5, 0, 255).astype(np.uint8)).save(p); return p

def check(name, ok, detail=''):
    print(('PASS ' if ok else 'FAIL ') + name + (' ' + detail if detail else ''))
    if not ok: fails.append(name)

# The sliders are adjustments added to the chosen look. 'Custom' starts every amount at 0 (grain size 1.5, color grain 20,
# vignette size 50 / feather 60 / round 50), so the old absolute tests below are written as offsets from those.
ZERO = dict(look='Custom')
rng = np.random.default_rng(1)
H, W = 400, 600
base = rng.uniform(0.1, 0.9, (H, W, 3)).round(3)
src = save(base, 'base.png'); b8 = np.asarray(Image.open(src), dtype=np.float64) / 255

# 1. all effects off: output equals input
o = run(src, ZERO, os.path.join(TMP, 'o1.png'))
check('identity when every effect is off', np.abs(o - b8).max() <= 1 / 255, f'max diff {np.abs(o - b8).max() * 255:.1f} levels')

# 2. fade + warmth against the formula
o = run(src, dict(ZERO, fade=20, warmth=50), os.path.join(TMP, 'o2.png'))
f, w = 0.20 * 0.25, 0.5 * 0.06
ref = f + b8 * (1 - f); ref[..., 0] += w; ref[..., 2] -= w
d = np.abs(o - np.clip(ref, 0, 1)).max() * 255
check('fade and warmth match the formula', d <= 1.01, f'max diff {d:.2f} levels')

# 3. vignette against the formula
vp = dict(ZERO, vignette=60, vignetteSize=-10, vignetteFeather=-10, vignetteRound=-20)
o = run(src, vp, os.path.join(TMP, 'o3.png'))
yy, xx = np.mgrid[0:H, 0:W].astype(np.float64); cx, cy = W / 2, H / 2
dx, dy = xx - cx, yy - cy
ell = np.hypot(dx / cx, dy / cy) * 0.707107; cir = np.hypot(dx, dy) / np.hypot(cx, cy)
r = ell + (cir - ell) * 0.30
st, wd = 1 - 0.40, 0.15 + 0.5 * 0.85
t = np.clip((r - st) / wd, 0, 1); t = t * t * (3 - 2 * t)
ref = b8 * (1 - 0.6 * t)[..., None]
d = np.abs(o - ref).max() * 255
check('vignette matches the formula', d <= 1.01, f'max diff {d:.2f} levels')
check('vignette leaves the centre alone', np.abs(o[H // 2 - 5:H // 2 + 5, W // 2 - 5:W // 2 + 5] - b8[H // 2 - 5:H // 2 + 5, W // 2 - 5:W // 2 + 5]).max() <= 1 / 255)

# 4. grain on flat grey: mean unchanged, strength scales with the slider, same seed = same pixels
flat = save(np.full((1200, 1800, 3), 0.5), 'flat.png')
g25 = run(flat, dict(ZERO, grain=25, grainSize=-0.5, seed=3), os.path.join(TMP, 'g25.png'))
g25b = run(flat, dict(ZERO, grain=25, grainSize=-0.5, seed=3), os.path.join(TMP, 'g25b.png'))
g50 = run(flat, dict(ZERO, grain=50, grainSize=-0.5, seed=3), os.path.join(TMP, 'g50.png'))
g25s = run(flat, dict(ZERO, grain=25, grainSize=-0.5, seed=4), os.path.join(TMP, 'g25s.png'))
check('grain is repeatable for the same seed', np.array_equal(g25, g25b))
check('a different seed gives different grain', not np.array_equal(g25, g25s))
check('grain keeps the average brightness', abs(g25.mean() - 0.5) < 0.002, f'mean {g25.mean():.4f}')
s25, s50 = g25.std(), g50.std()
check('grain strength follows the slider', 1.8 < s50 / s25 < 2.2, f'std {s25 * 255:.1f} -> {s50 * 255:.1f} levels')

# 5. no seams: at grain size 1 every neighbour step looks alike; at size 3 the steps repeat with the
#    3 pixel lattice (that is the grain's look), so compare the columns/rows 256 apart (where bands can meet) with the average
def steps(size):
    g = run(flat, dict(ZERO, grain=40, grainSize=size - 1.5, seed=1), os.path.join(TMP, 'gs%d.png' % size)).mean(axis=2)
    return np.abs(np.diff(g, axis=1)).mean(axis=0), np.abs(np.diff(g, axis=0)).mean(axis=1)
c1, r1 = steps(1)
worst = max(c1.max() / np.median(c1), r1.max() / np.median(r1))
check('no seams in fine grain', worst < 1.2, f'largest step between neighbours is {worst:.2f}x the typical one')
c3, r3 = steps(3)
edge = max(c3[255::256].mean() / c3.mean(), r3[255::256].mean() / r3.mean())
check('no seams in coarse grain at band edges', 0.85 < edge < 1.15, f'band-edge steps are {edge:.2f}x the average')

# 5b. the looks: every HyperImage preset against HyperImage's own code (test/reference.js, run with node)
CURVES = {
    'Fuji Superia 1600': [[0, 0], [26, 31], [58, 65], [75, 86], [138, 148], [182, 188], [201, 203], [212, 213], [243, 237], [255, 247]],
    'Kodak Kodachrome 2': [[0, 0], [33, 24], [64, 64], [92, 104], [128, 148], [166, 166], [207, 192], [229, 217], [255, 255]],
    'Tri-X': [[0, 0], [63, 88], [123, 161], [168, 211], [201, 240], [226, 251], [251, 255]],
    'WhiteWedding': [[0, 0], [45, 64], [90, 128], [128, 172], [191, 209], [225, 225], [244, 238], [252, 249], [255, 255]],
    'Kodak T-Max 400': [[0, 0], [26, 28], [77, 66], [150, 146], [226, 243], [255, 255]],
    'Tri-X B&W': [[0, 0], [63, 88], [123, 161], [168, 211], [201, 240], [226, 251], [251, 255]],
    'Matte Fade': [[0, 20], [64, 80], [128, 140], [192, 200], [255, 236]],
}
HI = {   # the HyperImage settings of each look (Depth, Sharpness and Texture are not ported, so they are left out here too)
    'Vintage Western': dict(black=16, gamma=88, exposure=-10, contrast=18, highlights=-25, shadows=8, saturation=-100, sepia=75),
    'Fuji Superia 1600': dict(contrast=5, highlights=-5, shadows=20, saturation=-8, temp=32, tint=3),
    'Kodak Kodachrome 2': dict(saturation=-10),
    'Tri-X': {}, 'WhiteWedding': {}, 'Tri-X B&W': dict(saturation=-100),
    'Kodak T-Max 400': dict(gamma=105, saturation=-100, temp=25),
    'Matte Fade': dict(contrast=-8, saturation=-8, stShadow='#4a6b8a', stHighlight='#efdcc0', stBalance=0, stAmount=12),
    'High-Contrast B&W': dict(black=12, contrast=30, saturation=-100),
    'Bleach Bypass': dict(black=10, contrast=28, saturation=-55),
    'Golden Hour': dict(temp=25, tint=3, vibrance=15, shadows=10, highlights=-8),
}
cancel = lambda look: dict(look=look, grain=-100, vignette=-100)   # the look's own grain and vignette are tested separately
px = (b8 * 255).reshape(-1, 3)
for name, settings in HI.items():
    q = json.dumps(dict(settings=settings, curve=CURVES.get(name), pixels=px.ravel().tolist()))
    ref = np.array(json.loads(subprocess.run(['node', os.path.join(HERE, 'reference.js')], input=q, capture_output=True, text=True, check=True).stdout)).reshape(H, W, 3)
    o = run(src, cancel(name), os.path.join(TMP, 'look.png'))
    d = np.abs(o * 255 - np.clip(ref, 0, 255)).max()
    check('look ' + name + ' matches HyperImage', d <= 1.6, f'max diff {d:.2f} levels')
# Vintage Western also carries grain and a vignette: the vignette is a pure multiply, so a flat grey shows it exactly
vw = run(flat, dict(look='Vintage Western', grain=-100), os.path.join(TMP, 'vw.png'))
ratio = vw[0, 0].mean() / vw[600, 900].mean()
check('Vintage Western vignette darkens the corners', 0.40 < ratio < 0.75, f'corner/centre = {ratio:.2f}')
# strength 0 is the original, strength 50 is halfway
o0 = run(src, dict(look='Kodak Kodachrome 2', strength=0, grain=-100, vignette=-100), os.path.join(TMP, 's0.png'))
check('strength 0 returns the original', np.abs(o0 - b8).max() <= 1 / 255)
full = run(src, dict(look='Tri-X B&W', grain=-100, vignette=-100), os.path.join(TMP, 's100.png'))
half = run(src, dict(look='Tri-X B&W', strength=50, grain=-100, vignette=-100), os.path.join(TMP, 's50.png'))
check('strength 50 is halfway', np.abs(half - (b8 + (full - b8) * 0.5)).max() <= 1.6 / 255)

# 5c. the preset tool knows exactly the plug-in's parameters and looks (it cannot drift from the manifest)
sys.path.insert(0, os.path.join(HERE, '..', 'tools'))
import film_look_presets as flp
r = subprocess.run([CLI, 'run', src, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)})], capture_output=True, text=True)
schema = json.loads(r.stdout.splitlines()[0])['result']['paramsSchema']
check('preset tool lists the plug-in looks', flp.LOOKS == schema['look']['options'])
check('preset tool ranges match the manifest', all(k in schema and (schema[k]['min'], schema[k]['max']) == tuple(v) for k, v in flp.RANGES.items()) and set(schema) == set(flp.RANGES) | {'look'})
flp.STORE = os.path.join(TMP, 'presets.json')
flp.main(['add', 'Test One', '--from', 'Tri-X', 'grain=12']); flp.main(['edit', 'test one', 'fade=5']); a = flp.load()
check('preset tool add and edit', a == {'Test One': {'look': 'Tri-X', 'grain': 12, 'fade': 5}}, str(a))
flp.main(['delete', 'TEST ONE'])
check('preset tool delete', flp.load() == {})

# 6. unsupported colour mode is refused, not silently wrong (CMYK)
cm = os.path.join(TMP, 'cmyk.tif'); Image.fromarray((base * 255).astype(np.uint8)).convert('CMYK').save(cm)
r = subprocess.run([CLI, 'run', cm, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
                    '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': {}}), '--out', os.path.join(TMP, 'cm.png')], capture_output=True, text=True)
print('INFO CMYK input:', (r.stdout + r.stderr).strip().splitlines()[-1][:160] if (r.stdout + r.stderr).strip() else 'no output')

# 7. a real photo with the defaults, for looking at
if len(sys.argv) > 1:
    os.makedirs(os.path.join(HERE, 'out'), exist_ok=True)
    o = subprocess.run([CLI, 'run', sys.argv[1], '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
                        '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': {}}), '--out', os.path.join(HERE, 'out', 'photo-default.jpg')], capture_output=True, text=True)
    print('wrote test/out/photo-default.jpg' if o.returncode == 0 else o.stderr)

print('\n%d failed: %s' % (len(fails), ', '.join(fails)) if fails else '\nall passed')
sys.exit(1 if fails else 0)
