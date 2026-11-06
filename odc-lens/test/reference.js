// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
// Reference for the tests: the blur, geometry, softness, bloom and vignette passes of HyperImage's Lens tool,
// copied verbatim (BriteWire, renderer/lens.js lines 50-246), wrapped so they run on a list of pixels.
// usage: node reference.js < in.json > out.json
//   in.json = {"w":W,"h":H,"settings":{...lens.js S...},"order":"hyper"|"parallel","pixels":[r,g,b,...] (0..255)}
// order "hyper" is HyperImage's own (bloom is taken from the softened image); "parallel" is what the plug-in does
// (bloom is taken from the geometry result, then added to the softened image) so the total reach stays within 256 px.
const clampi = (v, a, b) => (v < a ? a : v > b ? b : v);
let S;
// ── Blur ──────────────────────────────────────────────────────────────────────
// Separable box blur, three passes, which converges on a Gaussian closely enough that
// the difference is invisible and costs a fraction of a real one. Operates on a
// Float32Array of w*h*3 so repeated passes do not quantise.
function boxBlur3(src, w, h, r) {
  if (r < 1) return src.slice();
  let cur = src;
  for (let pass = 0; pass < 3; pass++) cur = boxBlurOnce(cur, w, h, r);
  return cur;
}

function boxBlurOnce(src, w, h, r) {
  const n = w * h, tmp = new Float32Array(n * 3), out = new Float32Array(n * 3);
  const win = 2 * r + 1;
  for (let y = 0; y < h; y++) {
    const row = y * w;
    let s0 = 0, s1 = 0, s2 = 0;
    for (let i = -r; i <= r; i++) {
      const o = (row + clampi(i, 0, w - 1)) * 3;
      s0 += src[o]; s1 += src[o + 1]; s2 += src[o + 2];
    }
    for (let x = 0; x < w; x++) {
      const o = (row + x) * 3;
      tmp[o] = s0 / win; tmp[o + 1] = s1 / win; tmp[o + 2] = s2 / win;
      const add = (row + clampi(x + r + 1, 0, w - 1)) * 3;
      const sub = (row + clampi(x - r, 0, w - 1)) * 3;
      s0 += src[add] - src[sub]; s1 += src[add + 1] - src[sub + 1]; s2 += src[add + 2] - src[sub + 2];
    }
  }
  for (let x = 0; x < w; x++) {
    let s0 = 0, s1 = 0, s2 = 0;
    for (let i = -r; i <= r; i++) {
      const o = (clampi(i, 0, h - 1) * w + x) * 3;
      s0 += tmp[o]; s1 += tmp[o + 1]; s2 += tmp[o + 2];
    }
    for (let y = 0; y < h; y++) {
      const o = (y * w + x) * 3;
      out[o] = s0 / win; out[o + 1] = s1 / win; out[o + 2] = s2 / win;
      const add = (clampi(y + r + 1, 0, h - 1) * w + x) * 3;
      const sub = (clampi(y - r, 0, h - 1) * w + x) * 3;
      s0 += tmp[add] - tmp[sub]; s1 += tmp[add + 1] - tmp[sub + 1]; s2 += tmp[add + 2] - tmp[sub + 2];
    }
  }
  return out;
}

