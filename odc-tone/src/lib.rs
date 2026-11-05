// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
//! ODC Tone: a PhotoCraft filter plug-in (plug-in ABI v1, see PhotoCraft `docs/plugins.md`).
//!
//! The corrective half of HyperImage's Adjust Color tool: the tone sliders (exposure, contrast, highlights,
//! shadows, black and white point, vibrance, saturation, temperature, tint, split tone) followed by the three
//! spatial steps that look at neighbouring pixels: **Local contrast** (HyperImage's "Depth": a soft-light blend
//! against a wide blur), **Sharpness** (unsharp mask) and **Texture** (a band-pass on luminance).
//! A **look** (Interior / Real Estate, Twilight Exterior, Overcast Day, Backlit Rescue, Product on White, Web Punch,
//! Soft Portrait, Clarity Punch) sets every amount; the sliders are adjustments ADDED to the look.
//! `no_std`, no imports, no maths library. The three spatial steps run one after the other, so their reaches add:
//! local contrast radius + 1 + texture radius must fit the 256 px overlap, and are scaled back together if they do not
//! (that happens only on photos wider than about 9000 px). The grading is clamped to 0..1 before the spatial steps, as
//! HyperImage's 8-bit path does.
#![no_std]
#![allow(dead_code)]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

const MANIFEST: &str = r#"{
  "id": "org.opendesignercloud.tone",
  "name": "ODC Tone…",
  "version": "1.0.0",
  "kind": "filter",
  "author": "Open Designer Cloud",
  "description": "Fix a picture's light: pull back a bright window, open the shadows, add local contrast, texture and sharpness. Looks for interiors and real estate, twilight exteriors, overcast days, backlit subjects, products on white, web export, soft portraits and heavy clarity. The sliders adjust the chosen look. Open source (Apache 2.0): opendesignercloud.com",
  "params": {
    "look":          {"type": "choice", "options": ["Web Punch", "Custom", "Interior / Real Estate", "Twilight Exterior", "Overcast Day", "Backlit Rescue", "Product on White", "Soft Portrait", "Clarity Punch"], "default": "Web Punch"},
    "strength":      {"type": "number", "min": 0,    "max": 100, "default": 100},
    "exposure":      {"type": "number", "min": -100, "max": 100, "default": 0},
    "contrast":      {"type": "number", "min": -100, "max": 100, "default": 0},
    "highlights":    {"type": "number", "min": -100, "max": 100, "default": 0},
    "shadows":       {"type": "number", "min": -100, "max": 100, "default": 0},
    "blackPoint":    {"type": "number", "min": -100, "max": 100, "default": 0},
    "whitePoint":    {"type": "number", "min": -100, "max": 100, "default": 0},
    "vibrance":      {"type": "number", "min": -100, "max": 100, "default": 0},
    "saturation":    {"type": "number", "min": -100, "max": 100, "default": 0},
    "temperature":   {"type": "number", "min": -100, "max": 100, "default": 0},
    "tint":          {"type": "number", "min": -100, "max": 100, "default": 0},
    "splitTone":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "localContrast": {"type": "number", "min": -100, "max": 100, "default": 0},
    "sharpness":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "texture":       {"type": "number", "min": -100, "max": 100, "default": 0}
  },
  "overlap": 256,
  "area": "content"
}"#;

// ---- the looks (HyperImage's corrective presets, color.js:752-835) ----

/// Everything a look sets, in HyperImage's units (levels 0..255, sliders -100..100).
#[derive(Clone, Copy)]
struct Look {
    name: &'static str,
    black: f32, gamma: f32, white: f32, exposure: f32, contrast: f32, highlights: f32, shadows: f32,
    vibrance: f32, saturation: f32, temp: f32, tint: f32,
    st_shadow: [f32; 3], st_highlight: [f32; 3], st_balance: f32, st_amount: f32,
    depth: f32, sharp: f32, texture: f32,
}

