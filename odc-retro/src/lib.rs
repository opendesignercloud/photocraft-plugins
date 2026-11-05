// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
//! ODC Retro: a PhotoCraft filter plug-in (plug-in ABI v1, see PhotoCraft `docs/plugins.md`).
//!
//! Old screens and old hardware: **VHS** (colour smeared sideways, colour fringes, static, glitch bands), **CRT**
//! (scanlines, RGB phosphor mask, vignette), **pixel art with a fixed palette and ordered dither** (Game Boy, 1-bit,
//! CGA, PICO-8, amber and green terminals). A **look** sets every amount; the sliders are adjustments ADDED to the look.
//! Order: pixelate, glitch bands, RGB split, colour smear, saturation and contrast, static, palette with ordered dither,
//! scanlines and phosphor mask, vignette.
//! `no_std`, no imports, no maths library. Everything is a function of the pixel's document position (blocks, bands,
//! scanlines and the dither matrix are aligned to the canvas, not to the band the host sends), so there are no seams and
//! the same settings with the same seed give the same pixels. Reach (kept within the 128 px overlap): pixel block 32 +
//! glitch shift 48 + RGB split 16 + colour smear 24 = 120. RGB documents only.
#![no_std]
#![allow(dead_code)]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

const MANIFEST: &str = r#"{
  "id": "org.opendesignercloud.retro",
  "name": "ODC Retro…",
  "version": "1.0.0",
  "kind": "filter",
  "author": "Open Designer Cloud",
  "description": "Old screens and old hardware: VHS tape, CRT scanlines and phosphor, glitch, and pixel art with fixed palettes and ordered dither (Game Boy, 1-bit, CGA, PICO-8, amber and green terminals). Nine looks; the sliders adjust the chosen look. RGB documents. Open source (Apache 2.0): opendesignercloud.com",
  "params": {
    "look":       {"type": "choice", "options": ["VHS Tape", "Custom", "CRT Monitor", "Game Boy", "1-Bit Dither", "CGA Arcade", "PICO-8", "Amber Terminal", "Glitch"], "default": "VHS Tape"},
    "strength":   {"type": "number", "min": 0,    "max": 100, "default": 100},
    "palette":    {"type": "choice", "options": ["As the look", "None", "Game Boy", "1-Bit", "Grayscale 4", "CGA Cyan Magenta", "CGA Red Green", "PICO-8", "Amber Terminal", "Green Phosphor"], "default": "As the look"},
    "pixelSize":  {"type": "number", "min": -20,  "max": 20,  "default": 0},
    "dither":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "scanlines":  {"type": "number", "min": -100, "max": 100, "default": 0},
    "lineSize":   {"type": "number", "min": -3,   "max": 3,   "default": 0},
    "phosphor":   {"type": "number", "min": -100, "max": 100, "default": 0},
    "colorSmear": {"type": "number", "min": -100, "max": 100, "default": 0},
    "colorSplit": {"type": "number", "min": -100, "max": 100, "default": 0},
    "glitch":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "static":     {"type": "number", "min": -100, "max": 100, "default": 0},
    "saturation": {"type": "number", "min": -100, "max": 100, "default": 0},
    "contrast":   {"type": "number", "min": -100, "max": 100, "default": 0},
    "vignette":   {"type": "number", "min": -100, "max": 100, "default": 0},
    "seed":       {"type": "int",    "min": 0,    "max": 9999, "default": 0}
  },
  "overlap": 128,
  "area": "content"
}"#;

// ---- palettes (0xRRGGBB), up to 16 colours ----

