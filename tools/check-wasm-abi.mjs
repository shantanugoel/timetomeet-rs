// Fail the build when a wasm i64 argument is passed as a JS Number (or vice versa).
//
// wasm-bindgen maps Rust i64/u64 to BigInt, and every other integer to Number. Getting it
// wrong does not produce a wrong answer - it throws inside the call, and in two cases
// during development that exception was swallowed by a promise, so the UI simply showed an
// empty result list with no error anywhere. This check reads the Rust signatures and the
// JS call sites and compares them.
import { readFileSync } from 'node:fs';

const rust = readFileSync('crates/tt-wasm/src/lib.rs', 'utf8');
const js = readFileSync('app/js/engine.js', 'utf8');

const INT64 = /^(i64|u64|isize|usize)$/;
const NUMERIC = /^(i8|u8|i16|u16|i32|u32|f32|f64)$/;

const splitArgs = (src) => {
  const out = [];
  let depth = 0, cur = '';
  for (const ch of src) {
    if (ch === ',' && depth === 0) { out.push(cur.trim()); cur = ''; continue; }
    if ('([{<'.includes(ch)) depth++;
    if (')]}>'.includes(ch)) depth--;
    cur += ch;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
};

const fns = {};
for (const m of rust.matchAll(/pub fn (\w+)\s*\(([\s\S]*?)\)\s*->/g)) {
  const [, name, sig] = m;
  const params = splitArgs(sig).map((p) => {
    const i = p.lastIndexOf(':');
    return { name: p.slice(0, i).trim(), type: p.slice(i + 1).trim() };
  });
  fns[name] = params;
}

const problems = [];
for (const [name, params] of Object.entries(fns)) {
  const at = js.indexOf(`wasm.${name}(`);
  if (at < 0) { problems.push(`engine.js never calls ${name}`); continue; }
  let i = at + `wasm.${name}(`.length;
  let depth = 1;
  const start = i;
  while (i < js.length && depth > 0) {
    if ('([{'.includes(js[i])) depth++;
    else if (')]}'.includes(js[i])) depth--;
    i++;
  }
  const callArgs = splitArgs(js.slice(start, i - 1));
  if (callArgs.length !== params.length) {
    problems.push(`${name}: rust takes ${params.length} args, engine.js passes ${callArgs.length}`);
    continue;
  }
  params.forEach((p, idx) => {
    const arg = callArgs[idx];
    const isBigInt = /^BigInt\(/.test(arg) || /^0n$/.test(arg);
    if (INT64.test(p.type) && !isBigInt) problems.push(`${name}(${p.name}): i64 arg passed as \`${arg}\` - needs BigInt(...)`);
    if (NUMERIC.test(p.type) && isBigInt) problems.push(`${name}(${p.name}): ${p.type} arg passed as BigInt - needs a Number`);
  });
}

if (problems.length) {
  console.error(`wasm ABI mismatch:\n${[...new Set(problems)].map((p) => `  - ${p}`).join('\n')}`);
  process.exit(1);
}
console.log(`wasm ABI ok · ${Object.keys(fns).length} exports, i64 args verified at every call site`);