const NEUTRAL: Look = Look {
    name: "Custom",
    black: 0.0, gamma: 100.0, white: 255.0, exposure: 0.0, contrast: 0.0, highlights: 0.0, shadows: 0.0,
    vibrance: 0.0, saturation: 0.0, temp: 0.0, tint: 0.0,
    st_shadow: [58.0, 110.0, 165.0], st_highlight: [232.0, 184.0, 120.0], st_balance: 0.0, st_amount: 0.0,
    depth: 0.0, sharp: 0.0, texture: 0.0,
};

/// In the same order as the manifest's options.
const LOOKS: [Look; 9] = [
    Look { name: "Web Punch", contrast: 10.0, vibrance: 15.0, depth: 12.0, texture: 10.0, sharp: 30.0, ..NEUTRAL },
    NEUTRAL,
    // A window three stops brighter than the room: highlights back first, then shadows, then contrast.
    Look { name: "Interior / Real Estate", highlights: -35.0, shadows: 30.0, contrast: 8.0, vibrance: 10.0, temp: 4.0, depth: 20.0, sharp: 18.0, ..NEUTRAL },
    // Dusk exteriors: open the building without draining the sky; blue shadows, warm windows.
    Look { name: "Twilight Exterior", black: 6.0, highlights: -15.0, shadows: 22.0, contrast: 10.0, vibrance: 20.0, depth: 15.0, sharp: 12.0,
        st_shadow: [47.0, 92.0, 143.0], st_highlight: [240.0, 196.0, 137.0], st_balance: -10.0, st_amount: 16.0, ..NEUTRAL },
    // Flat grey light has no black point and no warmth. Give it both.
    Look { name: "Overcast Day", black: 8.0, contrast: 15.0, highlights: -8.0, shadows: 10.0, vibrance: 18.0, temp: 12.0, depth: 15.0, sharp: 10.0, ..NEUTRAL },
    // A subject against a bright window or sky: big shadow lift, highlights held down, mild contrast.
    Look { name: "Backlit Rescue", highlights: -30.0, shadows: 45.0, contrast: 10.0, vibrance: 8.0, depth: 10.0, ..NEUTRAL },
    // A white sweep: lowering the white POINT forces a near-white background to actually reach white.
    Look { name: "Product on White", white: 244.0, black: 6.0, contrast: 10.0, highlights: 8.0, saturation: 4.0, sharp: 25.0, ..NEUTRAL },
    // Negative texture smooths skin at the medium scale while sharpness keeps lashes and hair crisp.
    Look { name: "Soft Portrait", highlights: -12.0, shadows: 12.0, contrast: -5.0, vibrance: 8.0, temp: 3.0, texture: -30.0, sharp: 10.0, ..NEUTRAL },
    // Heavy local contrast for architecture and detail. Strong: back Local contrast off if halos appear.
    Look { name: "Clarity Punch", highlights: -20.0, shadows: 15.0, contrast: 12.0, depth: 40.0, texture: 25.0, sharp: 15.0, ..NEUTRAL },
];

const HAS_ALPHA: u32 = 1 << 8;
const OVERLAP: f32 = 256.0;

#[unsafe(no_mangle)]
pub extern "C" fn pc_abi_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn pc_manifest() -> u64 {
    ((MANIFEST.len() as u64) << 32) | MANIFEST.as_ptr() as u64
}

/// Fresh pages from `memory.grow` (every run gets a fresh instance, so nothing is freed).
#[unsafe(no_mangle)]
pub extern "C" fn pc_alloc(size: u32) -> u32 {
    let pages = (size as usize).div_ceil(65536);
    let old = core::arch::wasm32::memory_grow(0, pages);
    if old == usize::MAX { 0 } else { (old * 65536) as u32 }
}

/// A zeroed block of `n` floats.
fn plane(n: usize) -> Option<&'static mut [f32]> {
    let at = pc_alloc((n * 4) as u32);
    if at == 0 {
        return None;
    }
    // SAFETY: `pc_alloc` returned a fresh block of `n * 4` bytes inside our memory, used once.
    Some(unsafe { core::slice::from_raw_parts_mut(at as usize as *mut f32, n) })
}