const PAL_NONE: usize = 0;
/// (name index is the position in the manifest's list minus 1) colours, count, dither spread.
const PALETTES: [([u32; 16], usize, f32); 9] = [
    ([0; 16], 0, 0.0),                                                                  // None
    ([0x0f380f, 0x306230, 0x8bac0f, 0x9bbc0f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 4, 0.50), // Game Boy
    ([0x000000, 0xffffff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 2, 1.00),           // 1-Bit
    ([0x000000, 0x555555, 0xaaaaaa, 0xffffff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 4, 0.40), // Grayscale 4
    ([0x000000, 0x55ffff, 0xff55ff, 0xffffff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 4, 0.60), // CGA Cyan Magenta
    ([0x000000, 0x55ff55, 0xff5555, 0xffff55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 4, 0.60), // CGA Red Green
    ([0x000000, 0x1d2b53, 0x7e2553, 0x008751, 0xab5236, 0x5f574f, 0xc2c3c7, 0xfff1e8,
      0xff004d, 0xffa300, 0xffec27, 0x00e436, 0x29adff, 0x83769c, 0xff77a8, 0xffccaa], 16, 0.30), // PICO-8
    ([0x000000, 0x4d2a00, 0xa35c00, 0xffb000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 4, 0.45), // Amber Terminal
    ([0x000000, 0x0a3d0a, 0x1f9a1f, 0x66ff66, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 4, 0.45), // Green Phosphor
];

// ---- the looks ----

/// Sizes are in thousandths of the photo's long side (pixel size) or short side (scanline period), so a look scales with
/// the photo; the rest are slider units.
struct Look {
    name: &'static str,
    pix: f32, palette: usize, dither: f32, scan: f32, line: f32, mask: f32,
    smear: f32, split: f32, glitch: f32, noise: f32, sat: f32, contrast: f32, vig: f32,
}

const NEUTRAL: Look = Look {
    name: "Custom", pix: 0.0, palette: PAL_NONE, dither: 0.0, scan: 0.0, line: 3.5, mask: 0.0,
    smear: 0.0, split: 0.0, glitch: 0.0, noise: 0.0, sat: 0.0, contrast: 0.0, vig: 0.0,
};

/// In the same order as the manifest's options.
const LOOKS: [Look; 9] = [
    // Colour smeared sideways, colour fringes, static, a little glitch, washed out.
    Look { name: "VHS Tape", line: 3.0, scan: 18.0, smear: 55.0, split: 30.0, glitch: 15.0, noise: 22.0, sat: -12.0, contrast: -6.0, vig: 12.0, ..NEUTRAL },
    NEUTRAL,
    // Scanlines, an RGB phosphor mask, punchy colour, a curved-glass vignette.
    Look { name: "CRT Monitor", scan: 45.0, line: 3.5, mask: 35.0, smear: 15.0, split: 10.0, noise: 6.0, sat: 12.0, contrast: 12.0, vig: 35.0, ..NEUTRAL },
    // 160 x 144 in four greens.
    Look { name: "Game Boy", pix: 8.0, palette: 1, dither: 60.0, contrast: 10.0, ..NEUTRAL },
    // Black and white pixels only: the Mac classic.
    Look { name: "1-Bit Dither", pix: 3.0, palette: 2, dither: 100.0, contrast: 18.0, ..NEUTRAL },
    // The cyan, magenta, white and black palette of CGA, on a scanned screen.
    Look { name: "CGA Arcade", pix: 6.0, palette: 4, dither: 70.0, scan: 14.0, line: 3.5, ..NEUTRAL },
    // 16 colours, light dither.
    Look { name: "PICO-8", pix: 6.0, palette: 6, dither: 45.0, ..NEUTRAL },
    // An amber monochrome terminal.
    Look { name: "Amber Terminal", pix: 3.0, palette: 7, dither: 40.0, scan: 40.0, line: 3.5, smear: 10.0, noise: 8.0, vig: 40.0, ..NEUTRAL },
    // Broken colour channels, torn lines, static.
    Look { name: "Glitch", line: 3.0, scan: 10.0, split: 70.0, glitch: 60.0, noise: 10.0, contrast: 8.0, ..NEUTRAL },
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


/// A zeroed block of `n` floats.
fn plane(n: usize) -> Option<&'static mut [f32]> {
    let at = pc_alloc((n * 4) as u32);
    if at == 0 {
        return None;
    }
    // SAFETY: `pc_alloc` returned a fresh block of `n * 4` bytes inside our memory, used once.
    Some(unsafe { core::slice::from_raw_parts_mut(at as usize as *mut f32, n) })
}

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

fn round(v: f32) -> f32 {
    floor(v + 0.5)
}

/// Ordered dither threshold 0..63 of the classic 8 x 8 Bayer matrix (built from the 2 x 2 matrix, high bit first).
fn bayer8(x: i32, y: i32) -> f32 {
    const M: [[u32; 2]; 2] = [[0, 2], [3, 1]];
    let mut v = 0u32;
    for bit in [2, 1, 0] {
        v = v * 4 + M[((y >> bit) & 1) as usize][((x >> bit) & 1) as usize];
    }
    v as f32
}

/// # Safety
/// The host guarantees `buf..buf + len` is a block it got from `pc_alloc`, holding `len / 4` samples,
/// and `params..params + params_len` is readable UTF-8 JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pc_filter(buf: *mut f32, len: u32, width: u32, height: u32, channels: u32, format: u32, params: *const u8, params_len: u32) -> i32 {
    let n = channels as usize;
    let mode = (format >> 16) & 0xff;
    // RGB only: the effects are about colour.
    if mode != 3 {
        return 2;
    }
    let alpha = format & HAS_ALPHA != 0;
    let colour = if alpha { n.wrapping_sub(1) } else { n };
    if n == 0 || colour != 3 || (width as usize) * (height as usize) * n * 4 > len as usize {
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
    let palette = match text(p, "palette") {
        Some(b"None") => 0,
        Some(b"Game Boy") => 1,
        Some(b"1-Bit") => 2,
        Some(b"Grayscale 4") => 3,
        Some(b"CGA Cyan Magenta") => 4,
        Some(b"CGA Red Green") => 5,
        Some(b"PICO-8") => 6,
        Some(b"Amber Terminal") => 7,
        Some(b"Green Phosphor") => 8,
        _ => look.palette,
    };
    let adj = |key: &str, base: f32, lo: f32, hi: f32| (base + num(p, key, 0.0)).clamp(lo, hi);
    let strength = num(p, "strength", 100.0).clamp(0.0, 100.0) / 100.0;
    let seed = num(p, "seed", 0.0).clamp(0.0, 9999.0) as u32;
    // The band's place in the document (the buffer's top-left sample) and the canvas size.
    let ox = num(p, "x", 0.0) as i32;
    let oy = num(p, "y", 0.0) as i32;
    let cw = num(p, "canvasWidth", width as f32).max(1.0);
    let ch = num(p, "canvasHeight", height as f32).max(1.0);
    let (long, short) = (cw.max(ch), cw.min(ch));

    let block = (round(adj("pixelSize", look.pix, 0.0, 100.0) * long / 1000.0) as i32).clamp(1, 32);
    let dither = adj("dither", look.dither, 0.0, 100.0) / 100.0;
    let scan = adj("scanlines", look.scan, 0.0, 100.0) / 100.0;
    let period = (round(adj("lineSize", look.line, 1.0, 12.0) * short / 1000.0) as i32).clamp(2, 24);
    let mask = adj("phosphor", look.mask, 0.0, 100.0) / 100.0;
    let smear = round(adj("colorSmear", look.smear, 0.0, 100.0) / 100.0 * 24.0) as i32;
    let split = round(adj("colorSplit", look.split, 0.0, 100.0) / 100.0 * 16.0) as i32;
    let glitch = adj("glitch", look.glitch, 0.0, 100.0) / 100.0;
    let noise = adj("static", look.noise, 0.0, 100.0) / 100.0 * 0.2;
    let sat = 1.0 + adj("saturation", look.sat, -100.0, 100.0) / 100.0;
    let con = 1.0 + adj("contrast", look.contrast, -100.0, 100.0) / 100.0;
    let vig = adj("vignette", look.vig, 0.0, 100.0) / 100.0;
    let (cx, cy) = (cw * 0.5, ch * 0.5);
    let band_h = ((round(short * 0.006)) as i32).max(3);

    // Planar copy of the colour channels, pixelated when asked: blocks are aligned to the canvas, not to the band.
    let np = w * h;
    let mut src: [&mut [f32]; 3] = [&mut [], &mut [], &mut []];
    for c in 0..3 {
        match plane(np) { Some(a) => src[c] = a, None => return 3 }
    }
    if block > 1 {
        let bs = block;
        let (bx0, bx1) = (ox.div_euclid(bs), (ox + w as i32 - 1).div_euclid(bs));
        let (by0, by1) = (oy.div_euclid(bs), (oy + h as i32 - 1).div_euclid(bs));
        for by in by0..=by1 {
            let ya = (by * bs - oy).max(0) as usize;
            let yb = (((by + 1) * bs - oy) as usize).min(h);
            for bx in bx0..=bx1 {
                let xa = (bx * bs - ox).max(0) as usize;
                let xb = (((bx + 1) * bs - ox) as usize).min(w);
                let cnt = ((yb - ya) * (xb - xa)) as f32;
                let mut s = [0.0f32; 3];
                for y in ya..yb {
                    for x in xa..xb {
                        let i = (y * w + x) * n;
                        s[0] += px[i];
                        s[1] += px[i + 1];
                        s[2] += px[i + 2];
                    }
                }
                for y in ya..yb {
                    for x in xa..xb {
                        for c in 0..3 {
                            src[c][y * w + x] = s[c] / cnt;
                        }
                    }
                }
            }
        }
    } else {
        for i in 0..np {
            for c in 0..3 {
                src[c][i] = px[i * n + c];
            }
        }
    }

    // Row buffers: the shifted, split colour, then its I and Q (for the smear).
    let (Some(rr), Some(rg), Some(rb), Some(ri), Some(rq)) = (plane(w), plane(w), plane(w), plane(w), plane(w)) else { return 3 };
    let (pal, pal_n, spread) = {
        let e = &PALETTES[palette.min(8)];
        (e.0, e.1, e.2)
    };
    let ov = (num(p, "overlap", 0.0).max(0.0) as usize).min(w / 2).min(h / 2);
    let (kx0, ky0, kx1, ky1) = (ov, ov, w - ov, h - ov);
    let wm = w as i32 - 1;

    for yy in ky0..ky1 {
        let gy = oy + yy as i32;
        // glitch band: some bands slide sideways
        let mut shift = 0i32;
        if glitch > 0.0 {
            let b = gy.div_euclid(band_h);
            let gate = (hash(b, 0x77, 9, seed) + 1.0) * 0.5;
            if gate < glitch * 0.6 {
                shift = round(hash(b, 0x51, 7, seed) * 48.0 * glitch) as i32;
            }
        }
        for x in 0..w {
            let sx = x as i32 - shift;
            let ir = (sx - split).clamp(0, wm) as usize;
            let ig = sx.clamp(0, wm) as usize;
            let ib = (sx + split).clamp(0, wm) as usize;
            let (r, g, b) = (src[0][yy * w + ir], src[1][yy * w + ig], src[2][yy * w + ib]);
            rr[x] = r;
            rg[x] = g;
            rb[x] = b;
            ri[x] = 0.596 * r - 0.274 * g - 0.322 * b;
            rq[x] = 0.211 * r - 0.523 * g + 0.312 * b;
        }
        // sliding window of the I and Q rows for the colour smear
        let (mut si, mut sq) = (0.0f32, 0.0f32);
        if smear > 0 {
            for k in -smear..=smear {
                let j = (kx0 as i32 + k).clamp(0, wm) as usize;
                si += ri[j];
                sq += rq[j];
            }
        }
        let inv_win = 1.0 / (2 * smear + 1) as f32;
        for x in kx0..kx1 {
            let gx = ox + x as i32;
            let mut c = [rr[x], rg[x], rb[x]];
            if smear > 0 {
                let yv = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                let (iv, qv) = (si * inv_win, sq * inv_win);
                c = [yv + 0.956 * iv + 0.621 * qv, yv - 0.272 * iv - 0.647 * qv, yv - 1.106 * iv + 1.703 * qv];
                let add = (x as i32 + smear + 1).clamp(0, wm) as usize;
                let sub = (x as i32 - smear).clamp(0, wm) as usize;
                si += ri[add] - ri[sub];
                sq += rq[add] - rq[sub];
            }
            // saturation, contrast
            let l = luma(c[0], c[1], c[2]);
            for q in 0..3 {
                c[q] = ((l + (c[q] - l) * sat) - 0.5) * con + 0.5;
            }
            // static: one value per pixel block, the same on every channel
            let (cxb, cyb) = (gx.div_euclid(block), gy.div_euclid(block));
            if noise > 0.0 {
                let u = hash(cxb, cyb, 0, seed.wrapping_add(101)) * noise;
                for q in 0..3 {
                    c[q] += u;
                }
            }
            for q in 0..3 {
                c[q] = c[q].clamp(0.0, 1.0);
            }
            // palette with ordered dither
            if pal_n > 0 {
                let bias = (bayer8(cxb, cyb) / 64.0 - 0.492_187_5) * spread * dither;
                let t = [c[0] + bias, c[1] + bias, c[2] + bias];
                let mut best = 0usize;
                let mut bd = f32::MAX;
                for k in 0..pal_n {
                    let e = pal[k];
                    let (pr, pg, pb) = (((e >> 16) & 255) as f32 / 255.0, ((e >> 8) & 255) as f32 / 255.0, (e & 255) as f32 / 255.0);
                    let d = (t[0] - pr) * (t[0] - pr) * 0.30 + (t[1] - pg) * (t[1] - pg) * 0.59 + (t[2] - pb) * (t[2] - pb) * 0.11;
                    if d < bd {
                        bd = d;
                        best = k;
                    }
                }
                let e = pal[best];
                c = [((e >> 16) & 255) as f32 / 255.0, ((e >> 8) & 255) as f32 / 255.0, (e & 255) as f32 / 255.0];
            }
            // scanlines and phosphor mask
            if scan > 0.0 {
                let t = (gy.rem_euclid(period)) as f32 / period as f32;
                let d = (2.0 * t - 1.0).abs();
                let f = 1.0 - scan * d * d;
                for q in 0..3 {
                    c[q] *= f;
                }
            }
            if mask > 0.0 {
                let col = gx.rem_euclid(3) as usize;
                for q in 0..3 {
                    let e = if q == col { 1.0 } else { 0.35 };
                    c[q] *= 1.0 - mask * (1.0 - e);
                }
            }
            // vignette
            if vig > 0.0 {
                let (dx, dy) = (gx as f32 - cx, gy as f32 - cy);
                let ex = dx / cx;
                let ey = dy / cy;
                let r = sqrt(ex * ex + ey * ey) * 0.707_107;
                let f = 1.0 - vig * smooth(0.45, 1.05, r);
                for q in 0..3 {
                    c[q] *= f;
                }
            }
            let i = (yy * w + x) * n;
            if alpha && px[i + n - 1] <= 0.0 {
                continue; // fully transparent: leave it
            }
            for q in 0..3 {
                let o = px[i + q];
                px[i + q] = o + (c[q].clamp(0.0, 1.0) - o) * strength;
            }
        }
    }
    0
}
