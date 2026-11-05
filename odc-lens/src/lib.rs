// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
//! ODC Lens: a PhotoCraft filter plug-in (plug-in ABI v1, see PhotoCraft `docs/plugins.md`).
//!
//! What a real piece of glass does to an image that a perfect one does not, ported from HyperImage's
//! Lens tool (`renderer/lens.js`): barrel/pincushion distortion and lateral chromatic aberration (one
//! radial remap), field softness with longitudinal chromatic aberration, bloom/halation, vignette.
//! A **look** (the six HyperImage presets) sets every amount; the sliders are adjustments ADDED to the
//! look, so 0 always means "as the look is".
//!
//! Sandbox notes. `no_std`, no imports, so there is no maths library (sqrt, cos, floor are written
//! here) and no storage. The effects need neighbouring pixels, so the manifest asks the host for the
//! maximum overlap (256 px) and every stage keeps within that budget:
//!   * distortion and lateral CA read up to `|k| * norm` px away; `k` is limited to 72 px of shift,
//!   * the blurs (three box passes of radius r) need 3 r px; their radius is limited to what is left.
//! On a photo up to about 2000 px on the short side nothing is limited; beyond that the largest
//! distortion, softness and bloom radius are scaled back to fit (see README).
//! Bloom is taken from the geometry pass, in parallel with the softness pass (HyperImage takes it
//! from the softened image); this keeps the total reach at distortion + one blur instead of the sum.
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

const MANIFEST: &str = r#"{
  "id": "org.opendesignercloud.lens",
  "name": "ODC Lens…",
  "version": "1.0.0",
  "kind": "filter",
  "author": "Open Designer Cloud",
  "description": "What real glass does to a picture: vintage softness, colour fringes, bloom and halation, barrel distortion, vignette. Six looks (Vintage Glass, Anamorphic, Dreamy Portrait, Halation, Toy Camera, Subtle); the sliders adjust the chosen look. Open source (Apache 2.0): opendesignercloud.com",
  "params": {
    "look":           {"type": "choice", "options": ["Vintage Glass", "Custom", "Anamorphic", "Dreamy Portrait", "Halation", "Toy Camera", "Subtle"], "default": "Vintage Glass"},
    "strength":       {"type": "number", "min": 0,    "max": 100, "default": 100},
    "distortion":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "caLateral":      {"type": "number", "min": -100, "max": 100, "default": 0},
    "softness":       {"type": "number", "min": -100, "max": 100, "default": 0},
    "softStart":      {"type": "number", "min": -100, "max": 100, "default": 0},
    "caLong":         {"type": "number", "min": -100, "max": 100, "default": 0},
    "bloom":          {"type": "number", "min": -100, "max": 100, "default": 0},
    "bloomRadius":    {"type": "number", "min": -100, "max": 100, "default": 0},
    "bloomThreshold": {"type": "number", "min": -100, "max": 100, "default": 0},
    "bloomHue":       {"type": "number", "min": -180, "max": 180, "default": 0},
    "bloomTint":      {"type": "number", "min": -100, "max": 100, "default": 0},
    "vignette":       {"type": "number", "min": -100, "max": 100, "default": 0}
  },
  "overlap": 256,
  "area": "content"
}"#;

// ---- the looks (HyperImage LENS_BUILTINS, lens.js:315-351) ----

/// Amounts are in HyperImage's slider units. The bloom tint is stored as hue (degrees) and
/// saturation (percent) because a plug-in dialog has no colour picker; the six hex tints convert exactly.
struct Look {
    name: &'static str,
    distortion: f32, ca_lateral: f32, softness: f32, soft_start: f32, ca_long: f32,
    bloom: f32, bloom_radius: f32, bloom_threshold: f32, hue: f32, tint: f32, vignette: f32,
}

const NEUTRAL: Look = Look {
    name: "Custom",
    distortion: 0.0, ca_lateral: 0.0, softness: 0.0, soft_start: 40.0, ca_long: 0.0,
    bloom: 0.0, bloom_radius: 24.0, bloom_threshold: 70.0, hue: 0.0, tint: 0.0, vignette: 0.0,
};