// ---- tiny JSON number reader: finds `"key":` and parses the number after it ----

fn find(h: &[u8], key: &str) -> Option<usize> {
    let k = key.as_bytes();
    let n = k.len() + 3; // quote key quote colon
    if h.len() < n {
        return None;
    }
    'outer: for i in 0..=h.len() - n {
        if h[i] != b'"' || h[i + 1 + k.len()] != b'"' || h[i + 2 + k.len()] != b':' {
            continue;
        }
        for j in 0..k.len() {
            if h[i + 1 + j] != k[j] {
                continue 'outer;
            }
        }
        return Some(i + n);
    }
    None
}

fn num(h: &[u8], key: &str, default: f32) -> f32 {
    let Some(mut i) = find(h, key) else { return default };
    while i < h.len() && (h[i] == b' ' || h[i] == b'\n') {
        i += 1;
    }
    let neg = i < h.len() && h[i] == b'-';
    if neg {
        i += 1;
    }
    let mut v = 0.0f32;
    let mut any = false;
    while i < h.len() && h[i].is_ascii_digit() {
        v = v * 10.0 + (h[i] - b'0') as f32;
        i += 1;
        any = true;
    }
    if i < h.len() && h[i] == b'.' {
        i += 1;
        let mut scale = 0.1f32;
        while i < h.len() && h[i].is_ascii_digit() {
            v += (h[i] - b'0') as f32 * scale;
            scale *= 0.1;
            i += 1;
            any = true;
        }
    }
    if !any {
        return default;
    }
    if i < h.len() && (h[i] == b'e' || h[i] == b'E') {
        i += 1;
        let eneg = i < h.len() && h[i] == b'-';
        if i < h.len() && (h[i] == b'-' || h[i] == b'+') {
            i += 1;
        }
        let mut e = 0i32;
        while i < h.len() && h[i].is_ascii_digit() {
            e = e * 10 + (h[i] - b'0') as i32;
            i += 1;
        }
        let mut m = 1.0f32;
        for _ in 0..e.min(30) {
            m *= 10.0;
        }
        v = if eneg { v / m } else { v * m };
    }
    if neg { -v } else { v }
}

// ---- math helpers (core has no float maths) ----

fn floor(v: f32) -> f32 {
    let i = v as i32;
    let f = i as f32;
    if f > v { f - 1.0 } else { f }
}

fn round(v: f32) -> f32 {
    floor(v + 0.5)
}

fn sqrt(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits((v.to_bits() >> 1) + 0x1fbd_1df5);
    y = 0.5 * (y + v / y);
    y = 0.5 * (y + v / y);
    0.5 * (y + v / y)
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---- exp / ln / pow for the grading tables (core has none) ----

const LN2: f32 = core::f32::consts::LN_2;

fn ln(x: f32) -> f32 {
    let bits = x.to_bits();
    let mut e = ((bits >> 23) & 0xff) as i32 - 127;
    let mut m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000); // 1..2
    if m > 1.414_213_6 {
        m *= 0.5;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    2.0 * s * (1.0 + s2 * (1.0 / 3.0 + s2 * (1.0 / 5.0 + s2 * (1.0 / 7.0 + s2 * (1.0 / 9.0))))) + e as f32 * LN2
}

fn exp(x: f32) -> f32 {
    let x = x.clamp(-80.0, 80.0);
    let k = floor(x / LN2 + 0.5);
    let r = x - k * LN2;
    let mut t = 1.0f32;
    let mut term = 1.0f32;
    for n in 1..9 {
        term *= r / n as f32;
        t += term;
    }
    t * f32::from_bits(((k as i32 + 127) as u32) << 23)
}

fn pow(a: f32, b: f32) -> f32 {
    if a <= 0.0 { 0.0 } else { exp(b * ln(a)) }
}

// ---- JSON string value reader (for the `look` choice) ----

/// The bytes of the string value after `"key":`, without the quotes.
fn text<'a>(h: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let mut i = find(h, key)?;
    while i < h.len() && (h[i] == b' ' || h[i] == b'\n') {
        i += 1;
    }
    if i >= h.len() || h[i] != b'"' {
        return None;
    }
    let start = i + 1;
    let mut j = start;
    while j < h.len() && h[j] != b'"' {
        j += 1;
    }
    Some(&h[start..j])
}

