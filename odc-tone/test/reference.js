// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Open Designer Cloud. Licensed under the Apache License, Version 2.0 (see LICENSE).
// Reference for the tests: the tone and colour grading of HyperImage's Adjust Color tool (as in odc-film-look/test/reference.js),
// followed by Depth, Sharpness (unsharp mask) and Texture copied verbatim from renderer/color.js (BriteWire), on a whole image.
// The grading result is clamped to 0..255 before the spatial passes (HyperImage's 8-bit path; the plug-in does the same).
// usage: node reference.js < in.json > out.json   with in.json = {"w":W,"h":H,"settings":{...},"pixels":[r,g,b,...] (0..255)}
const clampi = (v, a, b) => (v < a ? a : v > b ? b : v);
const NEUTRAL = {
  black: 0, gamma: 100, white: 255,
  exposure: 0, contrast: 0, highlights: 0, shadows: 0,
  vibrance: 0, saturation: 0, temp: 0, tint: 0, sepia: 0, depth: 0, sharp: 0,
  invert: 0, threshold: 0, vignette: 0,
  texture: 0, grain: 0, grainSize: 1, grainRough: 50, grainColor: 0,
  stShadow: '#3a6ea5', stHighlight: '#e8b878', stBalance: 0, stAmount: 0,
  nrColor: 0, nrLuma: 0, nrDetail: 50, despeckle: 0,
};

let S, curve;
function buildCurveLUT(points, Arr = Uint8ClampedArray) {
  const pts = points.slice().sort((a, b) => a.x - b.x);
  const n = pts.length, lut = new Arr(256);
  const xs = pts.map((p) => p.x), ys = pts.map((p) => p.y);
  if (n === 1) { for (let i = 0; i < 256; i++) lut[i] = ys[0]; return lut; }
  const dxs = [], ms = [];
  for (let i = 0; i < n - 1; i++) { const dx = xs[i + 1] - xs[i]; dxs.push(dx); ms.push(dx ? (ys[i + 1] - ys[i]) / dx : 0); }
  const c1 = new Array(n);
  c1[0] = ms[0]; c1[n - 1] = ms[n - 2];
  for (let i = 1; i < n - 1; i++) {
    if (ms[i - 1] * ms[i] <= 0) c1[i] = 0;
    else { const w1 = 2 * dxs[i] + dxs[i - 1], w2 = dxs[i] + 2 * dxs[i - 1]; c1[i] = (w1 + w2) / (w1 / ms[i - 1] + w2 / ms[i]); }
  }
  for (let x = 0; x < 256; x++) {
    if (x <= xs[0]) { lut[x] = ys[0]; continue; }
    if (x >= xs[n - 1]) { lut[x] = ys[n - 1]; continue; }
    let i = 0; while (i < n - 1 && x > xs[i + 1]) i++;
    const h = dxs[i], t = (x - xs[i]) / h, t2 = t * t, t3 = t2 * t;
    const h00 = 2 * t3 - 3 * t2 + 1, h10 = t3 - 2 * t2 + t, h01 = -2 * t3 + 3 * t2, h11 = t3 - t2;
    lut[x] = h00 * ys[i] + h10 * h * c1[i] + h01 * ys[i + 1] + h11 * h * c1[i + 1];
  }
  return lut;
}


function buildLUTs(Arr = Uint8ClampedArray) {
  const black = S.black, white = Math.max(S.black + 1, S.white), invGamma = 1 / (S.gamma / 100);
  const expF = Math.pow(2, S.exposure / 100);
  const conF = 1 + S.contrast / 100;
  const sh = S.shadows / 100, hi = S.highlights / 100;
  const tempA = (S.temp / 100) * 32, tintA = (S.tint / 100) * 32;
  const curveLUT = curve ? buildCurveLUT(curve.map(([x, y]) => ({ x, y })), Arr) : null;
  const r = new Arr(256), g = new Arr(256), b = new Arr(256);
  // Uint8ClampedArray clamps when written; a float table has to be told to.
  const fit = Arr === Uint8ClampedArray ? (v) => v : (v) => clampi(v, 0, 255);
  for (let i = 0; i < 256; i++) {
    let v = (i - black) / (white - black);
    v = v < 0 ? 0 : v > 1 ? 1 : v;
    v = Math.pow(v, invGamma) * 255;
    v *= expF;
    v = (v - 128) * conF + 128;
    const n = v / 255;
    // Lightroom's convention, which is what every slider labelled this way does:
    // positive brightens that end of the range, negative pulls it back. Highlights was
    // subtracted here, so it ran backwards against its own label.
    v += sh * (1 - n) * 64;
    v += hi * n * 64;
    // A preset's tone curve comes after the tone sliders and before the colour split,
    // which is where a camera Picture Control applies its custom curve.
    if (curveLUT) v = curveLUT[v < 0 ? 0 : v > 255 ? 255 : Math.round(v)];
    let rv = v + tempA, gv = v + tintA, bv = v - tempA;
    // Invert last, after levels, exposure, contrast and the temp/tint split, so
    // it inverts the image you can actually see rather than the raw input. The
    // arrays clamp on assignment, which makes the out-of-range cases agree with
    // clamping first and then inverting.
    if (S.invert) { rv = 255 - rv; gv = 255 - gv; bv = 255 - bv; }
    r[i] = fit(rv); g[i] = fit(gv); b[i] = fit(bv);
  }
  return { r, g, b };
}


