#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""End-to-end tests for ODC Retro through photocraft-cli (headless PhotoCraft).
Run ./build.sh first, then: python3 test/test_retro.py [path/to/photo.jpg]

There is nothing in HyperImage to compare with, so the reference is written here in numpy: an independent implementation of the
same pipeline over the whole image (blocks, glitch bands, scanlines and the dither matrix are aligned to the canvas). The big
photo is cut into bands by the host, so a match there also proves the overlap works (no seams). Where a palette decision sits
on a knife edge (float rounding), a handful of pixels may pick the neighbouring colour: palette tests allow 0.2 % of pixels to
differ. Needs numpy, Pillow."""
import json, os, shutil, subprocess, sys, tempfile, time
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
WASM = os.path.join(HERE, '..', 'dist', 'odc-retro.wasm')
CLI = os.environ.get('PHOTOCRAFT_CLI') or shutil.which('photocraft-cli') or '/Applications/PhotoCraft.app/Contents/MacOS/photocraft-cli'
PHOTO = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, '..', 'examples', 'original.jpg')
TMP = tempfile.mkdtemp(prefix='odc-retro-')
ID = 'org.opendesignercloud.retro'
fails = []
f32 = np.float32

PALETTES = {
    'None': ([], 0), 'Game Boy': ([0x0f380f, 0x306230, 0x8bac0f, 0x9bbc0f], .50), '1-Bit': ([0x000000, 0xffffff], 1.0),
    'Grayscale 4': ([0, 0x555555, 0xaaaaaa, 0xffffff], .40), 'CGA Cyan Magenta': ([0, 0x55ffff, 0xff55ff, 0xffffff], .60),
    'CGA Red Green': ([0, 0x55ff55, 0xff5555, 0xffff55], .60),
    'PICO-8': ([0x000000, 0x1d2b53, 0x7e2553, 0x008751, 0xab5236, 0x5f574f, 0xc2c3c7, 0xfff1e8, 0xff004d, 0xffa300, 0xffec27, 0x00e436,
                0x29adff, 0x83769c, 0xff77a8, 0xffccaa], .30),
    'Amber Terminal': ([0, 0x4d2a00, 0xa35c00, 0xffb000], .45), 'Green Phosphor': ([0, 0x0a3d0a, 0x1f9a1f, 0x66ff66], .45),
}
N = dict(pix=0, palette='None', dither=0, scan=0, line=3.5, mask=0, smear=0, split=0, glitch=0, noise=0, sat=0, contrast=0, vig=0)
LOOKS = {
    'Custom': dict(N),
    'VHS Tape': dict(N, line=3.0, scan=18, smear=55, split=30, glitch=15, noise=22, sat=-12, contrast=-6, vig=12),
    'CRT Monitor': dict(N, scan=45, line=3.5, mask=35, smear=15, split=10, noise=6, sat=12, contrast=12, vig=35),
    'Game Boy': dict(N, pix=8, palette='Game Boy', dither=60, contrast=10),
    '1-Bit Dither': dict(N, pix=3, palette='1-Bit', dither=100, contrast=18),
    'CGA Arcade': dict(N, pix=6, palette='CGA Cyan Magenta', dither=70, scan=14, line=3.5),
    'PICO-8': dict(N, pix=6, palette='PICO-8', dither=45),
    'Amber Terminal': dict(N, pix=3, palette='Amber Terminal', dither=40, scan=40, line=3.5, smear=10, noise=8, vig=40),
    'Glitch': dict(N, line=3.0, scan=10, split=70, glitch=60, noise=10, contrast=8),
}
# plug-in parameter name for each look field
PNAME = dict(pix='pixelSize', dither='dither', scan='scanlines', line='lineSize', mask='phosphor', smear='colorSmear', split='colorSplit',
             glitch='glitch', noise='static', sat='saturation', contrast='contrast', vig='vignette')


def run(src, params, out):
    cmd = [CLI, 'run', src, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
           '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': params}), '--out', out]
    t = time.time()
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0: raise SystemExit('photocraft-cli failed:\n' + r.stdout[-300:] + r.stderr)
    return np.asarray(Image.open(out).convert('RGB'), dtype=np.float64) / 255, time.time() - t


def hsh(x, y, ch, seed):
    u = lambda v: (np.asarray(v).astype(np.int64) & 0xffffffff).astype(np.uint32)
    m = lambda a, b: np.multiply(a, np.uint32(b), dtype=np.uint32)
    h = m(u(x), 0x9e3779b1) ^ m(u(y), 0x85ebca6b) ^ m(u(ch), 0xc2b2ae35) ^ m(u(seed), 0x27d4eb2f)
    h = h ^ (h >> np.uint32(16)); h = m(h, 0x7feb352d); h = h ^ (h >> np.uint32(15)); h = m(h, 0x846ca68b); h = h ^ (h >> np.uint32(16))
    return (h >> np.uint32(8)).astype(f32) * f32(2.0 / 16777216.0) - f32(1.0)


def bayer8(x, y):
    M = np.array([[0, 2], [3, 1]]); v = np.zeros(np.broadcast(x, y).shape, dtype=np.int64)
    for bit in (2, 1, 0): v = v * 4 + M[(y >> bit) & 1, (x >> bit) & 1]
    return v.astype(f32)


def rnd(v): return int(np.floor(f32(v) + f32(0.5)))


def box_mean_rows(a, r):
    """Mean over x-radius r, edges clamped. a: (H, W)."""
    if r == 0: return a
    W = a.shape[1]; pad = np.concatenate([np.repeat(a[:, :1], r, 1), a, np.repeat(a[:, -1:], r, 1)], 1)
    c = np.concatenate([np.zeros((a.shape[0], 1)), np.cumsum(pad, 1)], 1)
    return (c[:, 2 * r + 1:2 * r + 1 + W] - c[:, :W]) / (2 * r + 1)


def smooth(a, b, x):
    t = np.clip((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t)


def reference(img8, look, **adj):
    L = dict(LOOKS[look]); a = img8 / 255.0; H, W, _ = a.shape
    seed = int(adj.pop('seed', 0)); strength = adj.pop('strength', 100) / 100
    pal_name = adj.pop('palette', None) or L['palette']
    for k, v in adj.items(): L[k] = L[k] + v           # sliders are offsets from the look
    long_, short_ = float(max(W, H)), float(min(W, H))
    clamp = lambda v, lo, hi: min(max(v, lo), hi)
    block = int(clamp(rnd(f32(clamp(L['pix'], 0, 100)) * f32(long_) / f32(1000)), 1, 32))
    dither = clamp(L['dither'], 0, 100) / 100; scan = clamp(L['scan'], 0, 100) / 100
    period = int(clamp(rnd(f32(clamp(L['line'], 1, 12)) * f32(short_) / f32(1000)), 2, 24))
    mask = clamp(L['mask'], 0, 100) / 100
    smear = rnd(f32(clamp(L['smear'], 0, 100)) / f32(100) * f32(24)); split = rnd(f32(clamp(L['split'], 0, 100)) / f32(100) * f32(16))
    glitch = f32(clamp(L['glitch'], 0, 100) / 100); noise = f32(clamp(L['noise'], 0, 100) / 100 * 0.2)
    sat = 1 + clamp(L['sat'], -100, 100) / 100; con = 1 + clamp(L['contrast'], -100, 100) / 100; vig = clamp(L['vig'], 0, 100) / 100
    band_h = max(3, rnd(f32(short_) * f32(0.006)))
    # pixelate (blocks aligned to the canvas, edge replicated)
    pix = a
    if block > 1:
        Hp, Wp = -(-H // block) * block, -(-W // block) * block
        pd = np.pad(a, ((0, Hp - H), (0, Wp - W), (0, 0)), mode='edge')
        m = pd.reshape(Hp // block, block, Wp // block, block, 3).mean((1, 3))
        pix = np.repeat(np.repeat(m, block, 0), block, 1)[:H, :W]
    # glitch bands, RGB split
    gy = np.arange(H); shift = np.zeros(H, dtype=np.int64)
    if glitch > 0:
        b = gy // band_h
        gate = (hsh(b, 0x77, 9, seed) + f32(1)) * f32(0.5)
        sh = np.floor(hsh(b, 0x51, 7, seed) * f32(48) * glitch + f32(0.5)).astype(np.int64)
        shift = np.where(gate < glitch * f32(0.6), sh, 0)
    # The host hands the plug-in the picture with 128 px of edge-replicated margin, and the row effects (glitch shift, RGB split,
    # colour smear) read through it: do the same here, then crop back to the canvas.
    P = 128
    pad = np.pad(pix, ((0, 0), (P, P), (0, 0)), mode='edge'); Wp = W + 2 * P
    xs = np.arange(Wp)[None, :]
    def fetch(c, off): return np.take_along_axis(pad[..., c], np.clip(xs - shift[:, None] + off, 0, Wp - 1), 1)
    r, g, b_ = fetch(0, -split), fetch(1, 0), fetch(2, split)
    if smear > 0:
        y = .299 * r + .587 * g + .114 * b_
        i = .596 * r - .274 * g - .322 * b_; q = .211 * r - .523 * g + .312 * b_
        i, q = box_mean_rows(i, smear), box_mean_rows(q, smear)
        r, g, b_ = y + .956 * i + .621 * q, y - .272 * i - .647 * q, y - 1.106 * i + 1.703 * q
    r, g, b_ = r[:, P:P + W], g[:, P:P + W], b_[:, P:P + W]
    c = np.stack([r, g, b_], -1)
    l = .299 * c[..., 0] + .587 * c[..., 1] + .114 * c[..., 2]
    c = ((l[..., None] + (c - l[..., None]) * sat) - .5) * con + .5
    gx = np.arange(W)[None, :] + 0 * gy[:, None]; gyy = gy[:, None] + 0 * np.arange(W)[None, :]
    cxb, cyb = gx // block, gyy // block
    if noise > 0: c = c + (hsh(cxb, cyb, 0, seed + 101) * noise)[..., None]
    c = np.clip(c, 0, 1)
    cols, spread = PALETTES[pal_name]
    if cols:
        bias = (bayer8(cxb, cyb) / 64 - 0.4921875) * spread * dither
        pc = np.array([[(e >> 16) & 255, (e >> 8) & 255, e & 255] for e in cols]) / 255.0
        t = c + bias[..., None]
        d = ((t[..., None, :] - pc) ** 2 * np.array([.30, .59, .11])).sum(-1)
        c = pc[np.argmin(d, -1)]
    if scan > 0:
        tt = (gyy % period) / period; dd = np.abs(2 * tt - 1); c = c * (1 - scan * dd * dd)[..., None]
    if mask > 0:
        col = gx % 3; fac = np.stack([1 - mask * (1 - np.where(col == q, 1.0, 0.35)) for q in range(3)], -1); c = c * fac
    if vig > 0:
        cx, cy = W * 0.5, H * 0.5; ex, ey = (gx - cx) / cx, (gyy - cy) / cy
        rr = np.sqrt(ex * ex + ey * ey) * 0.707107; c = c * (1 - vig * smooth(0.45, 1.05, rr))[..., None]
    return np.clip(a + (np.clip(c, 0, 1) - a) * strength, 0, 1)


def check(name, ok, detail=''):
    print(('PASS ' if ok else 'FAIL ') + name + (' ' + detail if detail else ''))
    if not ok: fails.append(name)


def compare(name, src, img8, look='Custom', params=None, tol=1.6, frac=None, **adj):
    """params: what is sent to the plug-in; adj: the same changes for the reference (sliders, palette, seed, strength)."""
    p = dict(look=look); p.update(params or {})
    o, secs = run(src, p, os.path.join(TMP, 'o.png'))
    d = np.abs(o - reference(img8, look, **adj)) * 255
    if frac is None:
        ok, detail = d.max() <= tol, f'max diff {d.max():.2f} levels, mean {d.mean():.3f}'
    else:
        bad = (d.max(axis=2) > tol).mean(); ok, detail = bad <= frac, f'{bad * 100:.3f}% of pixels differ, mean {d.mean():.3f}'
    check(name, ok, f'{detail} ({secs:.1f}s)')
    return o


photo = Image.open(PHOTO).convert('RGB')
psrc = os.path.join(TMP, 'photo.png'); photo.save(psrc)
p8 = np.asarray(photo); H, W = p8.shape[:2]
print(f'photo {W}x{H}, cli {CLI}')
small = photo.resize((420, 280), Image.LANCZOS)
ssrc = os.path.join(TMP, 'small.png'); small.save(ssrc); s8 = np.asarray(small)

# 1. nothing on: output equals input
o, _ = run(ssrc, dict(look='Custom'), os.path.join(TMP, 'id.png'))
check('identity when every control is 0', np.abs(o * 255 - s8).max() <= 1, f'{np.abs(o * 255 - s8).max():.1f}')

# 2. each effect on its own (small picture, one band)
compare('pixel size', ssrc, s8, params=dict(pixelSize=12), pix=12)
compare('colour smear', ssrc, s8, params=dict(colorSmear=60), smear=60)
compare('RGB split', ssrc, s8, params=dict(colorSplit=70), split=70)
compare('glitch bands', ssrc, s8, params=dict(glitch=60, seed=5), glitch=60, seed=5)
compare('glitch bands, another seed', ssrc, s8, params=dict(glitch=60, seed=11), glitch=60, seed=11)
compare('static', ssrc, s8, params=dict(static=40, seed=3), noise=40, seed=3)
compare('saturation and contrast', ssrc, s8, params=dict(saturation=-40, contrast=30), sat=-40, contrast=30)
compare('scanlines', ssrc, s8, params=dict(scanlines=60), scan=60)
compare('scanline size', ssrc, s8, params=dict(scanlines=60, lineSize=2), scan=60, line=2)
compare('phosphor mask', ssrc, s8, params=dict(phosphor=70), mask=70)
compare('vignette', ssrc, s8, params=dict(vignette=70), vig=70)
for pal in [k for k in PALETTES if k != 'None']:
    compare(f'palette {pal} with dither', ssrc, s8, params=dict(palette=pal, dither=70, pixelSize=8), tol=1.6, frac=0.002, palette=pal, dither=70, pix=8)
compare('palette without dither', ssrc, s8, params=dict(palette='PICO-8'), frac=0.002, palette='PICO-8')

# 3. every look, and the same three on the full photo (cut into bands by the host)
for lk in [k for k in LOOKS if k != 'Custom']:
    compare(f'look "{lk}"', ssrc, s8, look=lk, frac=0.002)
for lk in ['VHS Tape', 'Glitch', 'Game Boy', 'CRT Monitor']:
    compare(f'seamless across bands ("{lk}", full photo)', psrc, p8, look=lk, frac=0.002)

# 4. palette results contain only palette colours
o, _ = run(ssrc, dict(look='Game Boy'), os.path.join(TMP, 'gb.png'))
colours = {tuple(np.round(c * 255).astype(int)) for c in o.reshape(-1, 3)}
check('Game Boy output uses only its four greens', colours <= {(15, 56, 15), (48, 98, 48), (139, 172, 15), (155, 188, 15)}, f'{len(colours)} colours')
o, _ = run(ssrc, dict(look='1-Bit Dither'), os.path.join(TMP, '1b.png'))
check('1-Bit output is only black and white', set(np.unique(np.round(o * 255))) <= {0.0, 255.0})
# pixel blocks really are blocks
o, _ = run(ssrc, dict(look='Custom', pixelSize=10), os.path.join(TMP, 'pb.png'))
b = rnd(10 * 420 / 1000)
check('pixelate makes constant blocks', np.abs(o[:b, :b] - o[0, 0]).max() * 255 <= 1.01 and np.abs(o[b:2 * b, :b] - o[b, 0]).max() * 255 <= 1.01, f'block {b}px')

# 5. strength, alpha, defaults, seed, repeatability
o0, _ = run(ssrc, dict(look='VHS Tape', strength=0), os.path.join(TMP, 's0.png'))
check('strength 0 returns the original', np.abs(o0 * 255 - s8).max() <= 1)
compare('strength 50', ssrc, s8, look='CRT Monitor', params=dict(strength=50), strength=50)
a1, _ = run(ssrc, dict(look='Glitch', seed=1), os.path.join(TMP, 'a1.png')); a1b, _ = run(ssrc, dict(look='Glitch', seed=1), os.path.join(TMP, 'a1b.png'))
a2, _ = run(ssrc, dict(look='Glitch', seed=2), os.path.join(TMP, 'a2.png'))
check('same seed gives the same picture', np.array_equal(a1, a1b)); check('a different seed gives a different picture', not np.array_equal(a1, a2))
rgba = Image.open(ssrc).convert('RGBA'); a = np.asarray(rgba).copy(); a[:, :, 3] = np.linspace(255, 40, a.shape[1]).astype(np.uint8)[None, :]
Image.fromarray(a).save(os.path.join(TMP, 'rgba.png'))
run(os.path.join(TMP, 'rgba.png'), dict(look='VHS Tape'), os.path.join(TMP, 'rgba_o.png'))
oa = np.asarray(Image.open(os.path.join(TMP, 'rgba_o.png')).convert('RGBA'))
check('alpha channel is kept', np.abs(oa[:, :, 3].astype(int) - a[:, :, 3]).max() <= 1)
od, _ = run(ssrc, {}, os.path.join(TMP, 'def.png')); ov, _ = run(ssrc, dict(look='VHS Tape'), os.path.join(TMP, 'vhs.png'))
check('default parameters = VHS Tape', np.abs(od - ov).max() == 0)

# 6. grayscale documents are refused cleanly (RGB only), leaving the picture alone
Image.fromarray(np.asarray(small.convert('L'))).save(os.path.join(TMP, 'gray.png'))
r = subprocess.run([CLI, 'run', os.path.join(TMP, 'gray.png'), '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
                    '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': {}}), '--out', os.path.join(TMP, 'gray_o.png')], capture_output=True, text=True)
check('grayscale document is refused with an error', r.returncode != 0, (r.stderr.strip().splitlines() or [''])[-1][:100])

# 7. speed on bigger pictures, with the heaviest looks
for (w, h) in [(2800, 1854), (6000, 4000)]:
    im = photo.resize((w, h), Image.BILINEAR); p = os.path.join(TMP, f'b{w}.png'); im.save(p)
    for lk in ['VHS Tape', 'Glitch', 'PICO-8']:
        _, secs = run(p, dict(look=lk), os.path.join(TMP, 'big_o.png'))
        check(f'{w}x{h} ({w * h / 1e6:.0f} MP) with {lk} finishes in time', secs < 55, f'{secs:.1f}s')

# 8. the preset tool knows exactly the plug-in's parameters and looks
sys.path.insert(0, os.path.join(HERE, '..', 'tools'))
import retro_presets as rp
r = subprocess.run([CLI, 'run', ssrc, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)})], capture_output=True, text=True)
schema = json.loads(r.stdout.splitlines()[0])['result']['paramsSchema']
check('preset tool lists the plug-in looks', rp.LOOKS == schema['look']['options'])
check('preset tool lists the palettes', rp.PALETTES == schema['palette']['options'])
check('preset tool ranges match the manifest', all(k in schema and (schema[k]['min'], schema[k]['max']) == tuple(v) for k, v in rp.RANGES.items()) and set(schema) == set(rp.RANGES) | {'look', 'palette'})

print(f'\nwork files: {TMP}')
print('ALL PASSED' if not fails else 'FAILED: ' + ', '.join(fails))
sys.exit(1 if fails else 0)
