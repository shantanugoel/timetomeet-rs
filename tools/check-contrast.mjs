#!/usr/bin/env node
/* Contrast is measured, never asserted.
 *
 * Every claim in this file resolves the real colours the browser would paint, including
 * `color-mix(in oklab, ...)` and per-theme tokens. It exists because hand-written CSS
 * accumulates pairs nobody re-checks: `.palette li .z` shipped at 3.79:1 against the
 * highlighted row's background, and nothing in the suite could have noticed - the palette
 * is closed until a person opens it.
 *
 * The converter carries its own tests. A checker that cannot fail is decoration, so
 * oklch->sRGB and the ratio math are verified against values that are not in dispute
 * (black on white = 21, #777 on white = 4.48) before any design token is trusted.
 *
 * Usage: node tools/check-contrast.mjs [--verbose]
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const CSS = join(ROOT, 'app', 'style', 'app.css');

/* ------------------------------------------------------------------ colour maths */

const clamp01 = (x) => (x < 0 ? 0 : x > 1 ? 1 : x);
const toSrgb = (u) => {
  u = clamp01(u);
  return Math.round(255 * (u <= 0.0031308 ? 12.92 * u : 1.055 * Math.pow(u, 1 / 2.4) - 0.055));
};
const toLinear = (c) => {
  c /= 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
};

function oklchToRgb(L, C, H) {
  const h = (H * Math.PI) / 180;
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const [l, m, s] = [l_ ** 3, m_ ** 3, s_ ** 3];
  return [
    toSrgb(4.0767416621 * l - 3.2386544606 * m + 0.1968663018 * s),
    toSrgb(-1.2684380046 * l + 2.6097574011 * m - 0.341305397 * s),
    toSrgb(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
  ];
}

function rgbToOklab([r, g, b]) {
  const [R, G, B] = [toLinear(r), toLinear(g), toLinear(b)];
  const l = Math.cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B);
  const m = Math.cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B);
  const s = Math.cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

function oklabToRgb([L, a, b]) {
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const [l, m, s] = [l_ ** 3, m_ ** 3, s_ ** 3];
  return [
    toSrgb(4.0767416621 * l - 3.2386544606 * m + 0.1968663018 * s),
    toSrgb(-1.2684380046 * l + 2.6097574011 * m - 0.341305397 * s),
    toSrgb(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
  ];
}

const luminance = ([r, g, b]) => 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);

function ratio(fg, bg) {
  const [a, b] = [luminance(fg), luminance(bg)].sort((x, y) => y - x);
  return (a + 0.05) / (b + 0.05);
}

// Over a solid background, because a translucent foreground (or shadow tint) is not the
// colour that reaches an eye.
function over(fg, alpha, bg) {
  return fg.map((c, i) => Math.round(alpha * c + (1 - alpha) * bg[i]));
}

/* ------------------------------------------------------------------ css tokens */

function extractBlock(css, selector) {
  const at = css.indexOf(selector);
  if (at < 0) throw new Error(`no block for ${selector}`);
  const open = css.indexOf('{', at);
  let depth = 0;
  for (let i = open; i < css.length; i++) {
    if (css[i] === '{') depth++;
    else if (css[i] === '}') {
      depth--;
      if (!depth) return css.slice(open + 1, i);
    }
  }
  throw new Error(`unbalanced block for ${selector}`);
}