function run(settings, curveIn, px) {
  S = { ...NEUTRAL, ...settings }; curve = curveIn;
  const d = new Float32Array(px.length / 3 * 4);
  for (let i = 0, j = 0; i < px.length; i += 3, j += 4) { d[j] = px[i]; d[j + 1] = px[i + 1]; d[j + 2] = px[i + 2]; d[j + 3] = 255; }
  const { r: LR, g: LG, b: LB } = buildLUTs(Float32Array);
  const lut = (T, v) => {
    if (v <= 0) return T[0];
    if (v >= 255) return T[255];
    const i0 = v | 0;
    return T[i0] + (T[i0 + 1] - T[i0]) * (v - i0);
  };
  const vib = S.vibrance / 100, sat = 1 + S.saturation / 100, sep = S.sepia / 100;
  // Split tone: one colour into the dark tones, another into the bright ones, with
  // Balance moving where the two meet. Unlike Temp/Tint (which run back in the LUT,
  // before saturation, and so are erased by a desaturation) this lands AFTER saturation,
  // which is what makes a cool black and white or a teal/orange grade possible at all.
  //
  // Each tone is scaled to the pixel's own luminance before it is mixed in, so the
  // colour changes and the brightness does not: tinting toward a mid blue must not
  // lighten the shadows it is tinting.
  const stAmt = S.stAmount / 100;
  const hex = (h) => { const n = parseInt(String(h || '#000000').slice(1), 16) || 0; return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; };
  const [shR, shG, shB] = hex(S.stShadow), [hiR, hiG, hiB] = hex(S.stHighlight);
  const shL = Math.max(0.02, (0.299 * shR + 0.587 * shG + 0.114 * shB) / 255);
  const hiL = Math.max(0.02, (0.299 * hiR + 0.587 * hiG + 0.114 * hiB) / 255);
  const pivot = Math.min(0.95, Math.max(0.05, 0.5 + S.stBalance / 200));
  for (let i = 0; i < d.length; i += 4) {
    let r = lut(LR, d[i]), g = lut(LG, d[i + 1]), b = lut(LB, d[i + 2]);
    if (vib !== 0) {
      // Vibrance: boost scaled by how muted the pixel already is, so saturated
      // colors barely move; skin-hued pixels (r > g > b) get half the boost.
      const mx = Math.max(r, g, b), mn = Math.min(r, g, b);
      let amt = vib * (1 - (mx - mn) / 255);
      if (r > g && g > b) amt *= 0.5;
      const l = 0.299 * r + 0.587 * g + 0.114 * b;
      r = l + (r - l) * (1 + amt); g = l + (g - l) * (1 + amt); b = l + (b - l) * (1 + amt);
    }
    if (sat !== 1) { const l = 0.299 * r + 0.587 * g + 0.114 * b; r = l + (r - l) * sat; g = l + (g - l) * sat; b = l + (b - l) * sat; }
    if (sep > 0) {
      const sr = r * 0.393 + g * 0.769 + b * 0.189, sg = r * 0.349 + g * 0.686 + b * 0.168, sb = r * 0.272 + g * 0.534 + b * 0.131;
      r += (sr - r) * sep; g += (sg - g) * sep; b += (sb - b) * sep;
    }
    if (stAmt > 0) {
      const l = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
      // Smoothstep both ramps, or the crossover shows up as a band across a gradient.
      let w = l < pivot ? (pivot - l) / pivot : 0;
      if (w > 0) {
        const k = w * w * (3 - 2 * w) * stAmt, s = l / shL;
        r += (shR * s - r) * k; g += (shG * s - g) * k; b += (shB * s - b) * k;
      }
      w = l > pivot ? (l - pivot) / (1 - pivot) : 0;
      if (w > 0) {
        const k = w * w * (3 - 2 * w) * stAmt, s = l / hiL;
        r += (hiR * s - r) * k; g += (hiG * s - g) * k; b += (hiB * s - b) * k;
      }
    }
    d[i] = r; d[i + 1] = g; d[i + 2] = b;
  }

  return d;
}

// Separable box blur of one RGBA channel (for unsharp sharpening).
function boxBlurChannel(src, w, h, ch, r) {
  const n = w * h, tmp = new Float32Array(n), out = new Float32Array(n), win = 2 * r + 1;
  for (let y = 0; y < h; y++) { const row = y * w; let s = 0;
    for (let i = -r; i <= r; i++) s += src[(row + clampi(i, 0, w - 1)) * 4 + ch];
    for (let x = 0; x < w; x++) { tmp[row + x] = s / win; s += src[(row + clampi(x + r + 1, 0, w - 1)) * 4 + ch] - src[(row + clampi(x - r, 0, w - 1)) * 4 + ch]; } }
  for (let x = 0; x < w; x++) { let s = 0;
    for (let i = -r; i <= r; i++) s += tmp[clampi(i, 0, h - 1) * w + x];
    for (let y = 0; y < h; y++) { out[y * w + x] = s / win; s += tmp[clampi(y + r + 1, 0, h - 1) * w + x] - tmp[clampi(y - r, 0, h - 1) * w + x]; } }
  return out;
}

