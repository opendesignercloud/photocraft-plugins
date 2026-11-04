// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
//! ODC Film Look: a PhotoCraft filter plug-in (plug-in ABI v1, see PhotoCraft `docs/plugins.md`).
//!
//! A **look** (a preset from HyperImage's Adjust Color: levels, gamma, exposure, contrast,
//! highlights, shadows, vibrance, saturation, temperature, tint, sepia, a tone curve and a split tone)
//! followed by four finishing steps: faded blacks, warmth, vignette, film grain. The sliders are
//! adjustments ADDED to the look, so 0 always means "as the look is".
//! `no_std`, no imports. Grain and vignette are computed from the document position of every pixel
//! (the host passes the band's `x`, `y` and the canvas size in `_image`), so the result is identical
//! whichever way the host cuts the layer into bands, and identical on every run for the same seed.
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

const MANIFEST: &str = r#"{
  "id": "org.opendesignercloud.film-look",
  "name": "ODC Film Look…",
  "version": "2.0.0",
  "kind": "filter",
  "author": "Open Designer Cloud",
  "description": "Film looks (Kodachrome, Tri-X, Fuji Superia, bleach bypass and more) with grain, vignette, faded blacks and warmth. The sliders adjust the chosen look. Open source (Apache 2.0): opendesignercloud.com",
  "params": {
    "look":            {"type": "choice", "options": ["Classic Film", "Custom", "Vintage Western", "Fuji Superia 1600", "Kodak Kodachrome 2", "Tri-X", "WhiteWedding", "Kodak T-Max 400", "Tri-X B&W", "Matte Fade", "High-Contrast B&W", "Bleach Bypass", "Golden Hour"], "default": "Classic Film"},
    "strength":        {"type": "number", "min": 0,    "max": 100, "default": 100},
    "grain":           {"type": "number", "min": -100, "max": 100, "default": 0},
    "grainSize":       {"type": "number", "min": -7,   "max": 7,   "default": 0},
    "grainColor":      {"type": "number", "min": -100, "max": 100, "default": 0},
    "vignette":        {"type": "number", "min": -100, "max": 100, "default": 0},
    "vignetteSize":    {"type": "number", "min": -100, "max": 100, "default": 0},
    "vignetteFeather": {"type": "number", "min": -100, "max": 100, "default": 0},
    "vignetteRound":   {"type": "number", "min": -100, "max": 100, "default": 0},
    "fade":            {"type": "number", "min": -100, "max": 100, "default": 0},
    "warmth":          {"type": "number", "min": -100, "max": 100, "default": 0},
    "seed":            {"type": "int",    "min": 0,    "max": 9999, "default": 0}
  },
  "overlap": 0,
  "area": "content"
}"#;

// ---- the looks ----

/// Everything a look sets. Tone and colour fields are in HyperImage's units (0 to 255 levels,
/// sliders -100..100); grain, vignette, fade and warmth are this plug-in's own finishing steps.
struct Look {
    name: &'static str,
    tone: bool, // false: skip the grading pass entirely (Classic Film, Custom)
    black: f32, gamma: f32, white: f32, exposure: f32, contrast: f32, highlights: f32, shadows: f32,
    vibrance: f32, saturation: f32, temp: f32, tint: f32, sepia: f32,
    curve: &'static [(f32, f32)],
    st_shadow: [f32; 3], st_highlight: [f32; 3], st_balance: f32, st_amount: f32,
    grain: f32, grain_size: f32, grain_color: f32,
    vignette: f32, v_size: f32, v_feather: f32, v_round: f32,
    fade: f32, warmth: f32,
}

const NEUTRAL: Look = Look {
    name: "Custom", tone: false,
    black: 0.0, gamma: 100.0, white: 255.0, exposure: 0.0, contrast: 0.0, highlights: 0.0, shadows: 0.0,
    vibrance: 0.0, saturation: 0.0, temp: 0.0, tint: 0.0, sepia: 0.0,
    curve: &[],
    st_shadow: [58.0, 110.0, 165.0], st_highlight: [232.0, 184.0, 120.0], st_balance: 0.0, st_amount: 0.0,
    grain: 0.0, grain_size: 1.5, grain_color: 20.0,
    vignette: 0.0, v_size: 50.0, v_feather: 60.0, v_round: 50.0,
    fade: 0.0, warmth: 0.0,
};

