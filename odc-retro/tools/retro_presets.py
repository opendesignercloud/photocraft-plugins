#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
"""Manage your own ODC Retro presets, and apply any preset to a file.

PhotoCraft plug-ins run in a sandbox with no storage, and the plug-in dialog has no preset menu (yet), so presets live
outside the plug-in: a JSON file of named parameter sets that this tool edits and replays through photocraft-cli.

  retro_presets.py list                          the 8 built-in looks and your own presets
  retro_presets.py show NAME
  retro_presets.py add NAME [--from LOOK_OR_PRESET] [param=value ...]
  retro_presets.py edit NAME param=value ...      change some values, keep the rest
  retro_presets.py delete NAME
  retro_presets.py apply NAME IN OUT [param=value ...]    run it on a picture (any format photocraft-cli writes)

Parameters are the plug-in's own (look, palette, strength, pixelSize, dither, scanlines, lineSize, phosphor, colorSmear,
colorSplit, glitch, static, saturation, contrast, vignette, seed); the sliders are adjustments added to the look. Built-in looks cannot be changed or
deleted: use `add MY-NAME --from "Game Boy" dither=40` to make your own version.
Presets file: ~/.config/odc-retro/presets.json (override with ODC_RETRO_PRESETS).
"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
WASM = os.path.join(HERE, '..', 'dist', 'odc-retro.wasm')
CLI = os.environ.get('PHOTOCRAFT_CLI', '/Applications/PhotoCraft.app/Contents/MacOS/photocraft-cli')
STORE = os.environ.get('ODC_RETRO_PRESETS') or os.path.expanduser('~/.config/odc-retro/presets.json')
ID = 'org.opendesignercloud.retro'
LOOKS = ['VHS Tape', 'Custom', 'CRT Monitor', 'Game Boy', '1-Bit Dither', 'CGA Arcade', 'PICO-8', 'Amber Terminal', 'Glitch']
PALETTES = ['As the look', 'None', 'Game Boy', '1-Bit', 'Grayscale 4', 'CGA Cyan Magenta', 'CGA Red Green', 'PICO-8', 'Amber Terminal', 'Green Phosphor']
RANGES = {'strength': (0, 100), 'pixelSize': (-20, 20), 'dither': (-100, 100), 'scanlines': (-100, 100), 'phosphor': (-100, 100), 'colorSmear': (-100, 100), 'colorSplit': (-100, 100), 'glitch': (-100, 100), 'static': (-100, 100), 'saturation': (-100, 100), 'contrast': (-100, 100), 'vignette': (-100, 100), 'lineSize': (-3, 3), 'seed': (0, 9999)}


def load():
    try:
        with open(STORE) as f:
            data = json.load(f)
    except FileNotFoundError:
        return {}
    if not isinstance(data, dict): sys.exit(f'{STORE} is not a preset file (expected a JSON object)')
    return data


def save(data):
    os.makedirs(os.path.dirname(STORE), exist_ok=True)
    tmp = STORE + '.tmp'
    with open(tmp, 'w') as f: json.dump(data, f, indent=2, sort_keys=True); f.write('\n')
    os.replace(tmp, STORE)


def parse(pairs):
    out = {}
    for p in pairs:
        if '=' not in p: sys.exit(f'expected param=value, got "{p}"')
        k, v = p.split('=', 1)
        if k == 'palette':
            if v not in PALETTES: sys.exit(f'unknown palette "{v}". Palettes: ' + ', '.join(PALETTES))
            out[k] = v
        elif k == 'look':
            if v not in LOOKS: sys.exit(f'unknown look "{v}". Looks: ' + ', '.join(LOOKS))
            out[k] = v
        elif k in RANGES:
            try: n = float(v)
            except ValueError: sys.exit(f'{k} must be a number, got "{v}"')
            lo, hi = RANGES[k]
            if not lo <= n <= hi: sys.exit(f'{k} must be between {lo} and {hi}, got {v}')
            out[k] = int(n) if n == int(n) else n
        else: sys.exit(f'unknown parameter "{k}". Parameters: look, palette, ' + ', '.join(RANGES))
    return out


def resolve(name, data):
    """A preset name or a built-in look name -> its parameter set."""
    if name in data: return dict(data[name])
    for l in LOOKS:
        if l.lower() == name.lower(): return {'look': l}
    sys.exit(f'no preset or look called "{name}". Run `list`.')


def user_key(name, data):
    for k in data:
        if k.lower() == name.lower(): return k
    return None


def check_name(name):
    if not name.strip() or len(name) > 64: sys.exit('a preset name is 1 to 64 characters')
    if name.lower() in (l.lower() for l in LOOKS): sys.exit(f'"{name}" is a built-in look; pick another name (or use --from "{name}")')


def main(a):
    if not a or a[0] in ('-h', '--help', 'help'): print(__doc__); return
    cmd, rest = a[0], a[1:]
    data = load()
    if cmd == 'list':
        print('Built-in looks:'); [print('  ' + l) for l in LOOKS]
        print('Your presets (%s):' % STORE)
        for k in sorted(data, key=str.lower): print('  %s  %s' % (k, json.dumps(data[k])))
        if not data: print('  (none yet: add one with `add NAME ...`)')
    elif cmd == 'show':
        if not rest: sys.exit('show NAME')
        print(json.dumps(resolve(rest[0], data), indent=2))
    elif cmd == 'add':
        if not rest: sys.exit('add NAME [--from LOOK_OR_PRESET] [param=value ...]')
        name, rest = rest[0], rest[1:]
        check_name(name)
        if user_key(name, data): sys.exit(f'a preset called "{user_key(name, data)}" exists; use `edit` to change it or `delete` first')
        base = {}
        if rest[:1] == ['--from']:
            if len(rest) < 2: sys.exit('--from needs a look or preset name')
            base, rest = resolve(rest[1], data), rest[2:]
        base.update(parse(rest)); base.setdefault('look', 'VHS Tape')
        data[name] = base; save(data); print(f'added "{name}": {json.dumps(base)}')
    elif cmd == 'edit':
        if len(rest) < 2: sys.exit('edit NAME param=value ...')
        k = user_key(rest[0], data)
        if not k:
            sys.exit(f'"{rest[0]}" is a built-in look, which cannot be edited. Make your own with `add NAME --from "{rest[0]}" ...`' if rest[0].lower() in (l.lower() for l in LOOKS) else f'no preset called "{rest[0]}"')
        data[k].update(parse(rest[1:])); save(data); print(f'updated "{k}": {json.dumps(data[k])}')
    elif cmd == 'delete':
        if not rest: sys.exit('delete NAME')
        k = user_key(rest[0], data)
        if not k: sys.exit('built-in looks cannot be deleted' if rest[0].lower() in (l.lower() for l in LOOKS) else f'no preset called "{rest[0]}"')
        del data[k]; save(data); print(f'deleted "{k}"')
    elif cmd == 'apply':
        if len(rest) < 3: sys.exit('apply NAME IN OUT [param=value ...]')
        params = resolve(rest[0], data); params.update(parse(rest[3:]))
        r = subprocess.run([CLI, 'run', rest[1], '--cmd', 'plugin.install', '--params', json.dumps({'path': os.path.abspath(WASM)}),
                            '--cmd', 'plugin.run', '--params', json.dumps({'id': ID, 'params': params}), '--out', rest[2]], capture_output=True, text=True)
        if r.returncode != 0: sys.exit(r.stderr.strip() or r.stdout.strip())
        print(f'wrote {rest[2]} with {json.dumps(params)}')
    else: sys.exit(f'unknown command "{cmd}". Try --help')


if __name__ == '__main__':
    main(sys.argv[1:])