/// In the same order as the manifest's options.
const LOOKS: [Look; 7] = [
    // #ffd9b0
    Look { name: "Vintage Glass", softness: 45.0, soft_start: 35.0, ca_long: 35.0, ca_lateral: 18.0, bloom: 38.0, bloom_radius: 30.0, bloom_threshold: 62.0, hue: 31.1, tint: 31.0, vignette: 22.0, ..NEUTRAL },
    NEUTRAL,
    // #9fc4ff
    Look { name: "Anamorphic", distortion: 18.0, ca_lateral: 38.0, softness: 40.0, soft_start: 28.0, ca_long: -28.0, bloom: 45.0, bloom_radius: 42.0, bloom_threshold: 55.0, hue: 216.9, tint: 37.6, vignette: 30.0, ..NEUTRAL },
    // #ffe6cf
    Look { name: "Dreamy Portrait", softness: 30.0, soft_start: 58.0, ca_long: 18.0, bloom: 55.0, bloom_radius: 46.0, bloom_threshold: 54.0, hue: 28.8, tint: 18.8, vignette: 18.0, ..NEUTRAL },
    // #ff9a6b
    Look { name: "Halation", bloom: 60.0, bloom_radius: 34.0, bloom_threshold: 58.0, hue: 19.1, tint: 58.0, ..NEUTRAL },
    // #ffffff
    Look { name: "Toy Camera", distortion: 58.0, ca_lateral: 62.0, softness: 68.0, soft_start: 18.0, ca_long: 45.0, bloom: 22.0, bloom_radius: 20.0, bloom_threshold: 72.0, vignette: 55.0, ..NEUTRAL },
    Look { name: "Subtle", softness: 18.0, soft_start: 72.0, ca_long: 12.0, bloom: 12.0, bloom_radius: 18.0, bloom_threshold: 80.0, vignette: 10.0, ..NEUTRAL },
];

const HAS_ALPHA: u32 = 1 << 8;
const OVERLAP: f32 = 256.0;
/// Most any geometry stage may move a pixel (px); the rest of the overlap is left for the blurs.
const MAX_SHIFT: f32 = 72.0;

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

// ---- tiny JSON readers: find `"key":` and parse the number or string after it ----

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

/// cos(x) for 0 <= x <= pi/2: Taylor series to x^10 (error under 1e-6 over the range).
fn cos(x: f32) -> f32 {
    let x2 = x * x;
    1.0 - x2 * (0.5 - x2 * (1.0 / 24.0 - x2 * (1.0 / 720.0 - x2 * (1.0 / 40320.0 - x2 * (1.0 / 3_628_800.0)))))
}

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
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

// ---- box blur: three passes of a separable running sum, edges clamped (HyperImage boxBlur3) ----

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

fn blur3(p: &mut [f32], line: &mut [f32], w: usize, h: usize, r: i32, b: Bounds) {
    if r < 1 {
        return;
    }
    for _ in 0..3 {
        box_pass(p, line, w, h, r, false, b);
        box_pass(p, line, w, h, r, true, b);
    }
}

/// Radii up to this are blurred at full resolution (identical to HyperImage); larger ones at a
/// reduced resolution, which is what makes a big blur affordable in the sandbox.
const EXACT_RADIUS: i32 = 8;