/// In the same order as the manifest's options.
const LOOKS: [Look; 13] = [
    Look { name: "Classic Film", grain: 25.0, vignette: 30.0, fade: 8.0, ..NEUTRAL },
    NEUTRAL,
    // Fully desaturated then a warm sepia, low-key, held highlights, fine grain, strong vignette.
    // HyperImage also adds Depth, Sharpness and Texture here: not ported (they need neighbouring pixels).
    Look { name: "Vintage Western", tone: true, black: 16.0, gamma: 88.0, exposure: -10.0, contrast: 18.0, highlights: -25.0, shadows: 8.0,
        saturation: -100.0, sepia: 75.0, grain: 34.0, grain_size: 1.0, grain_color: 0.0, vignette: 50.0, v_size: 67.0, v_feather: 61.0, v_round: 0.0, ..NEUTRAL },
    // A Nikon Z "Pure" recipe plus the FUJI-SUPERIA Picture Control curve (Texture -50 not ported).
    Look { name: "Fuji Superia 1600", tone: true, contrast: 5.0, highlights: -5.0, shadows: 20.0, saturation: -8.0, temp: 32.0, tint: 3.0,
        curve: &[(0.0, 0.0), (26.0, 31.0), (58.0, 65.0), (75.0, 86.0), (138.0, 148.0), (182.0, 188.0), (201.0, 203.0), (212.0, 213.0), (243.0, 237.0), (255.0, 247.0)], ..NEUTRAL },
    Look { name: "Kodak Kodachrome 2", tone: true, saturation: -10.0,
        curve: &[(0.0, 0.0), (33.0, 24.0), (64.0, 64.0), (92.0, 104.0), (128.0, 148.0), (166.0, 166.0), (207.0, 192.0), (229.0, 217.0), (255.0, 255.0)], ..NEUTRAL },
    Look { name: "Tri-X", tone: true,
        curve: &[(0.0, 0.0), (63.0, 88.0), (123.0, 161.0), (168.0, 211.0), (201.0, 240.0), (226.0, 251.0), (251.0, 255.0)], ..NEUTRAL },
    Look { name: "WhiteWedding", tone: true,
        curve: &[(0.0, 0.0), (45.0, 64.0), (90.0, 128.0), (128.0, 172.0), (191.0, 209.0), (225.0, 225.0), (244.0, 238.0), (252.0, 249.0), (255.0, 255.0)], ..NEUTRAL },
    // Monochrome with a yellow filter (Temp before the desaturation); Sharpness 24 not ported.
    Look { name: "Kodak T-Max 400", tone: true, gamma: 105.0, saturation: -100.0, temp: 25.0,
        curve: &[(0.0, 0.0), (26.0, 28.0), (77.0, 66.0), (150.0, 146.0), (226.0, 243.0), (255.0, 255.0)], ..NEUTRAL },
    Look { name: "Tri-X B&W", tone: true, saturation: -100.0,
        curve: &[(0.0, 0.0), (63.0, 88.0), (123.0, 161.0), (168.0, 211.0), (201.0, 240.0), (226.0, 251.0), (251.0, 255.0)], ..NEUTRAL },
    // Blacks lifted by the curve (flat across the bottom, which reads as matte) and cool shadows.
    Look { name: "Matte Fade", tone: true, contrast: -8.0, saturation: -8.0,
        curve: &[(0.0, 20.0), (64.0, 80.0), (128.0, 140.0), (192.0, 200.0), (255.0, 236.0)],
        st_shadow: [74.0, 107.0, 138.0], st_highlight: [239.0, 220.0, 192.0], st_balance: 0.0, st_amount: 12.0, ..NEUTRAL },
    Look { name: "High-Contrast B&W", tone: true, black: 12.0, contrast: 30.0, saturation: -100.0, ..NEUTRAL },
    Look { name: "Bleach Bypass", tone: true, black: 10.0, contrast: 28.0, saturation: -55.0, ..NEUTRAL },
    Look { name: "Golden Hour", tone: true, temp: 25.0, tint: 3.0, vibrance: 15.0, shadows: 10.0, highlights: -8.0, ..NEUTRAL },
];

