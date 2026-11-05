#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""End-to-end tests for ODC Tone through photocraft-cli (headless PhotoCraft).
Run ./build.sh first, then: python3 test/test_tone.py [path/to/photo.jpg]

Every step is checked against HyperImage's own code (test/reference.js: the Adjust Color grading plus Depth, Sharpness and
Texture, copied verbatim from renderer/color.js) run in node on the whole image. The big photo is cut into several bands
by the host, so a match there also proves the overlap works (no seams). Needs node, numpy, Pillow."""
import json, os, shutil, subprocess, sys, tempfile, time
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
WASM = os.path.join(HERE, '..', 'dist', 'odc-tone.wasm')
CLI = os.environ.get('PHOTOCRAFT_CLI') or shutil.which('photocraft-cli') or '/Applications/PhotoCraft.app/Contents/MacOS/photocraft-cli'
PHOTO = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, '..', 'examples', 'original.jpg')
TMP = tempfile.mkdtemp(prefix='odc-tone-')
ID = 'org.opendesignercloud.tone'
fails = []

# HyperImage's settings for each look (renderer/color.js). The plug-in's names differ for: black -> blackPoint,
# white -> whitePoint (as an offset from 255), temp -> temperature, stAmount -> splitTone, depth -> localContrast, sharp -> sharpness.
LOOKS = {
    'Web Punch': dict(contrast=10, vibrance=15, depth=12, texture=10, sharp=30),
    'Interior / Real Estate': dict(highlights=-35, shadows=30, contrast=8, vibrance=10, temp=4, depth=20, sharp=18),
    'Twilight Exterior': dict(black=6, highlights=-15, shadows=22, contrast=10, vibrance=20, depth=15, sharp=12,
                              stShadow='#2f5c8f', stHighlight='#f0c489', stBalance=-10, stAmount=16),
    'Overcast Day': dict(black=8, contrast=15, highlights=-8, shadows=10, vibrance=18, temp=12, depth=15, sharp=10),
    'Backlit Rescue': dict(highlights=-30, shadows=45, contrast=10, vibrance=8, depth=10),
    'Product on White': dict(white=244, black=6, contrast=10, highlights=8, saturation=4, sharp=25),
    'Soft Portrait': dict(highlights=-12, shadows=12, contrast=-5, vibrance=8, temp=3, texture=-30, sharp=10),
    'Clarity Punch': dict(highlights=-20, shadows=15, contrast=12, depth=40, texture=25, sharp=15),
}
RENAME = {'black': 'blackPoint', 'temp': 'temperature', 'stAmount': 'splitTone', 'depth': 'localContrast', 'sharp': 'sharpness'}


def to_params(s, look='Custom'):
    """HyperImage-style absolute settings -> plug-in parameters (offsets from Custom)."""
    p = dict(look=look)
    for k, v in s.items():
        if k == 'white': p['whitePoint'] = v - 255
        elif k in ('stShadow', 'stHighlight', 'stBalance'): continue
        else: p[RENAME.get(k, k)] = v
    return p


def run(src, params, out):
    cmd = [CLI, 'run', src, '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
           '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': params}), '--out', out]
    t = time.time()
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0: raise SystemExit('photocraft-cli failed:\n' + r.stdout[-300:] + r.stderr)
    return np.asarray(Image.open(out).convert('RGB'), dtype=np.float64) / 255, time.time() - t


def reference(img8, settings):
    h, w, _ = img8.shape
    data = json.dumps({'w': w, 'h': h, 'settings': settings, 'pixels': img8.reshape(-1).astype(float).tolist()})
    r = subprocess.run(['node', os.path.join(HERE, 'reference.js')], input=data, capture_output=True, text=True)
    if r.returncode != 0: raise SystemExit('node failed:\n' + r.stderr)
    return np.clip(np.array(json.loads(r.stdout)).reshape(h, w, 3), 0, 255) / 255


def check(name, ok, detail=''):
    print(('PASS ' if ok else 'FAIL ') + name + (' ' + detail if detail else ''))
    if not ok: fails.append(name)


def compare(name, src, img8, settings, look='Custom', tol=1.5, mean_tol=None):
    # a named look carries its own values, so only its name is sent; Custom gets the values as sliders
    o, secs = run(src, dict(look=look) if look != 'Custom' else to_params(settings), os.path.join(TMP, 'o.png'))
    d = np.abs(o - reference(img8, settings)) * 255
    check(name, d.max() <= tol and (mean_tol is None or d.mean() <= mean_tol), f'max diff {d.max():.2f} levels, mean {d.mean():.3f} ({secs:.1f}s)')
    return o


photo = Image.open(PHOTO).convert('RGB')
psrc = os.path.join(TMP, 'photo.png'); photo.save(psrc)
p8 = np.asarray(photo); H, W = p8.shape[:2]
print(f'photo {W}x{H}, cli {CLI}')
small = photo.resize((420, 280), Image.LANCZOS)
ssrc = os.path.join(TMP, 'small.png'); small.save(ssrc); s8 = np.asarray(small)

# 1. everything off
o, _ = run(ssrc, dict(look='Custom'), os.path.join(TMP, 'id.png'))
check('identity when every control is 0', np.abs(o * 255 - s8).max() <= 1, f'{np.abs(o * 255 - s8).max():.1f}')

# 2. the tone and colour sliders (per pixel)
compare('exposure, contrast', ssrc, s8, dict(exposure=30, contrast=25))
compare('highlights, shadows', ssrc, s8, dict(highlights=-50, shadows=50))
compare('black and white point', ssrc, s8, dict(black=20, white=235))
compare('vibrance, saturation', ssrc, s8, dict(vibrance=40, saturation=-20))
compare('temperature, tint', ssrc, s8, dict(temp=30, tint=-15))
compare('split tone (HyperImage default colours)', ssrc, s8, dict(stAmount=40))

# 3. the spatial steps, one at a time
compare('local contrast (Depth)', ssrc, s8, dict(depth=40))
compare('local contrast at 100', ssrc, s8, dict(depth=100))
compare('sharpness (unsharp mask)', ssrc, s8, dict(sharp=50))
compare('texture +', ssrc, s8, dict(texture=60))
compare('texture - (smoothing)', ssrc, s8, dict(texture=-60))
compare('all steps together', ssrc, s8, dict(contrast=10, highlights=-20, shadows=20, vibrance=15, depth=30, sharp=25, texture=20))

# 4. every look
for lk, s in LOOKS.items():
    compare(f'look "{lk}"', ssrc, s8, s, look=lk)

# 5. the full-size photo is cut into bands by the host: must match the whole-image reference (no seams)
for lk in ['Clarity Punch', 'Interior / Real Estate', 'Soft Portrait']:
    compare(f'seamless across bands ("{lk}", full photo)', psrc, p8, LOOKS[lk], look=lk)

# 6. strength, alpha, default, gray
o50, _ = run(ssrc, dict(look='Clarity Punch', strength=50), os.path.join(TMP, 's50.png'))
o100, _ = run(ssrc, dict(look='Clarity Punch'), os.path.join(TMP, 's100.png'))
check('strength 50 is halfway to the original', np.abs(o50 - (o100 + s8 / 255) / 2).max() * 255 <= 1.5)
rgba = Image.open(ssrc).convert('RGBA'); a = np.asarray(rgba).copy(); a[:, :, 3] = np.linspace(255, 40, a.shape[1]).astype(np.uint8)[None, :]
Image.fromarray(a).save(os.path.join(TMP, 'rgba.png'))
run(os.path.join(TMP, 'rgba.png'), dict(look='Interior / Real Estate'), os.path.join(TMP, 'rgba_o.png'))
oa = np.asarray(Image.open(os.path.join(TMP, 'rgba_o.png')).convert('RGBA'))
check('alpha channel is kept', np.abs(oa[:, :, 3].astype(int) - a[:, :, 3]).max() <= 1)
od, _ = run(ssrc, {}, os.path.join(TMP, 'def.png')); ow, _ = run(ssrc, dict(look='Web Punch'), os.path.join(TMP, 'wp.png'))
check('default parameters = Web Punch', np.abs(od - ow).max() == 0)
Image.fromarray(np.asarray(small.convert('L'))).save(os.path.join(TMP, 'gray.png'))
run(os.path.join(TMP, 'gray.png'), dict(look='Clarity Punch'), os.path.join(TMP, 'gray_o.png'))
check('grayscale document runs', True)

# 7. speed on bigger pictures
for (w, h) in [(2800, 1854), (4000, 2670)]:
    im = photo.resize((w, h), Image.BILINEAR); p = os.path.join(TMP, f'b{w}.png'); im.save(p)
    _, secs = run(p, dict(look='Clarity Punch'), os.path.join(TMP, 'big_o.png'))
    check(f'{w}x{h} ({w * h / 1e6:.0f} MP) with Clarity Punch finishes in time', secs < 55, f'{secs:.1f}s')

print(f'\nwork files: {TMP}')
print('ALL PASSED' if not fails else 'FAILED: ' + ', '.join(fails))
sys.exit(1 if fails else 0)