// Soft-light helper (W3C / Photoshop formulation), operating on 0..1 values.
const softG = (x) => (x <= 0.25 ? ((16 * x - 12) * x + 4) * x : Math.sqrt(x));
const softLight = (a, b) => (b <= 0.5
  ? a - (1 - 2 * b) * a * (1 - a)
  : a + (2 * b - 1) * (softG(a) - a));

// Local contrast ("Depth") — soft-light blend against a heavily blurred copy of
// the image. Unlike contrast/vibrance/saturation, which are point operations, this
// is spatial: it lifts regions that are broadly bright and deepens regions that are
// broadly dark, which is what reads as dimensional lighting rather than just louder
// colour. Radius scales with image size so a thumbnail and a 4K export look alike.
function applyDepth(d, w, h, amount) {
  const amt = amount / 100;
  const rad = Math.max(2, Math.round(Math.max(w, h) * 0.02));
  const n = w * h;
  for (let ch = 0; ch < 3; ch++) {
    const blur = boxBlurChannel(d, w, h, ch, rad);
    for (let p = 0, j = ch; p < n; p++, j += 4) {
      const a = d[j] / 255, b = blur[p] / 255;
      d[j] = clampi((a * (1 - amt) + softLight(a, b) * amt) * 255, 0, 255);
    }
  }
}

function lumaPlane(d, n) {
  const l = new Float32Array(n);
  for (let p = 0, i = 0; p < n; p++, i += 4) l[p] = 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
  return l;
}
function boxBlurPlane(src, w, h, r) {
  const win = 2 * r + 1, tmp = new Float32Array(w * h), out = new Float32Array(w * h);
  for (let y = 0; y < h; y++) {
    const row = y * w; let s = 0;
    for (let i = -r; i <= r; i++) s += src[row + clampi(i, 0, w - 1)];
    for (let x = 0; x < w; x++) {
      tmp[row + x] = s / win;
      s += src[row + clampi(x + r + 1, 0, w - 1)] - src[row + clampi(x - r, 0, w - 1)];
    }
  }
  for (let x = 0; x < w; x++) {
    let s = 0;
    for (let i = -r; i <= r; i++) s += tmp[clampi(i, 0, h - 1) * w + x];
    for (let y = 0; y < h; y++) {
      out[y * w + x] = s / win;
      s += tmp[clampi(y + r + 1, 0, h - 1) * w + x] - tmp[clampi(y - r, 0, h - 1) * w + x];
    }
  }
  return out;
}

// Texture: a band-pass on luminance (fine blur minus coarse blur) added back to
// every channel. That is medium-scale detail only, so it restores the surface
// character AI images smooth away without sharpening edges the way Sharpness
// does. Negative values take the band out and smooth instead.
function applyTexture(d, w, h, amt) {
  const n = w * h, L = lumaPlane(d, n);
  // Radii scale with the image so a thumbnail and a 4K export read alike.
  const base = Math.max(w, h) / 1000;
  const r1 = Math.max(1, Math.round(1.5 * base)), r2 = Math.max(r1 + 1, Math.round(6 * base));
  const f = boxBlurPlane(L, w, h, r1), c = boxBlurPlane(L, w, h, r2);
  const k = amt > 0 ? amt * 1.6 : amt;   // smoothing past a full band would invert detail
  for (let p = 0, i = 0; p < n; p++, i += 4) {
    const band = (f[p] - c[p]) * k;
    d[i] += band; d[i + 1] += band; d[i + 2] += band;   // Uint8ClampedArray clamps
  }
}

function tone(settings, w, h, px) {
  const d = run(settings, null, px);               // grading: RGBA float buffer
  for (let i = 0; i < d.length; i++) d[i] = clampi(d[i], 0, 255);
  if (S.depth > 0) applyDepth(d, w, h, S.depth);
  if (S.sharp > 0) {
    const amt = (S.sharp / 100) * 1.5, rad = 1;
    for (let ch = 0; ch < 3; ch++) {
      const blur = boxBlurChannel(d, w, h, ch, rad);
      for (let p = 0, j = ch; p < w * h; p++, j += 4) d[j] = clampi(d[j] + amt * (d[j] - blur[p]), 0, 255);
    }
  }
  if (S.texture !== 0) {
    applyTexture(d, w, h, S.texture / 100);
    for (let i = 0; i < d.length; i++) d[i] = clampi(d[i], 0, 255);   // Uint8ClampedArray in the app
  }
  const out = [];
  for (let j = 0; j < d.length; j += 4) out.push(d[j], d[j + 1], d[j + 2]);
  return out;
}
let buf = '';
process.stdin.on('data', (x) => (buf += x));
process.stdin.on('end', () => { const q = JSON.parse(buf); process.stdout.write(JSON.stringify(tone(q.settings, q.w, q.h, q.pixels))); });