/// Table read with interpolation between two entries, on a 0..255 input.
fn lut(t: &[f32], v: f32) -> f32 {
    if v <= 0.0 {
        return t[0];
    }
    if v >= 255.0 {
        return t[255];
    }
    let i = v as usize;
    t[i] + (t[i + 1] - t[i]) * (v - i as f32)
}

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

/// Where the canvas edges fall in a plane (buffer indices): reads past them are clamped to them on every
/// pass, exactly as HyperImage clamps at the image edge (the host's own padding is a one-off copy).
#[derive(Clone, Copy)]
struct Bounds {
    xlo: i32,
    xhi: i32,
    ylo: i32,
    yhi: i32,
}

/// One pass along rows (`vertical` false) or columns of the `w` x `h` plane `p`; `line` holds one row or column.
fn box_pass(p: &mut [f32], line: &mut [f32], w: usize, h: usize, r: i32, vertical: bool, b: Bounds) {
    let (len, count) = if vertical { (h, w) } else { (w, h) };
    let (lo, hi) = if vertical { (b.ylo, b.yhi) } else { (b.xlo, b.xhi) };
    let inv = 1.0 / (2 * r + 1) as f64;
    for k in 0..count {
        for i in 0..len {
            line[i] = if vertical { p[i * w + k] } else { p[k * w + i] };
        }
        let mut s = 0.0f64;
        for i in -r..=r {
            s += line[i.clamp(lo, hi) as usize] as f64;
        }
        for i in 0..len {
            let v = (s * inv) as f32;
            if vertical {
                p[i * w + k] = v;
            } else {
                p[k * w + i] = v;
            }
            let add = (i as i32 + r + 1).clamp(lo, hi) as usize;
            let sub = (i as i32 - r).clamp(lo, hi) as usize;
            s += line[add] as f64 - line[sub] as f64;
        }
    }
}


/// Per-channel tables: levels, gamma, exposure, contrast, shadows/highlights, then the temperature / tint
/// split (HyperImage buildLUTs without the tone curve). `cast` false leaves temperature and tint out (gray).
fn build_luts(l: &Look, cast: bool, r: &mut [f32], g: &mut [f32], b: &mut [f32]) {
    let black = l.black;
    let white = if l.white > l.black + 1.0 { l.white } else { l.black + 1.0 };
    let inv_gamma = 1.0 / (l.gamma / 100.0);
    let exp_f = pow(2.0, l.exposure / 100.0);
    let con_f = 1.0 + l.contrast / 100.0;
    let sh = l.shadows / 100.0;
    let hi = l.highlights / 100.0;
    let temp = if cast { l.temp / 100.0 * 32.0 } else { 0.0 };
    let tint = if cast { l.tint / 100.0 * 32.0 } else { 0.0 };
    for i in 0..256 {
        let mut v = ((i as f32 - black) / (white - black)).clamp(0.0, 1.0);
        v = pow(v, inv_gamma) * 255.0;
        v *= exp_f;
        v = (v - 128.0) * con_f + 128.0;
        let n = v / 255.0;
        v += sh * (1.0 - n) * 64.0;
        v += hi * n * 64.0;
        r[i] = (v + temp).clamp(0.0, 255.0);
        g[i] = (v + tint).clamp(0.0, 255.0);
        b[i] = (v - temp).clamp(0.0, 255.0);
    }
}

