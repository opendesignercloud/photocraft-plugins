// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
//! ODC Mono: a PhotoCraft filter plug-in (plug-in ABI v1, see PhotoCraft `docs/plugins.md`).
//!
//! Black and white the way film does it: the colour picture is converted with the **spectral response of a film stock**
//! (how much each colour counts), optionally through a **colour filter** (a red filter darkens a blue sky), then given a
//! tone curve, faded blacks, **split toning** (sepia, selenium, cyanotype, platinum), a vignette and **film grain**.
//! A **look** sets every amount; the sliders are adjustments ADDED to the look, so 0 always means "as the look is".
//! Everything is per pixel (no neighbours), so the manifest asks for no overlap, and grain and vignette come from the
//! document position of every pixel: the result has no seams between host bands and is the same on every run for a seed.
//! `no_std`, no imports, no maths library.
#![no_std]
#![allow(dead_code)]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

const MANIFEST: &str = r#"{
  "id": "org.opendesignercloud.mono",
  "name": "ODC Mono…",
  "version": "1.0.0",
  "kind": "filter",
  "author": "Open Designer Cloud",
  "description": "Black and white like film: film-stock colour response, red/orange/yellow/green/blue filters, tone curve, split toning (sepia, selenium, cyanotype, platinum), vignette and grain. Ten looks; the sliders adjust the chosen look. Open source (Apache 2.0): opendesignercloud.com",
  "params": {
    "look":           {"type": "choice", "options": ["Tri-X 400", "Custom", "Neutral", "T-Max 100", "Orthochromatic", "Infrared", "Red Filter Drama", "Noir", "Sepia Print", "Selenium Print", "Cyanotype", "Platinum Fade"], "default": "Tri-X 400"},
    "strength":       {"type": "number", "min": 0,    "max": 100, "default": 100},
    "filter":         {"type": "choice", "options": ["As the look", "None", "Yellow", "Orange", "Red", "Green", "Blue"], "default": "As the look"},
    "filterStrength": {"type": "number", "min": -100, "max": 100, "default": 0},
    "exposure":       {"type": "number", "min": -100, "max": 100, "default": 0},
    "contrast":       {"type": "number", "min": -100, "max": 100, "default": 0},
    "highlights":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "shadows":        {"type": "number", "min": -100, "max": 100, "default": 0},
    "fade":           {"type": "number", "min": -100, "max": 100, "default": 0},
    "toneAmount":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "toneBalance":    {"type": "number", "min": -100, "max": 100, "default": 0},
    "shadowHue":      {"type": "number", "min": -180, "max": 180, "default": 0},
    "highlightHue":   {"type": "number", "min": -180, "max": 180, "default": 0},
    "grain":          {"type": "number", "min": -100, "max": 100, "default": 0},
    "grainSize":      {"type": "number", "min": -7,   "max": 7,   "default": 0},
    "vignette":       {"type": "number", "min": -100, "max": 100, "default": 0},
    "vignetteSize":   {"type": "number", "min": -100, "max": 100, "default": 0},
    "seed":           {"type": "int",    "min": 0,    "max": 9999, "default": 0}
  },
  "overlap": 0,
  "area": "content"
}"#;

// ---- colour filters and film stocks ----

/// How much of each colour reaches the film through the filter (red, green, blue), at full strength.
const FILTERS: [[f32; 3]; 6] = [
    [1.0, 1.0, 1.0],   // None
    [1.0, 0.90, 0.15], // Yellow
    [1.0, 0.55, 0.05], // Orange
    [1.0, 0.12, 0.02], // Red
    [0.15, 1.0, 0.10], // Green
    [0.05, 0.25, 1.0], // Blue
];
const F_NONE: usize = 0;
const F_YELLOW: usize = 1;
const F_ORANGE: usize = 2;
const F_RED: usize = 3;

// ---- the looks ----

