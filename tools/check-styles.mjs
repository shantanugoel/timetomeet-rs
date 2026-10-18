// Fail the build when the JS references a class or CSS custom property the stylesheet
// does not define.
//
// This is not pedantry: a band gradient with one undefined var() invalidates the whole
// background-image and renders a blank row, and an undefined class silently renders an
// unstyled panel. Both happened during development and neither produced a console error,
// so the only thing that would have caught them is this check.
import { readFileSync } from 'node:fs';

const SOURCES = ['app/js/app.js', 'app/js/engine.js', 'app/index.html', 'app/404.html'];
const css = readFileSync('app/style/app.css', 'utf8');
const js = SOURCES.map((f) => [f, readFileSync(f, 'utf8')]);

const definedClasses = new Set([...css.matchAll(/\.([a-zA-Z][\w-]*)/g)].map((m) => m[1]));
const definedVars = new Set([...css.matchAll(/^\s*(--[\w-]+)\s*:/gm)].map((m) => m[1]));

const problems = [];
for (const [file, src] of js) {
  // class="a b" / className = 'a' / classList.add('a') / class="x ${...}"
  for (const m of src.matchAll(/class(?:Name)?\s*=\s*[{'"`]([^'"`}]+)/g)) {
    for (const cls of m[1].split(/\s+/)) {
      if (cls && /^[a-zA-Z][\w-]*$/.test(cls) && !cls.includes('$') && !definedClasses.has(cls)) problems.push(`${file}: class .${cls}`);
    }
  }
  for (const m of src.matchAll(/classList\.(?:add|toggle|remove)\(\s*['"`]([\w-]+)/g)) {
    if (!definedClasses.has(m[1])) problems.push(`${file}: class .${m[1]}`);
  }
  // var(--x) built statically in JS. Dynamic `var(--${q})` is checked by its fallback.
  for (const m of src.matchAll(/var\((--[\w-]+)(?!\s*,)/g)) {
    if (!definedVars.has(m[1])) problems.push(`${file}: var ${m[1]}`);
  }
}

// CSP is `style-src 'self'`: it drops the style *attribute* while still allowing CSSOM
// setters, so an inline style is a declaration that never applies in production plus a
// console violation nobody reads. 22 of these shipped before Lighthouse counted 58
// violations and found the empty states, the people editor and the 404 page unstyled.
for (const [file, src] of js) {
  const inline = [...src.matchAll(/\sstyle="[^"]*"/g)].map((m) => m[0].trim().slice(0, 46));
  for (const hit of inline) problems.push(`${file}: inline ${hit}`);
  if (/\.style\.cssText\s*=/.test(src)) problems.push(`${file}: .style.cssText (parses as a style attribute; CSP blocks it)`);
  if (/setAttribute\(\s*['"`]style['"`]/.test(src)) problems.push(`${file}: setAttribute('style') (CSP blocks it)`);
}

// A band fill is chosen for large areas; as small text it fails contrast, which happened
// three separate times here (the count labels, the score badge, the role chip). Anything
// that genuinely wants a fill as `color` - an SVG stroked with currentColor - must say so
// with `/* icon */`, so the choice is written down rather than defaulted.
const FILL_AS_TEXT = /(?<![-\w])color:\s*var\(--(?:working|shoulder|offhours|asleep)\)/;
for (const rule of css.split('}')) {
  if (FILL_AS_TEXT.test(rule) && !rule.includes('/* icon */')) {
    problems.push(`app/style/app.css: band fill used as text colour in "${rule.trim().split('{')[0].trim().slice(0, 40)}" (use a -text variant, or mark /* icon */)`);
  }
}

// Every quality the engine can emit must have a colour, matched by the wire spelling.
const qualities = ['working', 'shoulder', 'offhours', 'asleep'];
for (const q of qualities) {
  if (!definedVars.has(`--${q}`)) problems.push(`app/style/app.css: no --${q} (engine quality)`);
}

const unique = [...new Set(problems)];
if (unique.length) {
  console.error(`style contract violated:\n${unique.map((p) => `  - ${p}`).join('\n')}`);
  process.exit(1);
}
/* CSP is script-src 'self' with no 'unsafe-inline', so an inline <script> never runs: it
 * is dead code that also logs a console error. In our case it silently disabled the entire
 * boot script - timezone label, date field, clock - for every visitor while looking
 * perfectly fine on a local server that does not send the header. */
for (const [file, src] of js.filter(([f]) => f.endsWith('.html'))) {
  const inline = [...src.matchAll(/<script(?![^>]*\ssrc=)[^>]*>/gi)].length;
  if (inline) problems.push(`${file}: ${inline} inline <script> block(s) - CSP script-src 'self' will not execute them`);
}

console.log(`style contract ok · ${definedClasses.size} classes, ${definedVars.size} custom properties`);