/// Vertical box pass, row by row (cache friendly): running column sums `sums` (one per column), the result goes
/// to `aux` and is copied back. Edges clamped to rows `lo..=hi`.
fn box_v(p: &mut [f32], aux: &mut [f32], sums: &mut [f32], w: usize, h: usize, r: i32, lo: i32, hi: i32) {
    let inv = 1.0 / (2 * r + 1) as f32;
    for x in 0..w {
        sums[x] = 0.0;
    }
    for i in -r..=r {
        let row = i.clamp(lo, hi) as usize * w;
        for x in 0..w {
            sums[x] += p[row + x];
        }
    }
    for y in 0..h {
        let o = y * w;
        for x in 0..w {
            aux[o + x] = sums[x] * inv;
        }
        let add = (y as i32 + r + 1).clamp(lo, hi) as usize * w;
        let sub = (y as i32 - r).clamp(lo, hi) as usize * w;
        for x in 0..w {
            sums[x] += p[add + x] - p[sub + x];
        }
    }
    p[..w * h].copy_from_slice(&aux[..w * h]);
}

/// One box blur (a single pass, as HyperImage's `boxBlurChannel`): rows then columns, radius `r`, edges clamped.
/// `line` holds one row (at least `w` and `h` floats), `aux` a plane the size of `p`.
fn box1(p: &mut [f32], line: &mut [f32], aux: &mut [f32], w: usize, h: usize, r: i32, b: Bounds) {
    if r < 1 {
        return;
    }
    box_pass(p, line, w, h, r, false, b);
    box_v(p, aux, line, w, h, r, b.ylo, b.yhi);
}

/// Radii up to this are blurred at full resolution (identical to HyperImage); the wide local-contrast blur of a big
/// photo is done at reduced resolution (average s x s blocks, box blur the small plane, spread back with bilinear
/// interpolation). A blur that wide is smooth, so the difference is small, and it is what makes 24 MP photos finish.
const EXACT_BOX_RADIUS: i32 = 40;

fn box_fast(p: &mut [f32], line: &mut [f32], aux: &mut [f32], low: &mut [f32], w: usize, h: usize, r: i32, b: Bounds) {
    if r <= EXACT_BOX_RADIUS {
        return box1(p, line, aux, w, h, r, b);
    }
    let s = ((r / 8) as usize).clamp(2, 16);
    let (lw, lh) = (w.div_ceil(s), h.div_ceil(s));
    for ly in 0..lh {
        let (y0, y1) = (ly * s, ((ly + 1) * s).min(h));
        for lx in 0..lw {
            let (x0, x1) = (lx * s, ((lx + 1) * s).min(w));
            let mut sum = 0.0f32;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += p[y * w + x];
                }
            }
            low[ly * lw + lx] = sum / ((y1 - y0) * (x1 - x0)) as f32;
        }
    }
    let sb = Bounds { xlo: b.xlo / s as i32, xhi: b.xhi / s as i32, ylo: b.ylo / s as i32, yhi: b.yhi / s as i32 };
    let lr = (round(r as f32 / s as f32) as i32).max(1);
    box1(&mut low[..lw * lh], line, aux, lw, lh, lr, sb);
    let (wm, hm) = (lw as i32 - 1, lh as i32 - 1);
    let inv = 1.0 / s as f32;
    for y in 0..h {
        let fy = (y as f32 + 0.5) * inv - 0.5;
        let y0f = floor(fy);
        let ty = fy - y0f;
        let ya = (y0f as i32).clamp(0, hm) as usize;
        let yb = (y0f as i32 + 1).clamp(0, hm) as usize;
        for x in 0..w {
            let fx = (x as f32 + 0.5) * inv - 0.5;
            let x0f = floor(fx);
            let tx = fx - x0f;
            let xa = (x0f as i32).clamp(0, wm) as usize;
            let xb = (x0f as i32 + 1).clamp(0, wm) as usize;
            let (p00, p10) = (low[ya * lw + xa], low[ya * lw + xb]);
            let (p01, p11) = (low[yb * lw + xa], low[yb * lw + xb]);
            p[y * w + x] = p00 + (p10 - p00) * tx + (p01 - p00) * ty + (p00 - p10 - p01 + p11) * tx * ty;
        }
    }
}