function tokens(block) {
  // Strip comments first: a note that mentions `--some-token: value` would otherwise be
  // read as a declaration, and the design system is annotated heavily on purpose.
  const map = {};
  const body = block.replace(/\/\*[\s\S]*?\*\//g, '');
  for (const m of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) map[m[1]] = m[2].trim();
  return map;
}

const NAMED = { white: [255, 255, 255], black: [0, 0, 0], transparent: [0, 0, 0] };

function parse(expr, toks, theme) {
  expr = String(expr).trim();
  let m;
  if ((m = expr.match(/^var\(\s*(--[\w-]+)\s*\)$/))) {
    const v = toks[m[1]];
    if (v === undefined) throw new Error(`theme ${theme}: unknown token ${m[1]}`);
    return parse(v, toks, theme);
  }
  if ((m = expr.match(/^oklch\(\s*([\d.]+)%\s+([\d.-]+)\s+([\d.-]+)\s*\)$/))) {
    return { rgb: oklchToRgb(+m[1] / 100, +m[2], +m[3]), a: 1 };
  }
  if ((m = expr.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+))?\s*\)$/))) {
    return { rgb: [+m[1], +m[2], +m[3]], a: m[4] === undefined ? 1 : +m[4] };
  }
  if ((m = expr.match(/^#([0-9a-f]{6})$/i))) {
    const n = parseInt(m[1], 16);
    return { rgb: [(n >> 16) & 255, (n >> 8) & 255, n & 255], a: 1 };
  }
  if ((m = expr.match(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i))) {
    return { rgb: [1, 2, 3].map((i) => parseInt(m[i], 16) * 17), a: 1 };
  }
  if (NAMED[expr]) return { rgb: NAMED[expr], a: expr === 'transparent' ? 0 : 1 };
  if ((m = expr.match(/^color-mix\(\s*in\s+oklab\s*,\s*(.+?)\s*\)$/i))) {
    // color-mix(in oklab, A 60%, B 20%) - weights are relative, per the spec.
    const parts = splitTop(m[1]);
    if (parts.length !== 2) throw new Error(`color-mix needs exactly 2 colors: ${expr}`);
    const read = (p) => {
      const w = p.match(/([\d.]+)%\s*$/);
      return { c: parse(p.replace(/[\d.]+\s*%$/, '').trim(), toks, theme), w: w ? +w[1] : 1 };
    };
    const A = read(parts[0]);
    const B = read(parts[1]);
    const tot = A.w + B.w;
    const [a1, a2, a3] = rgbToOklab(A.c.rgb);
    const [b1, b2, b3] = rgbToOklab(B.c.rgb);
    const mix = oklabToRgb([
      (a1 * A.w + b1 * B.w) / tot,
      (a2 * A.w + b2 * B.w) / tot,
      (a3 * A.w + b3 * B.w) / tot,
    ]);
    return { rgb: mix, a: (A.c.a * A.w + B.c.a * B.w) / tot };
  }
  throw new Error(`cannot parse colour: ${expr}`);
}

function splitTop(s) {
  const out = [];
  let depth = 0;
  let cur = '';
  for (const ch of s) {
    if (ch === '(') depth++;
    if (ch === ')') depth--;
    if (ch === ',' && !depth) {
      out.push(cur.trim());
      cur = '';
    } else cur += ch;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}

/* ------------------------------------------------------------------ self-tests */

function selfTest() {
  const errs = [];
  const near = (a, b, tol, what) => {
    if (!(Math.abs(a - b) <= tol)) errs.push(`${what}: got ${a}, expected ${b} (\u00b1${tol})`);
  };
  const white = [255, 255, 255];
  const black = [0, 0, 0];
  near(ratio(black, white), 21, 0.001, 'black on white');
  near(ratio([0x77, 0x77, 0x77], white), 4.48, 0.02, '#777 on white');
  near(ratio(white, white), 1, 0.001, 'white on white');
  near(ratio([0x33, 0x33, 0x33], [0x22, 0x22, 0x22]), 1.0, 0.3, 'two dark greys are nearly equal');

  // Anchors, not intuitions. Oklab L is a cube root of a cone-response mix and is NOT CIE
  // Y, so oklch(50% 0 0) is ~#656565 rather than the #7f7f7f an untested guess produces.
  // These published values came with Oklab; if the matrices drift, this fails.
  near(oklchToRgb(1, 0, 0)[0], 255, 0, 'oklch(100% 0 0) is white');
  near(oklchToRgb(0, 0, 0)[0], 0, 0, 'oklch(0% 0 0) is black');
  near(oklchToRgb(0.5, 0, 0)[0], 101, 2, 'oklch(50% 0 0) grey channel');
  const red = rgbToOklab([255, 0, 0]);
  near(red[0], 0.628, 0.002, '#f00 oklab L');
  near(red[1], 0.2249, 0.002, '#f00 oklab a');
  near(red[2], 0.1258, 0.002, '#f00 oklab b');
  const blue = rgbToOklab([0, 0, 255]);
  near(blue[0], 0.452, 0.002, '#00f oklab L');
  near(blue[2], -0.3115, 0.002, '#00f oklab b');

  // Perceptual, not arithmetic: a 50/50 oklab mix of black and white lands on ~#656565.
  // Naive sRGB blending would give ~176 and gamma-aware blending ~187, so this fails if
  // someone "simplifies" the interpolation space - every mixed colour would silently move.
  const mixed = parse('color-mix(in oklab, #000000 50%, #ffffff 50%)', {}, 'dark');
  near(mixed.rgb[0], 101, 3, 'oklab midpoint of black/white');
  // Weighted mixes must actually be weighted, not averaged.
  const skewed = parse('color-mix(in oklab, #000000 25%, #ffffff 75%)', {}, 'dark');
  if (!(skewed.rgb[0] > mixed.rgb[0])) errs.push('color-mix weights are ignored');

  // Alpha has to composite over the background it will be painted on.
  const translucent = over([255, 255, 255], 0.32, [0, 0, 0]);
  near(translucent[0], 82, 1, 'rgba(255,255,255,.32) over black');
  return errs;
}

/* ------------------------------------------------------------------ cases */
/*
 * Each case is a foreground painted on a specific background, as the stylesheet really
 * does it. `min` is the WCAG threshold: 4.5 for text under 18.66px/24px (or 14px bold),
 * 3.0 for anything larger, and 3.0 for the non-text things that carry meaning.
 *
 * Add a case whenever you add a text/background pairing. The coverage guard below fails
 * the build if a rule paints a colour token that no case mentions, so the table cannot
 * silently fall behind the stylesheet.
 */
const CASES = [
  // Surfaces and body copy.
  { g: 'body', fg: 'var(--text)', bg: 'var(--bg)', min: 4.5 },
  { g: 'body', fg: 'var(--muted)', bg: 'var(--bg)', min: 4.5 },
  { g: 'body', fg: 'var(--faint)', bg: 'var(--bg)', min: 4.5 },
  { g: 'body', fg: 'var(--text)', bg: 'var(--surface)', min: 4.5 },
  { g: 'body', fg: 'var(--muted)', bg: 'var(--surface)', min: 4.5 },
  { g: 'body', fg: 'var(--faint)', bg: 'var(--surface)', min: 4.5 },
  { g: 'body', fg: 'var(--text)', bg: 'var(--surface-2)', min: 4.5 },
  { g: 'body', fg: 'var(--muted)', bg: 'var(--surface-2)', min: 4.5 },
  { g: 'body', fg: 'var(--faint)', bg: 'var(--surface-2)', min: 4.5 },
  { g: 'body', fg: 'var(--select)', bg: 'var(--bg)', min: 4.5, note: 'links' },
  { g: 'body', fg: 'var(--on-primary)', bg: 'var(--select)', min: 4.5, note: 'primary buttons' },
  // A hardcoded #fff on a tokenised background is the pairing most likely to be forgotten
  // when the token moves, so it is measured like any other.
  { g: 'body', fg: 'var(--on-danger)', bg: 'var(--danger)', min: 4.5, note: '.chip .x delete' },

  // The quality ramp. Labels never sit on a fill - the band is a gradient swatch and the
  // hour numbers live in the gutter - so the fills are judged against the row behind them
  // and against each other: that ordering is what survives greyscale.
  { g: 'bands', fg: 'var(--working-text)', bg: 'var(--surface)', min: 4.5, note: '.c-ok' },
  { g: 'bands', fg: 'var(--shoulder-text)', bg: 'var(--surface)', min: 4.5, note: '.c-tight' },
  { g: 'bands', fg: 'var(--off-text)', bg: 'var(--surface)', min: 4.5, note: '.c-hard' },
  { g: 'bands', fg: 'var(--asleep-text)', bg: 'var(--surface)', min: 4.5, note: '.c-bad' },
  { g: 'bands', fg: 'var(--working-text)', bg: 'var(--surface-2)', min: 4.5 },
  { g: 'bands', fg: 'var(--shoulder-text)', bg: 'var(--surface-2)', min: 4.5 },
  { g: 'bands', fg: 'var(--off-text)', bg: 'var(--surface-2)', min: 4.5 },
  { g: 'bands', fg: 'var(--asleep-text)', bg: 'var(--surface-2)', min: 4.5 },
  { g: 'bands', fg: 'var(--danger)', bg: 'var(--surface)', min: 4.5 },
  { g: 'bands', fg: 'var(--muted)', bg: 'var(--bg)', min: 4.5, note: 'hour labels' },
  // Adjacent bands must separate without colour, so luminance has to carry the step.
  { g: 'bands', fg: 'var(--offhours)', bg: 'var(--asleep)', min: 1.22, note: 'ramp step' },
  { g: 'bands', fg: 'var(--shoulder)', bg: 'var(--offhours)', min: 1.22, note: 'ramp step' },
  { g: 'bands', fg: 'var(--working)', bg: 'var(--shoulder)', min: 1.22, note: 'ramp step' },

  // Palette, including the highlighted option - the pair that shipped broken.
  { g: 'palette', fg: 'var(--text)', bg: 'var(--surface-2)', min: 4.5, note: 'city name' },
  { g: 'palette', fg: 'var(--text)', bg: 'var(--active)', min: 4.5, note: 'active row' },
  { g: 'palette', fg: 'var(--muted)', bg: 'var(--active)', min: 4.5, note: 'country on active row' },
  { g: 'palette', fg: 'var(--faint)', bg: 'var(--surface-2)', min: 4.5, note: 'zone on a resting row' },
  { g: 'palette', fg: 'var(--muted)', bg: 'var(--surface-2)', min: 4.5, note: 'country' },

  // Slots list and score badge.
  { g: 'slots', fg: 'var(--text)', bg: 'var(--surface)', min: 4.5 },
  { g: 'slots', fg: 'var(--muted)', bg: 'var(--surface)', min: 4.5 },
  { g: 'slots', fg: 'var(--on-primary)', bg: 'var(--select)', min: 4.5, note: 'selected slot' },
  { g: 'slots', fg: 'var(--muted)', bg: 'var(--surface)', min: 4.5, note: 'score' },
  { g: 'slots', fg: 'var(--text)', bg: 'var(--active)', min: 4.5, note: 'selected slot' },
  { g: 'slots', fg: 'var(--muted)', bg: 'var(--active)', min: 4.5, note: 'score on selected slot' },
];

const css = readFileSync(CSS, 'utf8');

// A theme expressed twice drifts; see tools/gen-auto-theme.mjs. Custom properties inside a
// @media block mean someone is maintaining a second copy of a palette by hand.
if (/prefers-color-scheme[^)]*\)\s*\{[^}]*--[-\w]+\s*:/.test(css)) {
  console.error('contrast: custom properties inside @media (prefers-color-scheme) - the automatic theme must be generated, not hand-copied');
  process.exit(1);
}
const dark = tokens(extractBlock(css, ':root {'));
const light = { ...dark, ...tokens(extractBlock(css, ':root[data-theme="light"] {')) };
const THEMES = { dark, light };

