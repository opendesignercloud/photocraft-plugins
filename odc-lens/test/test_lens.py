#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
"""End-to-end tests for ODC Lens through photocraft-cli (headless PhotoCraft).
Run ./build.sh first, then: python3 test/test_lens.py [path/to/photo.jpg]

Every effect is checked against HyperImage's own Lens code (test/reference.js, copied verbatim from renderer/lens.js)
run in node. The big picture is cut into several bands by the host, so a match there also proves the overlap works
(no seams). Needs node, numpy, Pillow."""
import colorsys, json, os, shutil, subprocess, sys, tempfile, time
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
WASM = os.path.join(HERE, '..', 'dist', 'odc-lens.wasm')
CLI = os.environ.get('PHOTOCRAFT_CLI') or shutil.which('photocraft-cli') or '/Applications/PhotoCraft.app/Contents/MacOS/photocraft-cli'
PHOTO = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, '..', '..', 'odc-film-look', 'examples', 'original.jpg')
TMP = tempfile.mkdtemp(prefix='odc-lens-')
ID = 'org.opendesignercloud.lens'
fails = []

# Custom look defaults (the plug-in's NEUTRAL): sliders are offsets from these.
BASE = dict(distortion=0, caLateral=0, softness=0, softStart=40, caLong=0, bloom=0, bloomRadius=24, bloomThreshold=70,
            hue=0, tint=0, vignette=0)


def run(src, params, out, extra_ok=False):
    cmd = [CLI, 'run', src, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
           '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': params}), '--out', out]
    t = time.time()
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        if extra_ok: return None
        raise SystemExit('photocraft-cli failed:\n' + r.stdout + r.stderr)
    return np.asarray(Image.open(out).convert('RGB'), dtype=np.float64) / 255, time.time() - t


def effective(**abs_):
    """Absolute lens settings -> (plug-in params as offsets from Custom, settings for reference.js)."""
    s = dict(BASE); s.update(abs_)
    off = dict(look='Custom', distortion=s['distortion'] - BASE['distortion'], caLateral=s['caLateral'],
               softness=s['softness'], softStart=s['softStart'] - BASE['softStart'], caLong=s['caLong'],
               bloom=s['bloom'], bloomRadius=s['bloomRadius'] - BASE['bloomRadius'],
               bloomThreshold=s['bloomThreshold'] - BASE['bloomThreshold'],
               bloomHue=((s['hue'] + 180) % 360) - 180,  # the slider is -180..180 and the hue wraps
               bloomTint=s['tint'],
               vignette=s['vignette'])
    r, g, b = colorsys.hsv_to_rgb((s['hue'] % 360) / 360, s['tint'] / 100, 1)
    ref = dict(distortion=s['distortion'], caLateral=s['caLateral'], softness=s['softness'], softStart=s['softStart'],
               caLong=s['caLong'], bloom=s['bloom'], bloomRadius=s['bloomRadius'], bloomThreshold=s['bloomThreshold'],
               bloomTint='#%02x%02x%02x' % (round(r * 255), round(g * 255), round(b * 255)), vignette=s['vignette'])
    return off, ref


def reference(img8, ref, order='parallel'):
    h, w, _ = img8.shape
    data = json.dumps({'w': w, 'h': h, 'settings': ref, 'order': order, 'pixels': img8.reshape(-1).astype(float).tolist()})
    r = subprocess.run(['node', os.path.join(HERE, 'reference.js')], input=data, capture_output=True, text=True)
    if r.returncode != 0: raise SystemExit('node failed:\n' + r.stderr)
    return np.clip(np.array(json.loads(r.stdout)).reshape(h, w, 3), 0, 255) / 255


def check(name, ok, detail=''):
    print(('PASS ' if ok else 'FAIL ') + name + (' ' + detail if detail else ''))
    if not ok: fails.append(name)


def compare(name, src, img8, tol=2.0, order='parallel', **abs_):
    off, ref = effective(**abs_)
    o, secs = run(src, off, os.path.join(TMP, 'o.png'))
    e = reference(img8, ref, order)
    d = np.abs(o - e) * 255
    check(name, d.max() <= tol, f'max diff {d.max():.2f} levels, mean {d.mean():.3f} ({secs:.1f}s)')
    return o


photo = Image.open(PHOTO).convert('RGB')
psrc = os.path.join(TMP, 'photo.png'); photo.save(psrc)
p8 = np.asarray(photo)
H, W = p8.shape[:2]
print(f'photo {W}x{H}, cli {CLI}')

# small picture (a single band, no seams possible) for the per-effect checks
small = photo.resize((420, 280), Image.LANCZOS)
ssrc = os.path.join(TMP, 'small.png'); small.save(ssrc)
s8 = np.asarray(small)

# 1. everything off: output equals input
o, _ = run(ssrc, dict(look='Custom'), os.path.join(TMP, 'id.png'))
check('identity when every effect is off', np.abs(o * 255 - s8).max() <= 1, f'max diff {np.abs(o * 255 - s8).max():.1f}')