/// `weights` is the film stock's response to red, green and blue (they need not add to 1). Toning colours are hue
/// degrees and saturation 0..1; amounts are slider units.
struct Look {
    name: &'static str,
    weights: [f32; 3],
    filter: usize, filter_strength: f32,
    curve: &'static [(f32, f32)],
    exposure: f32, contrast: f32, highlights: f32, shadows: f32, fade: f32,
    sh_hue: f32, sh_sat: f32, hi_hue: f32, hi_sat: f32, tone_amount: f32, tone_balance: f32,
    grain: f32, grain_size: f32, vignette: f32, v_size: f32, v_feather: f32,
}

const NEUTRAL: Look = Look {
    name: "Neutral", weights: [0.299, 0.587, 0.114], filter: F_NONE, filter_strength: 100.0,
    curve: &[],
    exposure: 0.0, contrast: 0.0, highlights: 0.0, shadows: 0.0, fade: 0.0,
    sh_hue: 30.0, sh_sat: 0.0, hi_hue: 40.0, hi_sat: 0.0, tone_amount: 0.0, tone_balance: 0.0,
    grain: 0.0, grain_size: 1.5, vignette: 0.0, v_size: 50.0, v_feather: 60.0,
};

// The stock weights are approximations of each emulsion's colour sensitivity, written for this plug-in.
const TRI_X: [f32; 3] = [0.34, 0.50, 0.16];