// ── Geometry pass: distortion + lateral CA ────────────────────────────────────
// Both are radial remaps, so they run together: one bilinear read per channel instead
// of three full passes. Distortion uses the standard r' = r(1 + k r²) about the centre,
// normalised so the corner stays at radius 1 and the strength means the same thing
// whatever the aspect ratio.
//
// Lateral CA scales red out and blue in (or the reverse), which is what a real lens
// does when the three colours focus at different magnifications: fringes that are
// invisible at the centre and grow toward the corners, exactly as they do in glass.
function geometryPass(src, w, h) {
  const k = S.distortion / 100 * 0.35;
  const ca = S.caLateral / 100 * 0.012;
  if (k === 0 && ca === 0) return src.slice();

  const out = new Float32Array(w * h * 3);
  const cx = (w - 1) / 2, cy = (h - 1) / 2;
  const norm = Math.hypot(cx, cy);          // corner distance, so r runs 0..1

  // Bilinear read of one channel, edge-clamped. Sampling past the frame repeats the
  // border rather than going black, or barrel distortion would draw a dark rim.
  const sample = (x, y, ch) => {
    const x0 = Math.floor(x), y0 = Math.floor(y);
    const fx = x - x0, fy = y - y0;
    const xa = clampi(x0, 0, w - 1), xb = clampi(x0 + 1, 0, w - 1);
    const ya = clampi(y0, 0, h - 1), yb = clampi(y0 + 1, 0, h - 1);
    const p00 = src[(ya * w + xa) * 3 + ch], p10 = src[(ya * w + xb) * 3 + ch];
    const p01 = src[(yb * w + xa) * 3 + ch], p11 = src[(yb * w + xb) * 3 + ch];
    return p00 + (p10 - p00) * fx + (p01 - p00) * fy + (p00 - p10 - p01 + p11) * fx * fy;
  };

  // Each channel gets its own magnification. Green is the reference, which is how a
  // lens is focused and why green fringes are the ones you rarely see.
  const mag = [1 + ca, 1, 1 - ca];
  for (let y = 0; y < h; y++) {
    const dy = y - cy;
    for (let x = 0; x < w; x++) {
      const dx = x - cx;
      const r = Math.hypot(dx, dy) / norm;
      // Positive k is barrel: the image is read from FURTHER out, pushing content
      // outward. Negative pinches it in.
      const d = 1 + k * r * r;
      const o = (y * w + x) * 3;
      for (let ch = 0; ch < 3; ch++) {
        const m = d * mag[ch];
        out[o + ch] = sample(cx + dx * m, cy + dy * m, ch);
      }
    }
  }
  return out;
}

// ── Field softness + longitudinal CA ──────────────────────────────────────────
// A curved focal plane means the corners sit off focus while the centre is sharp. The
// naive version lerps the sharp image against one wide blur, which reads as a fog
// rather than as defocus. Three levels and interpolating between ADJACENT ones gives a
// radius that actually grows, which is what the eye is reading.
//
// Longitudinal CA shifts each channel's position along that same ramp, so red reaches
// full defocus before blue does (or the reverse). That is what colours the edge of a
// defocused highlight, and it is the effect in the title card that makes the glow
// greener than the letters it came from.
function softnessPass(src, w, h) {
  if (S.softness <= 0) return src;
  const maxR = Math.max(1, Math.round((S.softness / 100) * Math.min(w, h) * 0.035));
  const mid = boxBlur3(src, w, h, Math.max(1, Math.round(maxR / 2)));
  const far = boxBlur3(src, w, h, maxR);

  const out = new Float32Array(w * h * 3);
  const cx = (w - 1) / 2, cy = (h - 1) / 2, norm = Math.hypot(cx, cy);
  const start = S.softStart / 100;
  const span = Math.max(0.01, 1 - start);
  const shift = S.caLong / 100 * 0.25;
  const chShift = [shift, 0, -shift];      // green is the reference plane again

  for (let y = 0; y < h; y++) {
    const dy = y - cy;
    for (let x = 0; x < w; x++) {
      const dx = x - cx;
      const r = Math.hypot(dx, dy) / norm;
      const o = (y * w + x) * 3;
      for (let ch = 0; ch < 3; ch++) {
        // 0 at the sharp centre, 1 at full defocus. Smoothstepped so the transition
        // has no visible ring where it begins.
        let t = clampi((r - start) / span + chShift[ch], 0, 1);
        t = t * t * (3 - 2 * t);
        const a = t * 2;                    // which pair of levels, and how far between
        out[o + ch] = a <= 1
          ? src[o + ch] + (mid[o + ch] - src[o + ch]) * a
          : mid[o + ch] + (far[o + ch] - mid[o + ch]) * (a - 1);
      }
    }
  }
  return out;
}