/// Soft-light of `a` by `b` (0..1), W3C / Photoshop formulation (HyperImage softLight).
fn soft_light(a: f32, b: f32) -> f32 {
    let g = if a <= 0.25 { ((16.0 * a - 12.0) * a + 4.0) * a } else { sqrt(a) };
    if b <= 0.5 { a - (1.0 - 2.0 * b) * a * (1.0 - a) } else { a + (2.0 * b - 1.0) * (g - a) }
}

/// # Safety
/// The host guarantees `buf..buf + len` is a block it got from `pc_alloc`, holding `len / 4` samples,
/// and `params..params + params_len` is readable UTF-8 JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_filter(buf: *mut f32, len: u32, width: u32, height: u32, channels: u32, format: u32, params: *const u8, params_len: u32) -> i32 {
    let n = channels as usize;
    let mode = (format >> 16) & 0xff;
    // RGB (3), grayscale (1) and duotone (8, stored as gray). CMYK, Lab, multichannel: not supported.
    let rgb = match mode {
        3 => true,
        1 | 8 => false,
        _ => return 2,
    };
    let alpha = format & HAS_ALPHA != 0;
    let colour = if alpha { n.wrapping_sub(1) } else { n };
    if n == 0 || colour != if rgb { 3 } else { 1 } || (width as usize) * (height as usize) * n * 4 > len as usize {
        return 1;
    }
    let (w, h) = (width as usize, height as usize);
    // SAFETY: see the function's contract.
    let px = unsafe { core::slice::from_raw_parts_mut(buf, w * h * n) };
    let p = unsafe { core::slice::from_raw_parts(params, params_len as usize) };

    // The look, then the sliders as adjustments added to it.
    let look = match text(p, "look") {
        Some(name) => LOOKS.iter().find(|l| l.name.as_bytes() == name).unwrap_or(&LOOKS[0]),
        None => &LOOKS[0],
    };
    let adj = |key: &str, base: f32, lo: f32, hi: f32| (base + num(p, key, 0.0)).clamp(lo, hi);
    let strength = num(p, "strength", 100.0).clamp(0.0, 100.0) / 100.0;
    let mut t = *look;
    t.exposure = adj("exposure", look.exposure, -100.0, 100.0);
    t.contrast = adj("contrast", look.contrast, -100.0, 100.0);
    t.highlights = adj("highlights", look.highlights, -100.0, 100.0);
    t.shadows = adj("shadows", look.shadows, -100.0, 100.0);
    t.black = adj("blackPoint", look.black, 0.0, 250.0);
    t.white = adj("whitePoint", look.white, 5.0, 255.0);
    t.vibrance = adj("vibrance", look.vibrance, -100.0, 100.0);
    t.saturation = adj("saturation", look.saturation, -100.0, 100.0);
    t.temp = adj("temperature", look.temp, -100.0, 100.0);
    t.tint = adj("tint", look.tint, -100.0, 100.0);
    t.st_amount = adj("splitTone", look.st_amount, 0.0, 100.0);
    let depth = adj("localContrast", look.depth, 0.0, 100.0) / 100.0;
    let sharp = adj("sharpness", look.sharp, 0.0, 100.0) / 100.0 * 1.5;
    let texture = adj("texture", look.texture, -100.0, 100.0) / 100.0;

    // The band's place in the document (the buffer's top-left sample, margins included) and the canvas.
    let ox = num(p, "x", 0.0);
    let oy = num(p, "y", 0.0);
    let cw = num(p, "canvasWidth", width as f32).max(1.0);
    let ch = num(p, "canvasHeight", height as f32).max(1.0);
    let nc = colour;

    // Radii (HyperImage: Depth 2% of the long side, Texture 1.5 and 6 per thousand), scaled back together when
    // their reaches (depth + 1 for sharpness + texture) would not fit the overlap.
    let long = cw.max(ch);
    let base = long / 1000.0;
    let mut rd = (round(long * 0.02) as i32).max(2);
    let r1 = (round(1.5 * base) as i32).max(1);
    let mut r2 = (round(6.0 * base) as i32).max(r1 + 1);
    let use_depth = depth > 0.0;
    let use_sharp = sharp > 0.0;
    let use_tex = texture != 0.0;
    let mut reach = (if use_depth { rd } else { 0 } + if use_sharp { 1 } else { 0 } + if use_tex { r2 } else { 0 }) as f32;
    let budget = OVERLAP - 6.0;
    let mut r1s = r1;
    if reach > budget {
        let k = budget / reach;
        rd = ((rd as f32 * k) as i32).max(2);
        r2 = ((r2 as f32 * k) as i32).max(2);
        r1s = ((r1 as f32 * k) as i32).max(1).min(r2 - 1).max(1);
        reach = budget;
    }
    let r1 = r1s;

    // Work only on the part the host keeps plus what the spatial steps reach.
    let ov = (num(p, "overlap", 0.0).max(0.0) as usize).min(w / 2).min(h / 2);
    let (cx0, cy0, cx1, cy1) = (ov, ov, w - ov, h - ov);
    let margin = reach as usize + 8;
    let rx0 = cx0.saturating_sub(margin);
    let ry0 = cy0.saturating_sub(margin);
    let rx1 = (cx1 + margin).min(w);
    let ry1 = (cy1 + margin).min(h);
    let (rw, rh) = (rx1 - rx0, ry1 - ry0);
    let nr = rw * rh;
    let (rox, roy) = (ox + rx0 as f32, oy + ry0 as f32);
    // Canvas edges inside the working area: blurs clamp there, as HyperImage's do at the image edge.
    let bounds = Bounds {
        xlo: (-(rox as i32)).clamp(0, rw as i32 - 1),
        xhi: (cw as i32 - 1 - rox as i32).clamp(0, rw as i32 - 1),
        ylo: (-(roy as i32)).clamp(0, rh as i32 - 1),
        yhi: (ch as i32 - 1 - roy as i32).clamp(0, rh as i32 - 1),
    };

    // Grading tables, then the grade into planar working planes (0..1, clamped).
    let Some(tables) = plane(768) else { return 3 };
    let (lr, rest) = tables.split_at_mut(256);
    let (lg, lb) = rest.split_at_mut(256);
    build_luts(&t, rgb, lr, lg, lb);
    let Some(line) = plane(rw.max(rh)) else { return 3 };
    let Some(tmp) = plane(nr) else { return 3 };
    let Some(aux) = plane(nr) else { return 3 };
    let mut g: [&mut [f32]; 3] = [&mut [], &mut [], &mut []];
    for c in 0..nc {
        match plane(nr) { Some(a) => g[c] = a, None => return 3 }
    }
    let vib = t.vibrance / 100.0;
    let sat = 1.0 + t.saturation / 100.0;
    let st_amt = t.st_amount / 100.0;
    let sh_l = (luma(t.st_shadow[0], t.st_shadow[1], t.st_shadow[2]) / 255.0).max(0.02);
    let hi_l = (luma(t.st_highlight[0], t.st_highlight[1], t.st_highlight[2]) / 255.0).max(0.02);
    let pivot = (0.5 + t.st_balance / 200.0).clamp(0.05, 0.95);
    for yy in 0..rh {
        for xx in 0..rw {
            let i = ((ry0 + yy) * w + rx0 + xx) * n;
            let o = yy * rw + xx;
            if rgb {
                let (mut r, mut gg, mut b) = (lut(lr, px[i] * 255.0), lut(lg, px[i + 1] * 255.0), lut(lb, px[i + 2] * 255.0));
                if vib != 0.0 {
                    let mx = r.max(gg).max(b);
                    let mn = r.min(gg).min(b);
                    let mut a = vib * (1.0 - (mx - mn) / 255.0);
                    if r > gg && gg > b {
                        a *= 0.5;
                    }
                    let l = luma(r, gg, b);
                    r = l + (r - l) * (1.0 + a);
                    gg = l + (gg - l) * (1.0 + a);
                    b = l + (b - l) * (1.0 + a);
                }
                if sat != 1.0 {
                    let l = luma(r, gg, b);
                    r = l + (r - l) * sat;
                    gg = l + (gg - l) * sat;
                    b = l + (b - l) * sat;
                }
                if st_amt > 0.0 {
                    let l = luma(r, gg, b) / 255.0;
                    let wt = if l < pivot { (pivot - l) / pivot } else { 0.0 };
                    if wt > 0.0 {
                        let k = wt * wt * (3.0 - 2.0 * wt) * st_amt;
                        let s = l / sh_l;
                        r += (t.st_shadow[0] * s - r) * k;
                        gg += (t.st_shadow[1] * s - gg) * k;
                        b += (t.st_shadow[2] * s - b) * k;
                    }
                    let wt = if l > pivot { (l - pivot) / (1.0 - pivot) } else { 0.0 };
                    if wt > 0.0 {
                        let k = wt * wt * (3.0 - 2.0 * wt) * st_amt;
                        let s = l / hi_l;
                        r += (t.st_highlight[0] * s - r) * k;
                        gg += (t.st_highlight[1] * s - gg) * k;
                        b += (t.st_highlight[2] * s - b) * k;
                    }
                }
                g[0][o] = (r / 255.0).clamp(0.0, 1.0);
                g[1][o] = (gg / 255.0).clamp(0.0, 1.0);
                g[2][o] = (b / 255.0).clamp(0.0, 1.0);
            } else {
                g[0][o] = (lut(lg, px[i] * 255.0) / 255.0).clamp(0.0, 1.0);
            }
        }
    }

    // Local contrast ("Depth"): soft-light against a wide blur, mixed by the amount.
    if use_depth {
        let Some(low) = plane(if rd > EXACT_BOX_RADIUS { nr / 4 + rw + rh + 64 } else { 1 }) else { return 3 };
        for c in 0..nc {
            tmp.copy_from_slice(g[c]);
            box_fast(tmp, line, aux, low, rw, rh, rd, bounds);
            for i in 0..nr {
                let a = g[c][i];
                g[c][i] = (a * (1.0 - depth) + soft_light(a, tmp[i]) * depth).clamp(0.0, 1.0);
            }
        }
    }
    // Sharpness: unsharp mask, radius 1.
    if use_sharp {
        for c in 0..nc {
            tmp.copy_from_slice(g[c]);
            box1(tmp, line, aux, rw, rh, 1, bounds);
            for i in 0..nr {
                g[c][i] = (g[c][i] + sharp * (g[c][i] - tmp[i])).clamp(0.0, 1.0);
            }
        }
    }
    // Texture: a band-pass on luminance (fine blur minus coarse blur) added to every channel.
    if use_tex {
        let (Some(lum), Some(coarse)) = (plane(nr), plane(nr)) else { return 3 };
        for i in 0..nr {
            lum[i] = if rgb { luma(g[0][i], g[1][i], g[2][i]) } else { g[0][i] };
        }
        coarse.copy_from_slice(lum);
        box1(lum, line, aux, rw, rh, r1, bounds);
        box1(coarse, line, aux, rw, rh, r2, bounds);
        let k = if texture > 0.0 { texture * 1.6 } else { texture };
        for i in 0..nr {
            let band = (lum[i] - coarse[i]) * k;
            for c in 0..nc {
                g[c][i] = (g[c][i] + band).clamp(0.0, 1.0);
            }
        }
    }

    // Write back the part the host keeps, mixed with the original by strength.
    for y in cy0..cy1 {
        for x in cx0..cx1 {
            let i = (y * w + x) * n;
            let j = (y - ry0) * rw + (x - rx0);
            for c in 0..nc {
                let o = px[i + c];
                px[i + c] = o + (g[c][j] - o) * strength;
            }
        }
    }
    0
}