# 2. each effect against HyperImage's code, one at a time (small picture)
compare('vignette', ssrc, s8, vignette=70)
compare('lateral chromatic aberration', ssrc, s8, caLateral=80)
compare('barrel distortion', ssrc, s8, distortion=40)
compare('pincushion distortion', ssrc, s8, distortion=-35)
compare('distortion + lateral CA', ssrc, s8, distortion=25, caLateral=50)
compare('softness (blur at the corners)', ssrc, s8, softness=60, softStart=30)
compare('softness + longitudinal CA', ssrc, s8, softness=60, softStart=30, caLong=60)
compare('bloom (white tint)', ssrc, s8, bloom=50, bloomThreshold=45, bloomRadius=30)
compare('bloom (orange tint)', ssrc, s8, bloom=60, bloomThreshold=50, hue=19, tint=58)

# 3. all together, in the plug-in's own order, then how far that is from HyperImage's order
allfx = dict(distortion=18, caLateral=38, softness=40, softStart=28, caLong=-28, bloom=45, bloomRadius=42,
             bloomThreshold=55, hue=217, tint=38, vignette=30)
# (bloom radius 12 px is blurred at reduced resolution, which is a little looser at the very edge: tolerance 6)
o = compare('all effects together', ssrc, s8, tol=6.0, **allfx)
_, ref = effective(**allfx)
hy = reference(s8, ref, 'hyper')
d = np.abs(o - hy) * 255
print(f'INFO  bloom from the geometry result instead of the softened image: max {d.max():.1f}, mean {d.mean():.3f} levels vs HyperImage order')
check('close to HyperImage order (a deliberate difference)', d.mean() < 2.5, f'mean {d.mean():.3f}')

# 4. the full-size photo is cut into several bands: must match the whole-image reference (no seams)
compare('seamless across bands (full photo, all effects)', psrc, p8, tol=2.0, **dict(allfx, bloomRadius=24))

# 5. strength mixes with the original
off, ref = effective(vignette=100)
o50, _ = run(ssrc, dict(off, strength=50), os.path.join(TMP, 's50.png'))
o100, _ = run(ssrc, off, os.path.join(TMP, 's100.png'))
mix = (o100 + s8 / 255) / 2
check('strength 50 is halfway to the original', np.abs(o50 - mix).max() * 255 <= 1.5, f'{np.abs(o50 - mix).max() * 255:.2f}')

# 6. alpha passes through untouched; transparent source stays transparent
rgba = Image.open(ssrc).convert('RGBA'); a = np.asarray(rgba).copy(); a[:, :, 3] = np.linspace(255, 40, a.shape[1]).astype(np.uint8)[None, :]
Image.fromarray(a).save(os.path.join(TMP, 'rgba.png'))
run(os.path.join(TMP, 'rgba.png'), dict(look='Vintage Glass'), os.path.join(TMP, 'rgba_o.png'))
oa = np.asarray(Image.open(os.path.join(TMP, 'rgba_o.png')).convert('RGBA'))
check('alpha channel is kept', np.abs(oa[:, :, 3].astype(int) - a[:, :, 3]).max() <= 1)

# 7. every look runs, changes the picture, and the default look is Vintage Glass
looks = ['Vintage Glass', 'Anamorphic', 'Dreamy Portrait', 'Halation', 'Toy Camera', 'Subtle']
for lk in looks:
    o, secs = run(ssrc, dict(look=lk), os.path.join(TMP, 'look.png'))
    check(f'look "{lk}" changes the picture', np.abs(o * 255 - s8).mean() > 0.3, f'mean change {np.abs(o * 255 - s8).mean():.2f} ({secs:.1f}s)')
od, _ = run(ssrc, {}, os.path.join(TMP, 'def.png'))
ov, _ = run(ssrc, dict(look='Vintage Glass'), os.path.join(TMP, 'vg.png'))
check('default parameters = Vintage Glass', np.abs(od - ov).max() == 0)

# 8. a grayscale document works (CA and tint have no effect on one channel)
Image.fromarray(np.asarray(small.convert('L'))).save(os.path.join(TMP, 'gray.png'))
r = run(os.path.join(TMP, 'gray.png'), dict(look='Anamorphic'), os.path.join(TMP, 'gray_o.png'), extra_ok=True)
check('grayscale document runs', r is not None)

# 9. big image: the budget keeps big radii inside the overlap (does not fail, does not seam)
big = photo.resize((W * 2, H * 2), Image.LANCZOS)
bsrc = os.path.join(TMP, 'big.png'); big.save(bsrc)
o, secs = run(bsrc, dict(look='Toy Camera'), os.path.join(TMP, 'big_o.png'))
check('2800x1854 with Toy Camera (limits apply)', o.shape == (big.size[1], big.size[0], 3) and np.isfinite(o).all(), f'{secs:.1f}s')

print(f'\nwork files: {TMP}')
print('ALL PASSED' if not fails else 'FAILED: ' + ', '.join(fails))
sys.exit(1 if fails else 0)