/// Blur of radius `r` (in pixels). Large radii: average s x s blocks, blur that small plane, spread it
/// back out with bilinear interpolation. A blur this wide is smooth, so the result is within a level or
/// two of the full-resolution one at a fraction of the cost.
fn blur(p: &mut [f32], line: &mut [f32], low: &mut [f32], w: usize, h: usize, r: i32, b: Bounds) {
    if r <= EXACT_RADIUS {
        return blur3(p, line, w, h, r, b);
    }
    let s = ((r / 4) as usize).clamp(2, 16);
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
    blur3(&mut low[..lw * lh], line, lw, lh, lr, sb);
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
    let np = w * h;
    // SAFETY: see the function's contract.
    let px = unsafe { core::slice::from_raw_parts_mut(buf, np * n) };
    let p = unsafe { core::slice::from_raw_parts(params, params_len as usize) };

    // The look, then the sliders as adjustments added to it.
    let look = match text(p, "look") {
        Some(name) => LOOKS.iter().find(|l| l.name.as_bytes() == name).unwrap_or(&LOOKS[0]),
        None => &LOOKS[0],
    };
    let adj = |key: &str, base: f32, lo: f32, hi: f32| (base + num(p, key, 0.0)).clamp(lo, hi);
    let strength = num(p, "strength", 100.0).clamp(0.0, 100.0) / 100.0;
    let distortion = adj("distortion", look.distortion, -100.0, 100.0);
    let ca_lat = adj("caLateral", look.ca_lateral, 0.0, 100.0);
    let softness = adj("softness", look.softness, 0.0, 100.0);
    let soft_start = adj("softStart", look.soft_start, 0.0, 100.0);
    let ca_long = adj("caLong", look.ca_long, -100.0, 100.0);
    let bloom = adj("bloom", look.bloom, 0.0, 100.0);
    let bloom_radius = adj("bloomRadius", look.bloom_radius, 1.0, 120.0);
    let bloom_thr = adj("bloomThreshold", look.bloom_threshold, 0.0, 100.0) / 100.0;
    let hue = look.hue + num(p, "bloomHue", 0.0);
    let tint_sat = adj("bloomTint", look.tint, 0.0, 100.0) / 100.0;
    let vig = adj("vignette", look.vignette, 0.0, 100.0) / 100.0;

    // The band's place in the document (the buffer's top-left sample, margins included) and the canvas.
    let ox = num(p, "x", 0.0);
    let oy = num(p, "y", 0.0);
    let cw = num(p, "canvasWidth", width as f32).max(1.0);
    let ch = num(p, "canvasHeight", height as f32).max(1.0);
    let (cx, cy) = ((cw - 1.0) * 0.5, (ch - 1.0) * 0.5);
    let norm = sqrt(cx * cx + cy * cy).max(1.0);
    let short = cw.min(ch);

    // Reach budget: how far geometry may move a pixel, and what that leaves for the blurs.
    let kmax = MAX_SHIFT / norm;
    let k = (distortion / 100.0 * 0.35).clamp(-kmax, kmax);
    let ca = (ca_lat / 100.0 * 0.012).min(MAX_SHIFT / norm);
    let reach = (if k < 0.0 { -k } else { k } + ca) * norm + 2.0;
    let blur_cap = (floor((OVERLAP - reach - 1.0) / 3.0) as i32).max(1);

    let use_geo = k != 0.0 || ca != 0.0;
    let use_soft = softness > 0.0;
    let use_bloom = bloom > 0.0;
    let nc = colour;

    // Blur radii (HyperImage's formulas, kept inside the budget).
    let bloom_r = (round(bloom_radius * short / 1000.0) as i32).clamp(1, blur_cap);
    let soft_r = (round(softness / 100.0 * short * 0.035) as i32).clamp(1, blur_cap);

    // The host hands over the band plus `overlap` pixels of context on every side and keeps only the
    // middle. Work only on the middle plus what the blurs reach (3 passes of radius r, plus a little for
    // the reduced-resolution blur): the rest of the margin is only ever read by the geometry pass.
    let ov = (num(p, "overlap", 0.0).max(0.0) as usize).min(w / 2).min(h / 2);
    let (cx0, cy0, cx1, cy1) = (ov, ov, w - ov, h - ov);
    let widest = if use_bloom { bloom_r } else { 0 }.max(if use_soft { soft_r } else { 0 });
    let margin = (3 * widest + 40) as usize;
    let rx0 = cx0.saturating_sub(margin);
    let ry0 = cy0.saturating_sub(margin);
    let rx1 = (cx1 + margin).min(w);
    let ry1 = (cy1 + margin).min(h);
    let (rw, rh) = (rx1 - rx0, ry1 - ry0);
    let nr = rw * rh;
    // Document position of the working area's top-left sample.
    let (rox, roy) = (ox + rx0 as f32, oy + ry0 as f32);

    // Where the canvas edges are inside the working area: blurs clamp there, as HyperImage's do.
    let bounds = Bounds {
        xlo: (-(rox as i32)).clamp(0, rw as i32 - 1),
        xhi: (cw as i32 - 1 - rox as i32).clamp(0, rw as i32 - 1),
        ylo: (-(roy as i32)).clamp(0, rh as i32 - 1),
        yhi: (ch as i32 - 1 - roy as i32).clamp(0, rh as i32 - 1),
    };

    // Working planes (planar, one per colour channel).
    let Some(line) = plane(rw.max(rh)) else { return 3 };
    let Some(low) = plane(nr / 4 + rw + rh + 64) else { return 3 };
    let mut geo: [&mut [f32]; 3] = [&mut [], &mut [], &mut []];
    for c in 0..nc {
        match plane(nr) { Some(a) => geo[c] = a, None => return 3 }
    }
    // Geometry pass: distortion + lateral CA, one bilinear read per channel (HyperImage geometryPass).
    if use_geo {
        let mag = [1.0 + ca, 1.0, 1.0 - ca];
        let (wm, hm) = (w as i32 - 1, h as i32 - 1);
        for yy in 0..rh {
            let dy = roy + yy as f32 - cy;
            for xx in 0..rw {
                let dx = rox + xx as f32 - cx;
                let r2 = (dx * dx + dy * dy) / (norm * norm);
                let d = 1.0 + k * r2;
                for c in 0..nc {
                    let m = d * if rgb { mag[c] } else { 1.0 };
                    // source position in buffer coordinates
                    let sx = cx + dx * m - ox;
                    let sy = cy + dy * m - oy;
                    let fx0 = floor(sx);
                    let fy0 = floor(sy);
                    let (tx, ty) = (sx - fx0, sy - fy0);
                    let (x0, y0) = (fx0 as i32, fy0 as i32);
                    let xa = x0.clamp(0, wm) as usize;
                    let xb = (x0 + 1).clamp(0, wm) as usize;
                    let ya = y0.clamp(0, hm) as usize;
                    let yb = (y0 + 1).clamp(0, hm) as usize;
                    let p00 = px[(ya * w + xa) * n + c];
                    let p10 = px[(ya * w + xb) * n + c];
                    let p01 = px[(yb * w + xa) * n + c];
                    let p11 = px[(yb * w + xb) * n + c];
                    geo[c][yy * rw + xx] = p00 + (p10 - p00) * tx + (p01 - p00) * ty + (p00 - p10 - p01 + p11) * tx * ty;
                }
            }
        }
    } else {
        for c in 0..nc {
            for yy in 0..rh {
                for xx in 0..rw {
                    geo[c][yy * rw + xx] = px[((ry0 + yy) * w + rx0 + xx) * n + c];
                }
            }
        }
    }

    // Bloom: what is above the luminance threshold, blurred (taken from the geometry result).
    let mut glow: [&mut [f32]; 3] = [&mut [], &mut [], &mut []];
    if use_bloom {
        for c in 0..nc {
            match plane(nr) { Some(a) => glow[c] = a, None => return 3 }
        }
        let denom = (1.0 - bloom_thr).max(1.0 / 255.0);
        for i in 0..nr {
            let l = if rgb { luma(geo[0][i], geo[1][i], geo[2][i]) } else { geo[0][i] };
            if l <= bloom_thr {
                continue;
            }
            let kk = (l - bloom_thr) / denom;
            for c in 0..nc {
                glow[c][i] = geo[c][i] * kk;
            }
        }
        for c in 0..nc {
            blur(glow[c], line, low, rw, rh, bloom_r, bounds);
        }
    }

    // Field softness + longitudinal CA: three levels (sharp, mid, far) blended along the radius.
    let mut soft: [&mut [f32]; 3] = [&mut [], &mut [], &mut []];
    if use_soft {
        let mid_r = (round(soft_r as f32 / 2.0) as i32).max(1);
        let start = soft_start / 100.0;
        let span = (1.0 - start).max(0.01);
        let shift = ca_long / 100.0 * 0.25;
        let ch_shift = [shift, 0.0, -shift];
        let (Some(mid), Some(far)) = (plane(nr), plane(nr)) else { return 3 };
        for c in 0..nc {
            match plane(nr) { Some(a) => soft[c] = a, None => return 3 }
        }
        for c in 0..nc {
            mid.copy_from_slice(geo[c]);
            far.copy_from_slice(geo[c]);
            blur(mid, line, low, rw, rh, mid_r, bounds);
            blur(far, line, low, rw, rh, soft_r, bounds);
            for yy in 0..rh {
                let dy = roy + yy as f32 - cy;
                for xx in 0..rw {
                    let dx = rox + xx as f32 - cx;
                    let r = sqrt(dx * dx + dy * dy) / norm;
                    let t = ((r - start) / span + if rgb { ch_shift[c] } else { 0.0 }).clamp(0.0, 1.0);
                    let t = t * t * (3.0 - 2.0 * t);
                    let a = t * 2.0;
                    let i = yy * rw + xx;
                    let (s, m, f) = (geo[c][i], mid[i], far[i]);
                    soft[c][i] = if a <= 1.0 { s + (m - s) * a } else { m + (f - m) * (a - 1.0) };
                }
            }
        }
    }

    // Tint of the bloom (a gray document gets its average).
    let tint = {
        let t = hsv(hue, tint_sat);
        if rgb { t } else { let g = (t[0] + t[1] + t[2]) / 3.0; [g, g, g] }
    };
    let amt = bloom / 100.0 * 2.2;

    // Combine, vignette, and mix with the original by strength: only the part the host keeps.
    for y in cy0..cy1 {
        let dy = oy + y as f32 - cy;
        for x in cx0..cx1 {
            let i = (y - ry0) * rw + (x - rx0);
            let dx = ox + x as f32 - cx;
            let mut f = 1.0;
            if vig > 0.0 {
                let r = sqrt(dx * dx + dy * dy) / norm;
                let c = cos((r * 0.9).min(core::f32::consts::FRAC_PI_2));
                let c2 = c * c;
                f = 1.0 - (1.0 - c2 * c2) * vig;
            }
            for c in 0..nc {
                let mut v = if use_soft { soft[c][i] } else { geo[c][i] };
                if use_bloom {
                    v += glow[c][i] * tint[c] * amt;
                }
                v *= f;
                let o = px[(y * w + x) * n + c];
                px[(y * w + x) * n + c] = o + (v - o) * strength;
            }
        }
    }
    0
}