/// In the same order as the manifest's options (Custom is plain Rec. 601 luma with every amount at 0).
const LOOKS: [Look; 12] = [
    // The Tri-X tone curve of HyperImage's Tri-X looks, coarse grain, a little vignette.
    Look { name: "Tri-X 400", weights: TRI_X,
        curve: &[(0.0, 0.0), (63.0, 88.0), (123.0, 161.0), (168.0, 211.0), (201.0, 240.0), (226.0, 251.0), (251.0, 255.0)],
        grain: 35.0, grain_size: 1.8, vignette: 20.0, ..NEUTRAL },
    Look { name: "Custom", ..NEUTRAL },
    Look { name: "Neutral", ..NEUTRAL },
    // Fine grain, the T-Max tone curve of HyperImage's Kodak T-Max 400 look.
    Look { name: "T-Max 100", weights: [0.28, 0.55, 0.17],
        curve: &[(0.0, 0.0), (26.0, 28.0), (77.0, 66.0), (150.0, 146.0), (226.0, 243.0), (255.0, 255.0)],
        grain: 10.0, grain_size: 1.0, ..NEUTRAL },
    // Blind to red: lips and red brick go dark, blue skies go pale.
    Look { name: "Orthochromatic", weights: [0.0, 0.62, 0.38], contrast: 12.0, grain: 28.0, grain_size: 1.6, vignette: 15.0, ..NEUTRAL },
    // Foliage glows white, blue skies go black.
    Look { name: "Infrared", weights: [0.15, 0.95, -0.10], contrast: 10.0, fade: 6.0, grain: 30.0, grain_size: 1.7, vignette: 25.0, ..NEUTRAL },
    // A red filter on Tri-X: black skies, bright clouds, dramatic contrast.
    Look { name: "Red Filter Drama", weights: TRI_X, filter: F_RED, contrast: 28.0, grain: 30.0, grain_size: 1.6, vignette: 30.0, ..NEUTRAL },
    // Deep blacks, hard contrast, heavy grain and a strong vignette.
    Look { name: "Noir", weights: TRI_X, filter: F_ORANGE, filter_strength: 60.0, contrast: 40.0, highlights: -10.0, shadows: -20.0,
        grain: 45.0, grain_size: 1.9, vignette: 55.0, v_size: 55.0, ..NEUTRAL },
    // A warm brown darkroom print.
    Look { name: "Sepia Print", weights: TRI_X, filter: F_YELLOW, filter_strength: 50.0, contrast: 8.0, fade: 5.0,
        sh_hue: 28.0, sh_sat: 0.65, hi_hue: 42.0, hi_sat: 0.30, tone_amount: 70.0, grain: 14.0, vignette: 18.0, ..NEUTRAL },
    // Cool purple-brown shadows over neutral highlights: archival fibre print.
    Look { name: "Selenium Print", weights: TRI_X, contrast: 12.0,
        sh_hue: 340.0, sh_sat: 0.35, hi_hue: 30.0, hi_sat: 0.10, tone_amount: 55.0, grain: 18.0, vignette: 15.0, ..NEUTRAL },
    // Prussian blue shadows fading to pale blue highlights.
    Look { name: "Cyanotype", weights: TRI_X, contrast: 15.0, fade: 8.0,
        sh_hue: 215.0, sh_sat: 0.90, hi_hue: 195.0, hi_sat: 0.22, tone_amount: 85.0, grain: 10.0, ..NEUTRAL },
    // Soft, lifted blacks, a trace of warm tone, very fine grain.
    Look { name: "Platinum Fade", weights: [0.30, 0.54, 0.16], contrast: -12.0, fade: 18.0,
        sh_hue: 28.0, sh_sat: 0.25, hi_hue: 40.0, hi_sat: 0.12, tone_amount: 50.0, grain: 12.0, grain_size: 1.2, ..NEUTRAL },
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

/// RGB (0..1) of the colour with hue `h` degrees, saturation `s` 0..1 and full value.
fn hsv(h: f32, s: f32) -> [f32; 3] {
    let hh = (h - 360.0 * floor(h / 360.0)) / 60.0;
    let i = floor(hh);
    let f = hh - i;
    let (p, q, t) = (1.0 - s, 1.0 - s * f, 1.0 - s * (1.0 - f));
    match i as i32 {
        0 => [1.0, t, p],
        1 => [q, 1.0, p],
        2 => [p, 1.0, t],
        3 => [p, q, 1.0],
        4 => [t, p, 1.0],
        _ => [1.0, p, q],
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


/// Table for the grey value: exposure, contrast, shadows and highlights, then the stock's tone curve
/// (ODC Film Look / HyperImage buildLUTs without levels, gamma and temperature).
fn build_lut(l: &Look, exposure: f32, contrast: f32, highlights: f32, shadows: f32, out: &mut [f32]) {
    let exp_f = pow(2.0, exposure / 100.0);
    let con_f = 1.0 + contrast / 100.0;
    let sh = shadows / 100.0;
    let hi = highlights / 100.0;
    let mut cl = [0.0f32; 256];
    if l.curve.len() >= 2 {
        curve_lut(l.curve, &mut cl);
    }
    for i in 0..256 {
        let mut v = i as f32 * exp_f;
        v = (v - 128.0) * con_f + 128.0;
        let n = v / 255.0;
        v += sh * (1.0 - n) * 64.0;
        v += hi * n * 64.0;
        if l.curve.len() >= 2 {
            v = cl[floor(v.clamp(0.0, 255.0) + 0.5) as usize];
        }
        out[i] = v.clamp(0.0, 255.0);
    }
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
    let filter = match text(p, "filter") {
        Some(b"None") => F_NONE,
        Some(b"Yellow") => F_YELLOW,
        Some(b"Orange") => F_ORANGE,
        Some(b"Red") => F_RED,
        Some(b"Green") => 4,
        Some(b"Blue") => 5,
        _ => look.filter,
    };
    let adj = |key: &str, base: f32, lo: f32, hi: f32| (base + num(p, key, 0.0)).clamp(lo, hi);
    let strength = num(p, "strength", 100.0).clamp(0.0, 100.0) / 100.0;
    let f_strength = adj("filterStrength", look.filter_strength, 0.0, 100.0) / 100.0;
    let fade = adj("fade", look.fade, 0.0, 100.0) / 100.0 * 0.25;
    let tone_amt = adj("toneAmount", look.tone_amount, 0.0, 100.0) / 100.0;
    let tone_bal = adj("toneBalance", look.tone_balance, -100.0, 100.0);
    let sh_col = hsv(look.sh_hue + num(p, "shadowHue", 0.0), look.sh_sat);
    let hi_col = hsv(look.hi_hue + num(p, "highlightHue", 0.0), look.hi_sat);
    let amount = adj("grain", look.grain, 0.0, 100.0) / 100.0 * 0.12;
    let size = adj("grainSize", look.grain_size, 1.0, 8.0);
    let vig = adj("vignette", look.vignette, 0.0, 100.0) / 100.0;
    let vstart = 1.0 - adj("vignetteSize", look.v_size, 0.0, 100.0) / 100.0;
    let vwidth = 0.15 + look.v_feather / 100.0 * 0.85;
    let vround = 0.5f32;
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
    let toning = rgb && tone_amt > 0.0;

    // Channel weights through the filter, normalised so that neutral grey stays the same grey.
    let mut wgt = [0.0f32; 3];
    for c in 0..3 {
        let tr = 1.0 + (FILTERS[filter][c] - 1.0) * f_strength;
        wgt[c] = look.weights[c] * tr;
    }
    let wsum = wgt[0] + wgt[1] + wgt[2];
    let inv = if wsum.abs() > 1e-4 { 1.0 / wsum } else { 0.0 };
    for c in 0..3 {
        wgt[c] *= inv;
    }

    // Tone table (256 floats from fresh memory pages).
    let at = pc_alloc(256 * 4);
    if at == 0 {
        return 3;
    }
    // SAFETY: `pc_alloc` returned a block of 1 KiB inside our memory.
    let table = unsafe { core::slice::from_raw_parts_mut(at as usize as *mut f32, 256) };
    build_lut(look, adj("exposure", look.exposure, -100.0, 100.0), adj("contrast", look.contrast, -100.0, 100.0),
        adj("highlights", look.highlights, -100.0, 100.0), adj("shadows", look.shadows, -100.0, 100.0), table);

    let sh_l = (luma(sh_col[0], sh_col[1], sh_col[2])).max(0.02);
    let hi_l = (luma(hi_col[0], hi_col[1], hi_col[2])).max(0.02);
    let pivot = (0.5 + tone_bal / 200.0).clamp(0.05, 0.95);

    for yy in 0..height as usize {
        let dy = oy + yy as f32 - cy;
        for xx in 0..width as usize {
            let i = (yy * width as usize + xx) * n;
            if alpha && px[i + n - 1] <= 0.0 {
                continue; // fully transparent: nothing to convert
            }
            let orig = if rgb { [px[i], px[i + 1], px[i + 2]] } else { [px[i], 0.0, 0.0] };

            // 1. the stock: convert to grey, then the tone table
            let g = if rgb { (wgt[0] * orig[0] + wgt[1] * orig[1] + wgt[2] * orig[2]).clamp(0.0, 1.0) } else { orig[0] };
            let mut v = lut(table, g * 255.0) / 255.0;
            // 2. faded blacks
            v = fade + v * (1.0 - fade);
            // 3. split toning: shadow colour below the pivot, highlight colour above, scaled to the pixel's brightness
            let mut c = [v, v, v];
            if toning {
                let l = v.clamp(0.0, 1.0);
                let w = if l < pivot { (pivot - l) / pivot } else { 0.0 };
                if w > 0.0 {
                    let k = smooth(0.0, 1.0, w) * tone_amt;
                    let s = l / sh_l;
                    for q in 0..3 {
                        c[q] += (sh_col[q] * s - c[q]) * k;
                    }
                }
                let w = if l > pivot { (l - pivot) / (1.0 - pivot) } else { 0.0 };
                if w > 0.0 {
                    let k = smooth(0.0, 1.0, w) * tone_amt;
                    let s = l / hi_l;
                    for q in 0..3 {
                        c[q] += (hi_col[q] * s - c[q]) * k;
                    }
                }
            }
            // toning scales a colour to the pixel's brightness, which can overshoot 1: keep it in range (HDR whites stay)
            if toning {
                let top = v.max(1.0);
                for q in 0..3 {
                    c[q] = c[q].clamp(0.0, top);
                }
            }
            // 4. vignette
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
            // 5. film grain (the same on every channel), strongest in the midtones
            let mut noise = 0.0;
            if use_grain {
                noise = grain(ox + xx as f32, oy + yy as f32, size, 0, seed) * amount;
            }
            for k in 0..colour {
                let mut o = if rgb { c[k] } else { c[0] };
                o *= gain;
                if use_grain {
                    let cl = o.clamp(0.0, 1.0);
                    o += noise * (1.0 - 0.6 * (2.0 * cl - 1.0) * (2.0 * cl - 1.0));
                }
                px[i + k] = orig[k] + (o - orig[k]) * strength;
            }
        }
    }
    0
}