const HAS_ALPHA: u32 = 1 << 8;

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

/// Integer hash (lowbias32) of a lattice point, a channel and the seed, mapped to -1..1.
fn hash(x: i32, y: i32, ch: u32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca6b) ^ ch.wrapping_mul(0xc2b2_ae35) ^ seed.wrapping_mul(0x27d4_eb2f);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
}

/// Grain sample at document position (px, py): smooth value noise on a lattice `size` pixels apart.
fn grain(px: f32, py: f32, size: f32, ch: u32, seed: u32) -> f32 {
    if size <= 1.01 {
        let (x, y) = (px as i32, py as i32);
        // triangular distribution is closer to real grain than flat noise
        return (hash(x, y, ch, seed) + hash(x, y, ch + 17, seed)) * 0.7;
    }
    let gx = px / size;
    let gy = py / size;
    let fx = floor(gx);
    let fy = floor(gy);
    let tx = smooth(0.0, 1.0, gx - fx);
    let ty = smooth(0.0, 1.0, gy - fy);
    let (ix, iy) = (fx as i32, fy as i32);
    let a = hash(ix, iy, ch, seed);
    let b = hash(ix + 1, iy, ch, seed);
    let c = hash(ix, iy + 1, ch, seed);
    let d = hash(ix + 1, iy + 1, ch, seed);
    let top = a + (b - a) * tx;
    let bot = c + (d - c) * tx;
    (top + (bot - top) * ty) * 1.7 // interpolation lowers the spread; bring it back up
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

// ---- tone curve and grading tables (a port of HyperImage's buildCurveLUT / buildLUTs) ----

/// Monotone cubic through the control points (Fritsch-Carlson), sampled at 0..255.
fn curve_lut(pts: &[(f32, f32)], out: &mut [f32]) {
    let n = pts.len().min(12);
    let mut dxs = [0.0f32; 12];
    let mut ms = [0.0f32; 12];
    let mut c1 = [0.0f32; 12];
    for i in 0..n - 1 {
        dxs[i] = pts[i + 1].0 - pts[i].0;
        ms[i] = if dxs[i] != 0.0 { (pts[i + 1].1 - pts[i].1) / dxs[i] } else { 0.0 };
    }
    c1[0] = ms[0];
    c1[n - 1] = ms[n - 2];
    for i in 1..n - 1 {
        if ms[i - 1] * ms[i] <= 0.0 {
            c1[i] = 0.0;
        } else {
            let w1 = 2.0 * dxs[i] + dxs[i - 1];
            let w2 = dxs[i] + 2.0 * dxs[i - 1];
            c1[i] = (w1 + w2) / (w1 / ms[i - 1] + w2 / ms[i]);
        }
    }
    for (x, o) in out.iter_mut().enumerate().take(256) {
        let xf = x as f32;
        if xf <= pts[0].0 {
            *o = pts[0].1;
            continue;
        }
        if xf >= pts[n - 1].0 {
            *o = pts[n - 1].1;
            continue;
        }
        let mut i = 0;
        while i < n - 1 && xf > pts[i + 1].0 {
            i += 1;
        }
        let h = dxs[i];
        let t = (xf - pts[i].0) / h;
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        *o = h00 * pts[i].1 + h10 * h * c1[i] + h01 * pts[i + 1].1 + h11 * h * c1[i + 1];
    }
}

/// Per-channel tables: levels, gamma, exposure, contrast, shadows/highlights, the curve, then the
/// temperature / tint split. `cast` false leaves temperature and tint out (grayscale documents).
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
    let mut cl = [0.0f32; 256];
    if l.curve.len() >= 2 {
        curve_lut(l.curve, &mut cl);
    }
    for i in 0..256 {
        let mut v = ((i as f32 - black) / (white - black)).clamp(0.0, 1.0);
        v = pow(v, inv_gamma) * 255.0;
        v *= exp_f;
        v = (v - 128.0) * con_f + 128.0;
        let n = v / 255.0;
        v += sh * (1.0 - n) * 64.0;
        v += hi * n * 64.0;
        if l.curve.len() >= 2 {
            v = cl[floor(v.clamp(0.0, 255.0) + 0.5) as usize];
        }
        r[i] = (v + temp).clamp(0.0, 255.0);
        g[i] = (v + tint).clamp(0.0, 255.0);
        b[i] = (v - temp).clamp(0.0, 255.0);
    }
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
    // SAFETY: see the function's contract.
    let px = unsafe { core::slice::from_raw_parts_mut(buf, width as usize * height as usize * n) };
    let p = unsafe { core::slice::from_raw_parts(params, params_len as usize) };

    // The look, then the sliders as adjustments added to it.
    let look = match text(p, "look") {
        Some(name) => LOOKS.iter().find(|l| l.name.as_bytes() == name).unwrap_or(&LOOKS[0]),
        None => &LOOKS[0],
    };
    let adj = |key: &str, base: f32, lo: f32, hi: f32| (base + num(p, key, 0.0)).clamp(lo, hi);
    let strength = num(p, "strength", 100.0).clamp(0.0, 100.0) / 100.0;
    let amount = adj("grain", look.grain, 0.0, 100.0) / 100.0 * 0.12;
    let size = adj("grainSize", look.grain_size, 1.0, 8.0);
    let chroma = adj("grainColor", look.grain_color, 0.0, 100.0) / 100.0;
    let vig = adj("vignette", look.vignette, 0.0, 100.0) / 100.0;
    let vstart = 1.0 - adj("vignetteSize", look.v_size, 0.0, 100.0) / 100.0;
    let vwidth = 0.15 + adj("vignetteFeather", look.v_feather, 0.0, 100.0) / 100.0 * 0.85;
    let vround = adj("vignetteRound", look.v_round, 0.0, 100.0) / 100.0;
    let fade = adj("fade", look.fade, 0.0, 100.0) / 100.0 * 0.25;
    let warm = adj("warmth", look.warmth, -100.0, 100.0) / 100.0 * 0.06;
    let seed = num(p, "seed", 0.0).clamp(0.0, 9999.0) as u32;
    // The band's place in the document (the buffer's top-left sample) and the canvas size.
    let ox = num(p, "x", 0.0);
    let oy = num(p, "y", 0.0);
    let cw = num(p, "canvasWidth", width as f32).max(1.0);
    let ch = num(p, "canvasHeight", height as f32).max(1.0);
    let (cx, cy) = (cw * 0.5, ch * 0.5);
    let half_diag = sqrt(cx * cx + cy * cy).max(1.0);
    let use_vig = vig > 0.0;
    let use_grain = amount > 0.0;

    // Grading tables for the look (3 x 256 floats from fresh memory pages).
    let mut tables: &mut [f32] = &mut [];
    if look.tone {
        let at = pc_alloc(3 * 256 * 4);
        if at == 0 {
            return 3;
        }
        // SAFETY: `pc_alloc` returned a block of 3 KiB inside our memory.
        tables = unsafe { core::slice::from_raw_parts_mut(at as usize as *mut f32, 768) };
        let (lr, rest) = tables.split_at_mut(256);
        let (lg, lb) = rest.split_at_mut(256);
        build_luts(look, rgb, lr, lg, lb);
    }
    let vib = look.vibrance / 100.0;
    let sat = 1.0 + look.saturation / 100.0;
    let sep = look.sepia / 100.0;
    let st_amt = look.st_amount / 100.0;
    let sh_l = (luma(look.st_shadow[0], look.st_shadow[1], look.st_shadow[2]) / 255.0).max(0.02);
    let hi_l = (luma(look.st_highlight[0], look.st_highlight[1], look.st_highlight[2]) / 255.0).max(0.02);
    let pivot = (0.5 + look.st_balance / 200.0).clamp(0.05, 0.95);

    for yy in 0..height as usize {
        let dy = oy + yy as f32 - cy;
        for xx in 0..width as usize {
            let i = (yy * width as usize + xx) * n;
            if alpha && px[i + n - 1] <= 0.0 {
                continue; // fully transparent: nothing to grade
            }
            let orig = if rgb { [px[i], px[i + 1], px[i + 2]] } else { [px[i], 0.0, 0.0] };
            let mut c = orig;

            // 1. the look: tables, vibrance, saturation, sepia, split tone (HyperImage's order)
            if look.tone {
                if rgb {
                    let (mut r, mut g, mut b) = (lut(&tables[..256], c[0] * 255.0), lut(&tables[256..512], c[1] * 255.0), lut(&tables[512..], c[2] * 255.0));
                    if vib != 0.0 {
                        let mx = r.max(g).max(b);
                        let mn = r.min(g).min(b);
                        let mut a = vib * (1.0 - (mx - mn) / 255.0);
                        if r > g && g > b {
                            a *= 0.5;
                        }
                        let l = luma(r, g, b);
                        r = l + (r - l) * (1.0 + a);
                        g = l + (g - l) * (1.0 + a);
                        b = l + (b - l) * (1.0 + a);
                    }
                    if sat != 1.0 {
                        let l = luma(r, g, b);
                        r = l + (r - l) * sat;
                        g = l + (g - l) * sat;
                        b = l + (b - l) * sat;
                    }
                    if sep > 0.0 {
                        let sr = r * 0.393 + g * 0.769 + b * 0.189;
                        let sg = r * 0.349 + g * 0.686 + b * 0.168;
                        let sb = r * 0.272 + g * 0.534 + b * 0.131;
                        r += (sr - r) * sep;
                        g += (sg - g) * sep;
                        b += (sb - b) * sep;
                    }
                    if st_amt > 0.0 {
                        let l = luma(r, g, b) / 255.0;
                        let w = if l < pivot { (pivot - l) / pivot } else { 0.0 };
                        if w > 0.0 {
                            let k = w * w * (3.0 - 2.0 * w) * st_amt;
                            let s = l / sh_l;
                            r += (look.st_shadow[0] * s - r) * k;
                            g += (look.st_shadow[1] * s - g) * k;
                            b += (look.st_shadow[2] * s - b) * k;
                        }
                        let w = if l > pivot { (l - pivot) / (1.0 - pivot) } else { 0.0 };
                        if w > 0.0 {
                            let k = w * w * (3.0 - 2.0 * w) * st_amt;
                            let s = l / hi_l;
                            r += (look.st_highlight[0] * s - r) * k;
                            g += (look.st_highlight[1] * s - g) * k;
                            b += (look.st_highlight[2] * s - b) * k;
                        }
                    }
                    c = [r / 255.0, g / 255.0, b / 255.0];
                } else {
                    c[0] = lut(&tables[256..512], c[0] * 255.0) / 255.0;
                }
            }

            // vignette strength at this pixel: 0 at the centre, up to 1 in the corners
            let dx = ox + xx as f32 - cx;
            let mut gain = 1.0;
            if use_vig {
                let ex = dx / cx;
                let ey = dy / cy;
                let ellipse = sqrt(ex * ex + ey * ey) * 0.707_107;
                let circle = sqrt(dx * dx + dy * dy) / half_diag;
                let r = ellipse + (circle - ellipse) * vround;
                gain = 1.0 - vig * smooth(vstart, vstart + vwidth, r);
            }
            // film grain: strongest in the midtones, weaker in deep shadows and highlights
            let mut noise = [0.0f32; 3];
            if use_grain {
                let (sx, sy) = (ox + xx as f32, oy + yy as f32);
                let mono = grain(sx, sy, size, 0, seed);
                if rgb {
                    let base = mono * (1.0 - chroma);
                    noise[0] = base + grain(sx, sy, size, 1, seed) * chroma;
                    noise[1] = base + grain(sx, sy, size, 2, seed) * chroma;
                    noise[2] = base + grain(sx, sy, size, 3, seed) * chroma;
                } else {
                    noise[0] = mono;
                }
            }
            for k in 0..colour {
                let mut v = c[k];
                v = fade + v * (1.0 - fade); // lift the blacks
                if rgb {
                    if k == 0 {
                        v += warm;
                    } else if k == 2 {
                        v -= warm;
                    }
                }
                v *= gain;
                if use_grain {
                    let cl = v.clamp(0.0, 1.0);
                    let w = 1.0 - 0.6 * (2.0 * cl - 1.0) * (2.0 * cl - 1.0);
                    v += noise[if rgb { k } else { 0 }] * amount * w;
                }
                px[i + k] = orig[k] + (v - orig[k]) * strength;
            }
        }
    }
    0
}
