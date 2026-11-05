// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
//! ODC Glow: a PhotoCraft filter plug-in (plug-in ABI v1, see PhotoCraft `docs/plugins.md`).
//!
//! Glow, haze and soft focus in one dialog: the Orton effect, glamour glow, soft focus, Pro-Mist style
//! highlight bloom. The picture is blurred (only its brightest parts if a threshold is set), the blur is
//! graded (contrast, saturation, warmth, brightness) and blended back with Screen, Soft Light, Overlay,
//! Lighten or Normal. A **look** sets every amount; the sliders are adjustments ADDED to the look, so 0
//! always means "as the look is".
//! `no_std`, no imports, no maths library. The blur is three box passes of radius r, so the manifest asks
//! the host for the maximum overlap (256 px) and r is limited to what fits (about 85 px). Radii above 8 px
//! are blurred at reduced resolution (smooth, so within a level or two of the exact blur).
#![no_std]
#![allow(dead_code)]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

const MANIFEST: &str = r#"{
  "id": "org.opendesignercloud.glow",
  "name": "ODC Glow…",
  "version": "1.0.0",
  "kind": "filter",
  "author": "Open Designer Cloud",
  "description": "Glow, haze and soft focus: the Orton effect, glamour glow, soft focus, Pro-Mist highlight bloom, dreamy haze and golden glow. Six looks; the sliders adjust the chosen look. Open source (Apache 2.0): opendesignercloud.com",
  "params": {
    "look":       {"type": "choice", "options": ["Orton Classic", "Custom", "Glamour Glow", "Soft Focus", "Pro-Mist", "Dreamy Haze", "Golden Glow"], "default": "Orton Classic"},
    "strength":   {"type": "number", "min": 0,    "max": 100, "default": 100},
    "blend":      {"type": "choice", "options": ["As the look", "Screen", "Soft Light", "Overlay", "Lighten", "Normal"], "default": "As the look"},
    "radius":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "amount":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "threshold":  {"type": "number", "min": -100, "max": 100, "default": 0},
    "contrast":   {"type": "number", "min": -100, "max": 100, "default": 0},
    "saturation": {"type": "number", "min": -100, "max": 100, "default": 0},
    "warmth":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "brightness": {"type": "number", "min": -100, "max": 100, "default": 0},
    "protect":    {"type": "number", "min": -100, "max": 100, "default": 0}
  },
  "overlap": 256,
  "area": "content"
}"#;

// ---- the looks ----

const SCREEN: u8 = 0;
const SOFT_LIGHT: u8 = 1;
const OVERLAP_B: u8 = 2;
const LIGHTEN: u8 = 3;
const NORMAL: u8 = 4;

/// `radius` is in thousandths of the short side; the rest are slider units (0..100 or -100..100).
struct Look {
    name: &'static str,
    blend: u8,
    radius: f32, amount: f32, threshold: f32, contrast: f32, saturation: f32, warmth: f32, brightness: f32, protect: f32,
}

const NEUTRAL: Look = Look {
    name: "Custom", blend: SCREEN,
    radius: 20.0, amount: 0.0, threshold: 0.0, contrast: 0.0, saturation: 0.0, warmth: 0.0, brightness: 0.0, protect: 0.0,
};

