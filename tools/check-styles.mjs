// Fail the build when the JS references a class or CSS custom property the stylesheet
// does not define.
//
// This is not pedantry: a band gradient with one undefined var() invalidates the whole
// background-image and renders a blank row, and an undefined class silently renders an
// unstyled panel. Both happened during development and neither produced a console error,
// so the only thing that would have caught them is this check.
import { readFileSync } from 'node:fs';

const css = readFileSync('app/style/app.css', 'utf8');
const js = ['app/js/app.js', 'app/js/engine.js'].map((f) => [f, readFileSync(f, 'utf8')]);

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
console.log(`style contract ok · ${definedClasses.size} classes, ${definedVars.size} custom properties`);