const bad = [];
for (const [theme, toks] of Object.entries(THEMES)) {
  for (const c of CASES) {
    const f = parse(c.fg, toks, theme);
    const b = parse(c.bg, toks, theme);
    const rgb = f.a < 1 ? over(f.rgb, f.a, b.rgb) : f.rgb;
    const r = ratio(rgb, b.rgb);
    if (r < c.min) bad.push(`[${theme}] ${c.g}: ${c.fg} on ${c.bg} = ${r.toFixed(2)}:1, need ${c.min} (${c.note || ''})`);
    if (process.argv.includes('--verbose')) console.log(`  ${theme.padEnd(5)} ${String(r.toFixed(2)).padStart(5)}:1  ${c.fg} on ${c.bg}  (${c.note || c.g})`);
  }
}

/* Coverage: every colour token the stylesheet uses for text must appear in a case, so a
 * new --something can't arrive unmeasured. */
const used = new Set([...css.matchAll(/(?<![\w-])color\s*:\s*var\(\s*(--[\w-]+)/g)].map((m) => m[1]));
const covered = new Set();
for (const c of CASES) for (const m of String(c.fg).matchAll(/var\(\s*(--[\w-]+)/g)) covered.add(m[1]);
for (const t of used) if (!covered.has(t)) bad.push(`token ${t} is used for text but no contrast case covers it`);

const st = selfTest();
if (st.length) {
  console.error('contrast checker is itself wrong:\n  ' + st.join('\n  '));
  process.exit(1);
}
if (bad.length) {
  console.error(`contrast: ${bad.length} problem(s)\n  ` + bad.join('\n  '));
  process.exit(1);
}
console.log(`contrast ok · ${CASES.length * 2} pairs across 2 themes, ${used.size} text tokens covered`);
