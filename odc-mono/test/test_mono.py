#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""End-to-end tests for ODC Mono through photocraft-cli (headless PhotoCraft).
Run ./build.sh first, then: python3 test/test_mono.py [path/to/photo.jpg]

HyperImage has no black and white tool, so the reference is written here in numpy: an independent implementation of the
same pipeline (stock weights through a colour filter, tone table with the stock's curve, fade, split toning). Grain and
vignette are checked by formula / statistics, and for seams where the host cuts a big layer into bands. Needs numpy, Pillow."""
import json, os, shutil, subprocess, sys, tempfile, time
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
WASM = os.path.join(HERE, '..', 'dist', 'odc-mono.wasm')
CLI = os.environ.get('PHOTOCRAFT_CLI') or shutil.which('photocraft-cli') or '/Applications/PhotoCraft.app/Contents/MacOS/photocraft-cli'
PHOTO = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, '..', 'examples', 'original.jpg')
TMP = tempfile.mkdtemp(prefix='odc-mono-')
ID = 'org.opendesignercloud.mono'
fails = []

FILTERS = {'None': (1, 1, 1), 'Yellow': (1, .90, .15), 'Orange': (1, .55, .05), 'Red': (1, .12, .02), 'Green': (.15, 1, .10), 'Blue': (.05, .25, 1)}
TRIX = [(0, 0), (63, 88), (123, 161), (168, 211), (201, 240), (226, 251), (251, 255)]
TMAX = [(0, 0), (26, 28), (77, 66), (150, 146), (226, 243), (255, 255)]
N = dict(w=(.299, .587, .114), filt='None', fs=100, curve=None, exposure=0, contrast=0, highlights=0, shadows=0, fade=0,
         sh=(30, 0), hi=(40, 0), tone=0, bal=0)
LOOKS = {
    'Neutral': dict(N), 'Custom': dict(N),
    'Tri-X 400': dict(N, w=(.34, .50, .16), curve=TRIX),
    'T-Max 100': dict(N, w=(.28, .55, .17), curve=TMAX),
    'Orthochromatic': dict(N, w=(0, .62, .38), contrast=12),
    'Infrared': dict(N, w=(.15, .95, -.10), contrast=10, fade=6),
    'Red Filter Drama': dict(N, w=(.34, .50, .16), filt='Red', contrast=28),
    'Noir': dict(N, w=(.34, .50, .16), filt='Orange', fs=60, contrast=40, highlights=-10, shadows=-20),
    'Sepia Print': dict(N, w=(.34, .50, .16), filt='Yellow', fs=50, contrast=8, fade=5, sh=(28, .65), hi=(42, .30), tone=70),
    'Selenium Print': dict(N, w=(.34, .50, .16), contrast=12, sh=(340, .35), hi=(30, .10), tone=55),
    'Cyanotype': dict(N, w=(.34, .50, .16), contrast=15, fade=8, sh=(215, .90), hi=(195, .22), tone=85),
    'Platinum Fade': dict(N, w=(.30, .54, .16), contrast=-12, fade=18, sh=(28, .25), hi=(40, .12), tone=50),
}


def run(src, params, out):
    cmd = [CLI, 'run', src, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
           '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': params}), '--out', out]
    t = time.time()
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0: raise SystemExit('photocraft-cli failed:\n' + r.stdout[-300:] + r.stderr)
    return np.asarray(Image.open(out).convert('RGB'), dtype=np.float64) / 255, time.time() - t


def curve_lut(pts):
    """Monotone cubic (Fritsch-Carlson) through the points, sampled at 0..255, as HyperImage's buildCurveLUT."""
    xs = [p[0] for p in pts]; ys = [p[1] for p in pts]; n = len(pts)
    dx = [xs[i + 1] - xs[i] for i in range(n - 1)]; m = [(ys[i + 1] - ys[i]) / dx[i] for i in range(n - 1)]
    c = [m[0]] + [0.0] * (n - 2) + [m[-1]]
    for i in range(1, n - 1):
        if m[i - 1] * m[i] <= 0: c[i] = 0
        else:
            w1, w2 = 2 * dx[i] + dx[i - 1], dx[i] + 2 * dx[i - 1]; c[i] = (w1 + w2) / (w1 / m[i - 1] + w2 / m[i])
    out = np.zeros(256)
    for x in range(256):
        if x <= xs[0]: out[x] = ys[0]; continue
        if x >= xs[-1]: out[x] = ys[-1]; continue
        i = 0
        while i < n - 1 and x > xs[i + 1]: i += 1
        h = dx[i]; t = (x - xs[i]) / h; t2, t3 = t * t, t * t * t
        out[x] = (2 * t3 - 3 * t2 + 1) * ys[i] + (t3 - 2 * t2 + t) * h * c[i] + (-2 * t3 + 3 * t2) * ys[i + 1] + (t3 - t2) * h * c[i + 1]
    return out


def hsv(h, s):
    h = (h % 360) / 60.0; i = int(np.floor(h)); f = h - i; p, q, t = 1 - s, 1 - s * f, 1 - s * (1 - f)
    return np.array([(1, t, p), (q, 1, p), (p, 1, t), (p, q, 1), (t, p, 1), (1, p, q)][i % 6], dtype=float)


def smooth(a, b, x):
    t = np.clip((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t)


def reference(img8, look, **adj):
    L = dict(LOOKS[look])
    for k, v in adj.items(): L[k] = v
    a = img8 / 255.0
    T = np.array(FILTERS[L['filt']]); fs = np.clip(L['fs'], 0, 100) / 100
    w = np.array(L['w']) * (1 + (T - 1) * fs); w = w / w.sum()
    g = np.clip(a @ w, 0, 1)
    i = np.arange(256, dtype=float)
    v = i * 2 ** (L['exposure'] / 100); v = (v - 128) * (1 + L['contrast'] / 100) + 128
    n = v / 255; v = v + L['shadows'] / 100 * (1 - n) * 64 + L['highlights'] / 100 * n * 64
    if L['curve']: v = curve_lut(L['curve'])[np.floor(np.clip(v, 0, 255) + 0.5).astype(int)]
    table = np.clip(v, 0, 255)
    x = g * 255; lo = np.clip(np.floor(x).astype(int), 0, 254)
    out = (table[lo] + (table[lo + 1] - table[lo]) * (x - lo)) / 255
    out = np.where(x >= 255, table[255] / 255, out)
    fade = np.clip(L['fade'], 0, 100) / 100 * 0.25
    out = fade + out * (1 - fade)
    c = np.stack([out, out, out], -1)
    amt = np.clip(L['tone'], 0, 100) / 100
    if amt > 0:
        shc, hic = hsv(*L['sh']), hsv(*L['hi'])
        shl, hil = max(0.02, shc @ [.299, .587, .114]), max(0.02, hic @ [.299, .587, .114])
        pivot = np.clip(0.5 + L['bal'] / 200, 0.05, 0.95)
        l = np.clip(out, 0, 1)
        w1 = np.where(l < pivot, (pivot - l) / pivot, 0); k1 = (smooth(0, 1, w1) * amt)[..., None]
        c = c + (shc * (l / shl)[..., None] - c) * k1 * (w1 > 0)[..., None]
        w2 = np.where(l > pivot, (l - pivot) / (1 - pivot), 0); k2 = (smooth(0, 1, w2) * amt)[..., None]
        c = c + (hic * (l / hil)[..., None] - c) * k2 * (w2 > 0)[..., None]
    return np.clip(c, 0, 1)


def check(name, ok, detail=''):
    print(('PASS ' if ok else 'FAIL ') + name + (' ' + detail if detail else ''))
    if not ok: fails.append(name)


NOEXTRA = dict(grain=-100, vignette=-100)   # cancel a look's own grain and vignette (tested separately)


def compare(name, src, img8, look, params=None, tol=1.6, **adj):
    p = dict(look=look, **NOEXTRA); p.update(params or {})
    o, secs = run(src, p, os.path.join(TMP, 'o.png'))
    d = np.abs(o - reference(img8, look, **adj)) * 255
    check(name, d.max() <= tol, f'max diff {d.max():.2f} levels, mean {d.mean():.3f} ({secs:.1f}s)')
    return o


photo = Image.open(PHOTO).convert('RGB')
psrc = os.path.join(TMP, 'photo.png'); photo.save(psrc)
p8 = np.asarray(photo); H, W = p8.shape[:2]
print(f'photo {W}x{H}, cli {CLI}')
small = photo.resize((420, 280), Image.LANCZOS)
ssrc = os.path.join(TMP, 'small.png'); small.save(ssrc); s8 = np.asarray(small)

# 1. conversion, filters and the sliders
compare('Custom is plain luma grey', ssrc, s8, 'Custom')
for f in ['Yellow', 'Orange', 'Red', 'Green', 'Blue']:
    compare(f'{f} filter at full strength', ssrc, s8, 'Custom', dict(filter=f), filt=f)
compare('Red filter at half strength', ssrc, s8, 'Custom', dict(filter='Red', filterStrength=-50), filt='Red', fs=50)
compare('exposure and contrast', ssrc, s8, 'Custom', dict(exposure=20, contrast=30), exposure=20, contrast=30)
compare('highlights and shadows', ssrc, s8, 'Custom', dict(highlights=-40, shadows=40), highlights=-40, shadows=40)
compare('fade', ssrc, s8, 'Custom', dict(fade=40), fade=40)
compare('slider offsets add to a look (Tri-X 400)', ssrc, s8, 'Tri-X 400', dict(contrast=15, fade=10), contrast=15, fade=10)

# 2. neutral stays neutral through any filter
grey = np.tile(np.linspace(0, 1, 256)[None, :, None], (8, 1, 3)); gsrc = os.path.join(TMP, 'grey.png')
Image.fromarray((grey * 255 + 0.5).astype(np.uint8)).save(gsrc)
o, _ = run(gsrc, dict(look='Custom', filter='Red', **NOEXTRA), os.path.join(TMP, 'g.png'))
check('a grey ramp stays a grey ramp through a red filter', np.abs(o - grey).max() * 255 <= 1.01, f'{np.abs(o - grey).max() * 255:.2f}')

# 3. every look (stock weights, filter, curve, tone)
for lk in [k for k in LOOKS if k not in ('Custom', 'Neutral')]:
    compare(f'look "{lk}"', ssrc, s8, lk)
# toned looks are coloured, untoned ones are grey
o, _ = run(ssrc, dict(look='Tri-X 400', **NOEXTRA), os.path.join(TMP, 'tx.png'))
check('Tri-X 400 is neutral grey', np.abs(o[..., 0] - o[..., 2]).max() * 255 <= 1.01)
o, _ = run(ssrc, dict(look='Cyanotype', **NOEXTRA), os.path.join(TMP, 'cy.png'))
check('Cyanotype is blue (more blue than red in the shadows)', (o[..., 2] - o[..., 0])[o.mean(axis=2) < 0.3].mean() > 0.04)

# 4. toning sliders
compare('toneAmount, shadowHue, highlightHue', ssrc, s8, 'Selenium Print', dict(toneAmount=20, shadowHue=60, highlightHue=-30, toneBalance=15),
        tone=75, sh=(40, .35), hi=(0, .10), bal=15)

# 5. vignette against the formula; the centre is untouched
H0, W0 = 400, 600
flat = np.full((H0, W0, 3), 0.5); fsrc = os.path.join(TMP, 'flat.png'); Image.fromarray((flat * 255).astype(np.uint8)).save(fsrc)
o, _ = run(fsrc, dict(look='Neutral', vignette=60, vignetteSize=-10), os.path.join(TMP, 'vg.png'))
yy, xx = np.mgrid[0:H0, 0:W0].astype(float); cx, cy = W0 / 2, H0 / 2; dx, dy = xx - cx, yy - cy
ell = np.hypot(dx / cx, dy / cy) * 0.707107; cir = np.hypot(dx, dy) / np.hypot(cx, cy); r = ell + (cir - ell) * 0.5
t = smooth(1 - 0.40, 1 - 0.40 + 0.15 + 0.6 * 0.85, r)
ref = (127.5 / 255) * (1 - 0.6 * t)
d = np.abs(o[..., 0] - ref).max() * 255
check('vignette matches the formula', d <= 1.01, f'max diff {d:.2f} levels')
check('vignette leaves the centre alone', np.abs(o[H0 // 2 - 5:H0 // 2 + 5, W0 // 2 - 5:W0 // 2 + 5] - 127.5 / 255).max() * 255 <= 1.01)

# 6. grain on flat grey: same on every channel, mean unchanged, strength follows the slider, repeatable, no seams
big = np.full((1200, 1800, 3), 0.5); bsrc = os.path.join(TMP, 'big.png'); Image.fromarray((big * 255).astype(np.uint8)).save(bsrc)
base = dict(look='Neutral')
g25, _ = run(bsrc, dict(base, grain=25, grainSize=-0.5, seed=3), os.path.join(TMP, 'g25.png'))
g25b, _ = run(bsrc, dict(base, grain=25, grainSize=-0.5, seed=3), os.path.join(TMP, 'g25b.png'))
g50, _ = run(bsrc, dict(base, grain=50, grainSize=-0.5, seed=3), os.path.join(TMP, 'g50.png'))
g25s, _ = run(bsrc, dict(base, grain=25, grainSize=-0.5, seed=4), os.path.join(TMP, 'g25s.png'))
check('grain is repeatable for the same seed', np.array_equal(g25, g25b))
check('a different seed gives different grain', not np.array_equal(g25, g25s))
check('grain is monochrome (same on every channel)', np.abs(g25[..., 0] - g25[..., 2]).max() * 255 <= 1.01)
check('grain keeps the average brightness', abs(g25.mean() - 0.5) < 0.002, f'mean {g25.mean():.4f}')
check('grain strength follows the slider', 1.8 < g50.std() / g25.std() < 2.2, f'std {g25.std() * 255:.1f} -> {g50.std() * 255:.1f} levels')


def steps(size):
    g = run(bsrc, dict(base, grain=40, grainSize=size - 1.5, seed=1), os.path.join(TMP, 'gs%d.png' % size))[0].mean(axis=2)
    return np.abs(np.diff(g, axis=1)).mean(axis=0), np.abs(np.diff(g, axis=0)).mean(axis=1)


c1, r1 = steps(1)
check('no seams in fine grain', max(c1.max() / np.median(c1), r1.max() / np.median(r1)) < 1.2)
c3, r3 = steps(3)
edge = max(c3[255::256].mean() / c3.mean(), r3[255::256].mean() / r3.mean())
check('no seams in coarse grain at band edges', 0.85 < edge < 1.15, f'band-edge steps are {edge:.2f}x the average')

# 7. strength, alpha, defaults, gray document
o0, _ = run(ssrc, dict(look='Tri-X 400', strength=0), os.path.join(TMP, 's0.png'))
check('strength 0 returns the original', np.abs(o0 * 255 - s8).max() <= 1)
full, _ = run(ssrc, dict(look='Sepia Print', **NOEXTRA), os.path.join(TMP, 'f.png'))
half, _ = run(ssrc, dict(look='Sepia Print', strength=50, **NOEXTRA), os.path.join(TMP, 'h.png'))
check('strength 50 is halfway', np.abs(half - (full + s8 / 255) / 2).max() * 255 <= 1.6)
rgba = Image.open(ssrc).convert('RGBA'); a = np.asarray(rgba).copy(); a[:, :, 3] = np.linspace(255, 40, a.shape[1]).astype(np.uint8)[None, :]
Image.fromarray(a).save(os.path.join(TMP, 'rgba.png'))
run(os.path.join(TMP, 'rgba.png'), dict(look='Noir'), os.path.join(TMP, 'rgba_o.png'))
oa = np.asarray(Image.open(os.path.join(TMP, 'rgba_o.png')).convert('RGBA'))
check('alpha channel is kept', np.abs(oa[:, :, 3].astype(int) - a[:, :, 3]).max() <= 1)
od, _ = run(ssrc, {}, os.path.join(TMP, 'def.png')); ot, _ = run(ssrc, dict(look='Tri-X 400'), os.path.join(TMP, 'trix.png'))
check('default parameters = Tri-X 400', np.abs(od - ot).max() == 0)
Image.fromarray(np.asarray(small.convert('L'))).save(os.path.join(TMP, 'gray.png'))
run(os.path.join(TMP, 'gray.png'), dict(look='Noir'), os.path.join(TMP, 'gray_o.png'))
check('grayscale document runs', True)

# 8. the full-size photo (cut into bands by the host) matches the whole-image reference; speed on bigger pictures
compare('full photo in bands matches the reference ("Cyanotype")', psrc, p8, 'Cyanotype')
for (w, h) in [(2800, 1854), (6000, 4000)]:
    im = photo.resize((w, h), Image.BILINEAR); p = os.path.join(TMP, f'b{w}.png'); im.save(p)
    _, secs = run(p, dict(look='Noir'), os.path.join(TMP, 'big_o.png'))
    check(f'{w}x{h} ({w * h / 1e6:.0f} MP) with Noir finishes in time', secs < 55, f'{secs:.1f}s')

# 9. the preset tool knows exactly the plug-in's parameters and looks
sys.path.insert(0, os.path.join(HERE, '..', 'tools'))
import mono_presets as mp
r = subprocess.run([CLI, 'run', ssrc, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)})], capture_output=True, text=True)
schema = json.loads(r.stdout.splitlines()[0])['result']['paramsSchema']
check('preset tool lists the plug-in looks', mp.LOOKS == schema['look']['options'])
check('preset tool lists the filters', mp.FILTERS == schema['filter']['options'])
check('preset tool ranges match the manifest', all(k in schema and (schema[k]['min'], schema[k]['max']) == tuple(v) for k, v in mp.RANGES.items()) and set(schema) == set(mp.RANGES) | {'look', 'filter'})

print(f'\nwork files: {TMP}')
print('ALL PASSED' if not fails else 'FAILED: ' + ', '.join(fails))
sys.exit(1 if fails else 0)