// ── Bloom ─────────────────────────────────────────────────────────────────────
// Light scattering inside the glass: the brightest parts of the frame spill into what
// surrounds them. Only what is ABOVE the threshold blooms, and it is added rather than
// mixed, because scattered light is extra light and mixing would darken the source it
// came from.
function bloomPass(src, w, h) {
  if (S.bloom <= 0) return src;
  const thr = (S.bloomThreshold / 100) * 255;
  const n = w * h;
  const bright = new Float32Array(n * 3);
  for (let i = 0; i < n * 3; i += 3) {
    // Thresholded on luminance, so a saturated red does not bloom as if it were white.
    const l = 0.299 * src[i] + 0.587 * src[i + 1] + 0.114 * src[i + 2];
    if (l <= thr) continue;
    const k = (l - thr) / Math.max(1, 255 - thr);
    bright[i] = src[i] * k; bright[i + 1] = src[i + 1] * k; bright[i + 2] = src[i + 2] * k;
  }
  const r = Math.max(1, Math.round(S.bloomRadius * Math.min(w, h) / 1000));
  const glow = boxBlur3(bright, w, h, r);

  const t = parseInt(S.bloomTint.slice(1), 16);
  const tint = [((t >> 16) & 255) / 255, ((t >> 8) & 255) / 255, (t & 255) / 255];
  const amt = S.bloom / 100 * 2.2;
  const out = new Float32Array(n * 3);
  for (let i = 0; i < n * 3; i += 3) {
    out[i]     = src[i]     + glow[i]     * tint[0] * amt;
    out[i + 1] = src[i + 1] + glow[i + 1] * tint[1] * amt;
    out[i + 2] = src[i + 2] + glow[i + 2] * tint[2] * amt;
  }
  return out;
}

// ── Vignette ──────────────────────────────────────────────────────────────────
// The simple radial one. HyperImage's Vignette TOOL stays where it is: it has handles,
// two ellipses and a blend mode, and is the right thing when the darkening is a
// compositional choice. This one is here because it is part of what a lens does, and
// because reaching for another tool to finish a lens look breaks the panel in half.
function vignettePass(src, w, h) {
  if (S.vignette <= 0) return src;
  const cx = (w - 1) / 2, cy = (h - 1) / 2, norm = Math.hypot(cx, cy);
  const amt = S.vignette / 100;
  const out = new Float32Array(w * h * 3);
  for (let y = 0; y < h; y++) {
    const dy = y - cy;
    for (let x = 0; x < w; x++) {
      const dx = x - cx;
      const r = Math.hypot(dx, dy) / norm;
      // cos^4, which is the actual falloff of an ideal lens rather than a linear ramp.
      const c = Math.cos(Math.min(Math.PI / 2, r * 0.9));
      const f = 1 - (1 - c * c * c * c) * amt;
      const o = (y * w + x) * 3;
      out[o] = src[o] * f; out[o + 1] = src[o + 1] * f; out[o + 2] = src[o + 2] * f;
    }
  }
  return out;
}

const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
S = input.settings;
const { w, h } = input;
let buf = Float32Array.from(input.pixels);
buf = geometryPass(buf, w, h);
if (input.order === 'parallel') {
  const soft = softnessPass(buf, w, h);
  const bloomed = bloomPass(buf, w, h);
  const out = new Float32Array(buf.length);
  for (let i = 0; i < buf.length; i++) out[i] = soft[i] + (bloomed[i] - buf[i]);
  buf = vignettePass(out, w, h);
} else {
  buf = softnessPass(buf, w, h);
  buf = bloomPass(buf, w, h);
  buf = vignettePass(buf, w, h);
}
process.stdout.write(JSON.stringify(Array.from(buf)));