/// In the same order as the manifest's options.
const LOOKS: [Look; 7] = [
    // The Orton effect: a contrasty, saturated blur laid over the sharp picture.
    Look { name: "Orton Classic", blend: OVERLAP_B, radius: 22.0, amount: 50.0, contrast: 25.0, saturation: 18.0, brightness: 6.0, ..NEUTRAL },
    NEUTRAL,
    // A soft, bright, slightly warm veil that keeps the shadows clean (skin, portraits).
    Look { name: "Glamour Glow", blend: SCREEN, radius: 14.0, amount: 38.0, contrast: -25.0, saturation: -10.0, warmth: 15.0, protect: 35.0, ..NEUTRAL },
    // The picture mixed with a softer copy of itself.
    Look { name: "Soft Focus", blend: NORMAL, radius: 9.0, amount: 55.0, ..NEUTRAL },
    // Highlights spill into their surroundings, like a Black Pro-Mist filter.
    Look { name: "Pro-Mist", blend: SCREEN, radius: 30.0, amount: 70.0, threshold: 52.0, warmth: 10.0, ..NEUTRAL },
    // A wide, cool, low-contrast haze.
    Look { name: "Dreamy Haze", blend: SCREEN, radius: 45.0, amount: 45.0, threshold: 20.0, contrast: -30.0, saturation: -15.0, warmth: -18.0, brightness: 10.0, protect: 20.0, ..NEUTRAL },
    // Warm light bleeding out of the highlights.
    Look { name: "Golden Glow", blend: SOFT_LIGHT, radius: 26.0, amount: 70.0, threshold: 40.0, saturation: 18.0, warmth: 55.0, brightness: 6.0, ..NEUTRAL },
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

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
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

/// Base `a` and glow `b` (0..1) blended by `mode`.
fn blend(a: f32, b: f32, mode: u8) -> f32 {
    match mode {
        SCREEN => 1.0 - (1.0 - a) * (1.0 - b),
        SOFT_LIGHT => (1.0 - 2.0 * b) * a * a + 2.0 * a * b,
        OVERLAP_B => if a < 0.5 { 2.0 * a * b } else { 1.0 - 2.0 * (1.0 - a) * (1.0 - b) },
        LIGHTEN => if a > b { a } else { b },
        _ => b,
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    let mode_b = match text(p, "blend") {
        Some(b"Screen") => SCREEN,
        Some(b"Soft Light") => SOFT_LIGHT,
        Some(b"Overlay") => OVERLAP_B,
        Some(b"Lighten") => LIGHTEN,
        Some(b"Normal") => NORMAL,
        _ => look.blend,
    };
    let adj = |key: &str, base: f32, lo: f32, hi: f32| (base + num(p, key, 0.0)).clamp(lo, hi);
    let strength = num(p, "strength", 100.0).clamp(0.0, 100.0) / 100.0;
    let radius = adj("radius", look.radius, 1.0, 120.0);
    let amount = adj("amount", look.amount, 0.0, 100.0) / 100.0;
    let thr = adj("threshold", look.threshold, 0.0, 100.0) / 100.0;
    let contrast = 1.0 + adj("contrast", look.contrast, -100.0, 100.0) / 100.0;
    let sat = 1.0 + adj("saturation", look.saturation, -100.0, 100.0) / 100.0;
    let warm = adj("warmth", look.warmth, -100.0, 100.0) / 100.0 * 0.12;
    let bright = 1.0 + adj("brightness", look.brightness, -100.0, 100.0) / 100.0;
    let protect = adj("protect", look.protect, 0.0, 100.0) / 100.0;
    if amount <= 0.0 {
        return 0;
    }

    // The band's place in the document (the buffer's top-left sample, margins included) and the canvas.
    let ox = num(p, "x", 0.0);
    let oy = num(p, "y", 0.0);
    let cw = num(p, "canvasWidth", width as f32).max(1.0);
    let ch = num(p, "canvasHeight", height as f32).max(1.0);
    let short = cw.min(ch);
    let nc = colour;

    let blur_cap = (floor((OVERLAP - 1.0) / 3.0) as i32).max(1);
    let r = (round(radius * short / 1000.0) as i32).clamp(1, blur_cap);

    // The host hands over the band plus `overlap` pixels of context on every side and keeps only the
    // middle: work on the middle plus what the blur reaches.
    let ov = (num(p, "overlap", 0.0).max(0.0) as usize).min(w / 2).min(h / 2);
    let (cx0, cy0, cx1, cy1) = (ov, ov, w - ov, h - ov);
    let margin = (3 * r + 40) as usize;
    let rx0 = cx0.saturating_sub(margin);
    let ry0 = cy0.saturating_sub(margin);
    let rx1 = (cx1 + margin).min(w);
    let ry1 = (cy1 + margin).min(h);
    let (rw, rh) = (rx1 - rx0, ry1 - ry0);
    let nr = rw * rh;
    let (rox, roy) = (ox + rx0 as f32, oy + ry0 as f32);
    // Canvas edges inside the working area: the blur clamps there, as an image editor's does.
    let bounds = Bounds {
        xlo: (-(rox as i32)).clamp(0, rw as i32 - 1),
        xhi: (cw as i32 - 1 - rox as i32).clamp(0, rw as i32 - 1),
        ylo: (-(roy as i32)).clamp(0, rh as i32 - 1),
        yhi: (ch as i32 - 1 - roy as i32).clamp(0, rh as i32 - 1),
    };

    let Some(line) = plane(rw.max(rh)) else { return 3 };
    let Some(low) = plane(nr / 4 + rw + rh + 64) else { return 3 };
    let mut g: [&mut [f32]; 3] = [&mut [], &mut [], &mut []];
    for c in 0..nc {
        match plane(nr) { Some(a) => g[c] = a, None => return 3 }
    }
    // The source of the glow: the picture, weighted toward its highlights when a threshold is set.
    for yy in 0..rh {
        for xx in 0..rw {
            let i = ((ry0 + yy) * w + rx0 + xx) * n;
            let l = if rgb { luma(px[i], px[i + 1], px[i + 2]) } else { px[i] };
            let wgt = if thr > 0.0 { smooth(thr, (thr + 0.25).min(1.0).max(thr + 0.01), l) } else { 1.0 };
            for c in 0..nc {
                g[c][yy * rw + xx] = px[i + c] * wgt;
            }
        }
    }
    for c in 0..nc {
        blur(g[c], line, low, rw, rh, r, bounds);
    }

    // Combine over the part the host keeps.
    for y in cy0..cy1 {
        for x in cx0..cx1 {
            let i = (y * w + x) * n;
            let j = (y - ry0) * rw + (x - rx0);
            let base = if rgb { [px[i], px[i + 1], px[i + 2]] } else { [px[i], 0.0, 0.0] };
            let l = if rgb { luma(base[0], base[1], base[2]) } else { base[0] };
            let mut gl = [g[0][j], if rgb { g[1][j] } else { 0.0 }, if rgb { g[2][j] } else { 0.0 }];
            // grade the glow: brightness, contrast about mid gray, saturation, warmth
            let gl_l = if rgb { luma(gl[0], gl[1], gl[2]) } else { gl[0] };
            for c in 0..nc {
                let mut v = gl[c] * bright;
                if rgb {
                    v = gl_l * bright + (v - gl_l * bright) * sat;
                    if c == 0 { v += warm } else if c == 2 { v -= warm }
                }
                gl[c] = ((v - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
            }
            // glow weight: the amount, less in the shadows when they are protected
            let k = amount * (1.0 - protect * (1.0 - smooth(0.0, 0.5, l)));
            for c in 0..nc {
                let a = base[c].clamp(0.0, 1.0);
                let blended = blend(a, gl[c], mode_b);
                let v = base[c] + (blended - a) * k;
                px[i + c] = base[c] + (v - base[c]) * strength;
            }
        }
    }
    0
}
