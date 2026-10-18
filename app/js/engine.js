/* The entire wasm boundary on the JS side.
 *
 * Every call is a thin JSON-typed wrapper over one exported function, and every call
 * returns either parsed data or {error}. Nothing here touches the DOM.
 */

let wasm = null;
let ready = false;
let datasetLoaded = false;

const ERR = (msg) => ({ error: String(msg) });

function parse(text) {
  try {
    return JSON.parse(text);
  } catch (e) {
    return ERR(`engine returned non-JSON: ${String(text).slice(0, 120)}`);
  }
}

export async function init() {
  if (ready) return true;
  const mod = await import('/pkg/tt_wasm.js?v=APP_VERSION');
  // wasm-bindgen's web target wants the URL of the _bg.wasm file.
  await mod.default({ module_or_path: '/pkg/tt_wasm_bg.wasm?v=APP_VERSION' });
  wasm = mod;
  ready = true;
  return true;
}

export const isReady = () => ready;
export const isDatasetLoaded = () => datasetLoaded;

/** Fetch + register the city index. Called on first search, never at boot. */
export async function loadDataset() {
  if (!ready || datasetLoaded) return datasetLoaded;
  const res = await fetch('/tt-dataset.bin?v=APP_VERSION', { cache: 'force-cache' });
  if (!res.ok) throw new Error(`dataset fetch failed: ${res.status}`);
  const buf = await res.arrayBuffer();
  const out = parse(wasm.init_dataset(new Uint8Array(buf)));
  if (out.error) throw new Error(out.error);
  datasetLoaded = true;
  return out;
}

export function buildDay({ anchorDate, viewerTz, durationMin, people, spanDays, now }) {
  if (!ready) return ERR('engine not loaded');
  return parse(wasm.build_day(anchorDate, viewerTz, JSON.stringify(people), BigInt(now), spanDays));
}

export function scoreWindow({ startUtc, durationMin, viewerTz, people }) {
  if (!ready) return ERR('engine not loaded');
  return parse(wasm.score_window(BigInt(startUtc), durationMin, viewerTz, JSON.stringify(people)));
}

export function suggestSlots({ anchorDate, viewerTz, people, now, durationMin, k = 6, spanHours = 24 }) {
  if (!ready) return ERR('engine not loaded');
  return parse(wasm.suggest_slots(anchorDate, viewerTz, JSON.stringify(people), BigInt(now), durationMin, k, spanHours));
}

export function search(query, k = 8) {
  if (!ready || !datasetLoaded) return [];
  const out = parse(wasm.search_cities(query, k, BigInt(Math.floor(Date.now() / 1000))));
  return Array.isArray(out) ? out : [];
}

export function renderIcs({ startUtc, durationMin, viewerTz, people, summary, link }) {
  if (!ready) return '';
  return wasm.render_ics(
    BigInt(startUtc), durationMin, viewerTz, JSON.stringify(people),
    JSON.stringify({ summary: summary || 'Meeting', link: link || null, local_times: true, now_utc: Math.floor(Date.now() / 1000) })
  );
}

export function renderText({ startUtc, durationMin, viewerTz, people, link }) {
  if (!ready) return '';
  return wasm.render_text(BigInt(startUtc), durationMin, viewerTz, JSON.stringify(people), link || '');
}

export function encodeState(state) {
  if (!ready) return '';
  return wasm.encode_state(JSON.stringify(state));
}

export function decodeState(hash) {
  if (!ready) return { isEmpty: true, people: [] };
  return parse(wasm.decode_state(hash));
}

export { wasm };
