/* Time to Meet - the view layer.
 *
 * Deliberately plain DOM + real pointer events, no framework: the grid is one element per
 * row whose *entire* availability band is a CSS gradient string produced by the Rust
 * engine. Dragging therefore never creates, removes or restyles a band node - it moves one
 * absolutely-positioned box and re-runs `score_window`. That is the whole reason a
 * 12-person grid stays at 60fps.
 *
 * The engine owns every decision about time. Nothing in this file knows what an offset is.
 */

import * as E from './engine.js?v=APP_VERSION';

const MIN_PER_DAY = 1440;
const SNAP = 15;
const LS = { groups: 'tt-groups', theme: 'tt-theme', people: 'tt-people', viewer: 'tt-viewer', group: 'tt-group' };

const DEFAULTS = [
  { name: 'You', city: 'Bengaluru', tz: 'Asia/Kolkata', role: 'organizer' },
  { name: 'Ana', city: 'San Francisco', tz: 'America/Los_Angeles', role: 'required', work: [480, 960] },
  { name: 'Ravi', city: 'London', tz: 'Europe/London', role: 'required' },
  { name: 'Mei', city: 'Tokyo', tz: 'Asia/Tokyo', role: 'optional', flex: 30 },
];

const $ = (id) => document.getElementById(id);
const nowSec = () => Math.floor(Date.now() / 1000);
const pad = (n) => (n < 10 ? '0' : '') + n;
const isoOf = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

const state = {
  viewerTz: localStorage.getItem(LS.viewer) || safeTz(),
  anchorDate: '',
  durationMin: 60,
  spanDays: 1,
  people: [],
  startUtc: null,
  group: localStorage.getItem(LS.group) || '',
  mode: 'plan',
  autoSlot: true,
  report: null,
  verdict: null,
  slots: [],
};

function safeTz() {
  try { return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'; } catch { return 'UTC'; }
}

function makePerson(p, i) {
  return {
    id: i,
    name: p.name,
    city: p.city || p.name,
    tz: p.tz,
    work_start_min: p.work?.[0] ?? 540,
    work_end_min: p.work?.[1] ?? 1080,
    flex_min: p.flex ?? 60,
    role: p.role || 'required',
    weekend: p.weekend || 'satSun',
    lat: p.lat ?? 0,
    lon: p.lon ?? 0,
  };
}

/* ------------------------------------------------------------------ rendering */

let bandWidth = 0;      // pixel width of one 24h band
let activeGridStart = 0; // grid start of the day the selection lives in

/* The band width is derived from its container rather than from a painted .band, because
   label density has to be known *before* the rows are built - and a fixed label step turns
   into unreadable overlapping text the moment the grid gets narrow. */
function measure() {
  const labelW = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--label-w')) || 176;
  bandWidth = Math.max(120, ($('grid')?.clientWidth || 720) - labelW - 6);
}

/** Smallest hour step from this ladder that keeps `count` labels of `charW` apart. */
function hourStep(charW) {
  for (const s of [1, 2, 3, 4, 6, 8, 12, 24]) if (((24 / s) + 1) * charW <= bandWidth) return s;
  return 24;
}

/** Turn the engine's run-length bands into ONE CSS gradient (never one node per cell).
 * The fallback in var() is deliberate: an unrecognised quality must degrade to a visible
 * grey band, not invalidate the whole background-image and render an empty row. */
function gradientFor(bands) {
  if (!bands?.length) return 'none';
  // A hairline is drawn at every quality boundary. Two adjacent qualities can always be
  // close in luminance - and they must stay distinguishable for someone who cannot see
  // the hue at all - so the *edge* carries the structure, not the fill alone. It costs
  // nothing: it is more stops in the gradient the band already has.
  const EDGE = 0.16; // percent of the band, ~1.3px at a 800px grid
  const stops = [];
  bands.forEach((b, i) => {
    const a = (b.start_min / MIN_PER_DAY) * 100;
    const z = (b.end_min / MIN_PER_DAY) * 100;
    const from = i > 0 ? a + EDGE : a;
    stops.push(`var(--${b.quality}, var(--offhours)) ${from.toFixed(3)}% ${z.toFixed(3)}%`);
    if (i < bands.length - 1) {
      stops.push(`var(--band-edge) ${z.toFixed(3)}% ${(z + EDGE).toFixed(3)}%`);
    }
  });
  return `linear-gradient(90deg, ${stops.join(', ')})`;
}

/** Mask for the weekend hatch, built from the same run-length bands. */
function weekendMask(bands) {
  const runs = (bands || []).filter((b) => b.weekend);
  if (!runs.length) return null;
  const stops = [];
  let cursor = 0;
  for (const b of runs) {
    if (b.start_min > cursor) stops.push(`transparent ${pct(cursor)} ${pct(b.start_min)}`);
    stops.push(`#000 ${pct(b.start_min)} ${pct(b.end_min)}`);
    cursor = b.end_min;
  }
  if (cursor < MIN_PER_DAY) stops.push(`transparent ${pct(cursor)} 100%`);
  return `linear-gradient(90deg, ${stops.join(', ')})`;
}

const pct = (min) => `${((min / MIN_PER_DAY) * 100).toFixed(3)}%`;

function rowNode(row) {
  const el = document.createElement('div');
  el.className = 'row';
  el.dataset.pid = row.person_id;

  const label = document.createElement('div');
  label.className = 'rowlabel';
  const marks = row.dst_change ? ' <span title="DST change on this date">⚠</span>' : '';
  label.innerHTML =
    `<div class="n"><span class="nm"></span></div>
     <div class="mt"></div>
     <div class="m"><span class="tz">${row.offset_min >= 0 ? '+' : '-'}${pad(Math.floor(Math.abs(row.offset_min) / 60))}:${pad(Math.abs(row.offset_min) % 60)}</span>
     <span class="work">${row.work_label}</span>${marks}</div>`;
  label.querySelector('.nm').textContent = row.name;
  if (row.role !== 'required') {
    const r = document.createElement('span');
    r.className = `role ${row.role}`;
    r.textContent = row.role === 'organizer' ? 'you' : row.role;
    label.querySelector('.n').appendChild(r);
  }

  const band = document.createElement('div');
  band.className = 'band';
  band.style.setProperty('--bands', gradientFor(row.bands));
  band.setAttribute('role', 'img');
  band.setAttribute('aria-label', ariaFor(row));

  const weekmask = weekendMask(row.bands);
  if (weekmask) {
    const w = document.createElement('div');
    w.className = 'weekend';
    w.style.setProperty('--weekmask', weekmask);
    band.appendChild(w);
  }
  // Their local hour ruler: reading each person's time in their own frame is the whole
  // point of the layout, so it is printed inside the row, not just on the shared axis.
  const localStep = hourStep(30);
  for (let h = 0; h < 24; h += localStep) {
    const t = document.createElement('span');
    t.className = 'local';
    t.style.insetInlineStart = pct(h * 60);
    t.textContent = row.hour_labels?.[h] ?? '';
    band.appendChild(t);
  }
  for (const m of row.midnights || []) {
    const d = document.createElement('span');
    d.className = 'mark midnight';
    d.style.insetInlineStart = pct(m.grid_min);
    d.title = m.date_label;
    band.appendChild(d);
  }
  for (const [min, glyph, title] of [[row.sunrise_min, '↑', 'sunrise'], [row.sunset_min, '↓', 'sunset']]) {
    if (min == null) continue;
    const d = document.createElement('span');
    d.className = 'mark sun';
    d.style.insetInlineStart = pct(min);
    d.textContent = glyph;
    d.title = title;
    band.appendChild(d);
  }
  if (row.now_min != null) {
    const d = document.createElement('span');
    d.className = 'mark now';
    d.style.insetInlineStart = pct(row.now_min);
    d.title = 'their now';
    band.appendChild(d);
  }

  el.append(label, band);
  return el;
}

function ariaFor(row) {
  if (!row.ok) return `${row.name}: ${row.error}`;
  const parts = row.bands.map((b) => `${b.quality} from ${fmtMin(b.start_min)} to ${fmtMin(b.end_min)}`);
  return `${row.name}, ${row.city}, UTC offset ${row.offset_min / 60}. Working hours ${row.work_label}. ${parts.join(', ')}.`;
}
const fmtMin = (m) => `${pad(Math.floor(m / 60) % 24)}:${pad(m % 60)}`;

function paintRows() {
  measure();
  const rows = $('rows');
  rows.querySelector('#booting')?.remove();
  rows.querySelectorAll('.daygroup, .axis.dayhead').forEach((n) => n.remove());
  if (!state.report?.days?.length) return;

  const frag = document.createDocumentFragment();
  state.report.days.forEach((day, di) => {
    if (state.spanDays > 1) {
      const head = document.createElement('div');
      head.className = 'axis dayhead';
      head.style.position = 'relative';
      head.innerHTML = `<div class="who">${day.date_label}</div><div class="ruler"></div>`;
      frag.appendChild(head);
    }
    const g = document.createElement('div');
    g.className = 'daygroup';
    g.dataset.gridStart = day.grid_start;
    for (const row of day.rows) g.appendChild(rowNode(row));
    if (day.dst_zones.length) {
      const note = document.createElement('div');
      note.className = 'empty';
      note.style.cssText = 'padding:6px 16px;text-align:start;font-size:12px;color:var(--danger)';
      note.textContent = `⚠ DST change on ${day.date_label} for ${day.dst_zones.join(', ')} — clocks shift relative to everyone else.`;
      g.appendChild(note);
    }
    frag.appendChild(g);
  });
  rows.appendChild(frag);

  paintRuler();
  paintSelection();
  // The grid was just rebuilt, so every label is blank until the current selection is
  // written back into it.
  if (state.verdict) paintRowTimes(state.verdict);
}

/// The viewer's own date label for an instant, using the day slice that contains it -
/// so a 3-day span compares each person's date against the right day, not day one.
function viewerDateLabel(startUtc) {
  const days = state.report?.days || [];
  let label = days[0]?.date_label ?? '';
  for (const d of days) if (startUtc >= d.grid_start) label = d.date_label;
  return label;
}

/// "What time is this for ME" is the whole point of the tool, and the row label is the
/// only region that stays on screen while the grid scrolls sideways - so the answer lives
/// there, not only in the slot cards. Colour alone never carries it: the square takes the
/// quality hue, the digits carry the fact, and a weekday appears whenever their date is
/// not yours.
function paintRowTimes(v) {
  const viewerDate = viewerDateLabel(v.start_utc);
  for (const p of v.persons) {
    const el = document.querySelector(`.row[data-pid="${p.person_id}"] .mt`);
    if (!el) continue;
    el.className = `mt q-${p.quality}`;
    el.textContent = '';
    const dot = document.createElement('i');
    dot.className = 'dot';
    const time = document.createElement('span');
    time.textContent = `${p.local_start}–${p.local_end}`;
    el.append(dot, time);
    if (p.local_date !== viewerDate) {
      const d = document.createElement('span');
      d.className = 'd';
      d.textContent = p.local_date.split(' ')[0];
      el.append(d);
    }
    if (p.clipped_min) {
      const c = document.createElement('span');
      c.className = 'clip';
      c.textContent = `−${p.clipped_min}m`;
      el.append(c);
    }
    el.title = `${p.local_date} · ${p.reason}`;
  }
}

function clearRowTimes() {
  document.querySelectorAll('.row .mt').forEach((el) => { el.textContent = ''; el.title = ''; });
}

function paintRuler() {
  const ruler = $('ruler');
  ruler.innerHTML = '';
  if (!state.report?.days?.length) return;
  const gs = state.report.days[0].grid_start;
  const fmt = new Intl.DateTimeFormat('en-GB', { hour: '2-digit', hourCycle: 'h23', timeZone: state.viewerTz });
  const step = hourStep(26);
  for (let h = 0; h < 24; h += 1) {
    const ts = gs + h * 3600;
    const s = document.createElement('span');
    s.style.insetInlineStart = `${((h / 24) * 100).toFixed(3)}%`;
    s.textContent = fmt.format(new Date(ts * 1000));
    if (h % 6 === 0) s.className = 'major';
    if (h % step) s.remove();
    else ruler.appendChild(s);
  }
  $('axisWho').textContent = `${state.report.days[0].date_label} · your day`;
}

/* ------------------------------------------------------------------ selection */

function selMark() {
  let m = $('ruler').querySelector('.selmark');
  if (!m) { m = document.createElement('div'); m.className = 'selmark'; $('ruler').appendChild(m); }
  return m;
}

function selBox() {
  let box = $('overlay').querySelector('.sel');
  if (!box) {
    box = document.createElement('div');
    box.className = 'sel';
    box.innerHTML = '<div class="grip start" data-grip="start"></div><div class="tag"></div><div class="grip end" data-grip="end"></div>';
    box.tabIndex = 0;
    box.setAttribute('role', 'slider');
    box.setAttribute('aria-label', 'Meeting window');
    $('overlay').appendChild(box);
    attachDrag(box);
  }
  return box;
}

function paintSelection() {
  const box = selBox();
  if (state.mode === 'roster' || state.startUtc == null || !activeDay()) {
    box.style.display = 'none';
    const m = $('ruler').querySelector('.selmark');
    if (m) m.style.display = 'none';
    return;
  }
  const mk = $('ruler').querySelector('.selmark');
  if (mk) mk.style.display = '';
  const day = activeDay();
  const startMin = (state.startUtc - day.grid_start) / 60;
  if (startMin < 0 || startMin > MIN_PER_DAY) { box.style.display = 'none'; return; }
  box.style.display = '';
  box.style.insetInlineStart = pct(startMin);
  const wMin = Math.min(state.durationMin, MIN_PER_DAY - startMin);
  box.style.inlineSize = pct(wMin);
  const a = fmtClock(state.startUtc), b = fmtClock(state.startUtc + state.durationMin * 60);
  box.setAttribute('aria-valuetext', `${a} to ${b} in your timezone`);
  const mark = selMark();
  mark.style.insetInlineStart = pct(startMin);
  mark.style.inlineSize = pct(wMin);
  mark.dataset.t = `${a}-${b}`;
  mark.textContent = `${a}-${b}`;
  mark.classList.toggle('tight', (wMin / MIN_PER_DAY) * (bandWidth || 700) < 92);
  // Flip the label to the leading side when it would run past the grid. The threshold is
  // in minutes, so it has to be derived from the current band width: on a phone a 70px
  // label is worth 400+ minutes, on a desktop barely 130.
  const labelMin = (80 / Math.max(120, bandWidth)) * MIN_PER_DAY;
  mark.classList.toggle('left', startMin > MIN_PER_DAY - labelMin);
}

function activeDay() {
  if (!state.report?.days) return null;
  return state.report.days.find((d) => state.startUtc >= d.grid_start && state.startUtc < d.grid_start + 86400) || state.report.days[0];
}

const shortDate = (ts) =>
  new Intl.DateTimeFormat('en-GB', { weekday: 'short', day: 'numeric', month: 'short', timeZone: state.viewerTz }).format(new Date(ts * 1000));

function fmtClock(ts) {
  return new Intl.DateTimeFormat('en-GB', { hour: '2-digit', minute: '2-digit', hour12: false, timeZone: state.viewerTz }).format(new Date(ts * 1000));
}

function minutesFromEvent(ev) {
  const band = document.querySelector('.daygroup .band');
  if (!band) return 0;
  const r = band.getBoundingClientRect();
  const frac = Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width));
  return Math.round((frac * MIN_PER_DAY) / SNAP) * SNAP;
}

function attachDrag(box) {
  let mode = null, grabOffsetMin = 0;
  box.addEventListener('pointerdown', () => { state.autoSlot = false; }, true);
  const dayStart = () => activeDay()?.grid_start ?? 0;

  box.addEventListener('pointerdown', (ev) => {
    if (state.startUtc == null) return;
    mode = ev.target.dataset.grip || 'move';
    grabOffsetMin = minutesFromEvent(ev) - (state.startUtc - dayStart()) / 60;
    box.classList.add('dragging');
    box.setPointerCapture(ev.pointerId);
    ev.preventDefault();
  });

  box.addEventListener('pointermove', (ev) => {
    if (!mode) return;
    const m = minutesFromEvent(ev);
    const gs = dayStart();
    if (mode === 'move') {
      const start = clamp(m - grabOffsetMin, 0, MIN_PER_DAY - state.durationMin);
      state.startUtc = gs + start * 60;
    } else if (mode === 'start') {
      const end = (state.startUtc - gs) / 60 + state.durationMin;
      const start = clamp(m, 0, end - SNAP);
      state.durationMin = Math.round((end - start) / SNAP) * SNAP;
      state.startUtc = gs + start * 60;
    } else {
      const start = (state.startUtc - gs) / 60;
      state.durationMin = clamp(Math.round((m - start) / SNAP) * SNAP, SNAP, MIN_PER_DAY - start);
    }
    // Cheap path: move the box and re-score. The band DOM is never touched.
    paintSelection();
    requestCheap();
  });

  const end = (ev) => {
    if (!mode) return;
    mode = null;
    box.classList.remove('dragging');
    try { box.releasePointerCapture(ev.pointerId); } catch {}
    syncDurationButtons();
    paint({ slots: true });
  };
  box.addEventListener('pointerup', end);
  box.addEventListener('pointercancel', end);

  box.addEventListener('keydown', (ev) => {
    const gs = dayStart();
    const start = (state.startUtc ?? gs) - gs;
    const step = (ev.shiftKey ? 60 : SNAP) * 60;
    let handled = true;
    if (ev.key === 'ArrowRight') state.startUtc = gs + Math.min(start + step, (MIN_PER_DAY - state.durationMin) * 60);
    else if (ev.key === 'ArrowLeft') state.startUtc = gs + Math.max(start - step, 0);
    else if (ev.key === '+' || ev.key === '=') state.durationMin = Math.min(state.durationMin + SNAP, MIN_PER_DAY - start / 60);
    else if (ev.key === '-') state.durationMin = Math.max(state.durationMin - SNAP, SNAP);
    else handled = false;
    if (handled) { ev.preventDefault(); paintSelection(); paint({ slots: false }); syncDurationButtons(); }
  });
}

const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

// Tap anywhere on a band to drop the meeting there - the primary touch interaction.
$('rows').addEventListener('pointerdown', (ev) => {
  const band = ev.target.closest('.band');
  if (!band || ev.target.closest('.sel')) return;
  state.autoSlot = false;
  const group = band.closest('.daygroup');
  const gs = Number(group?.dataset.gridStart || 0);
  if (!gs) return;
  const m = minutesFromEvent(ev);
  state.startUtc = gs + clamp(m - state.durationMin / 2, 0, MIN_PER_DAY - state.durationMin) * 60;
  paintSelection();
  paint({ slots: false });
});

/* ------------------------------------------------------------------ paint all */

let cheapPending = false;
function requestCheap() {
  if (cheapPending) return;
  cheapPending = true;
  requestAnimationFrame(() => { cheapPending = false; paintVerdict(); });
}

function paintVerdict() {
  if (state.startUtc == null || !state.people.length) { clearRowTimes(); return; }
  const v = E.scoreWindow({ startUtc: state.startUtc, durationMin: state.durationMin, viewerTz: state.viewerTz, people: state.people });
  if (v.error) return;
  state.verdict = v;
  paintRowTimes(v);
  const c = v.counts;
  $('selTitle').textContent = `${fmtClock(state.startUtc)}–${fmtClock(state.startUtc + state.durationMin * 60)} your time`;
  $('selSub').textContent = v.worst_name
    ? `${v.label} · worst off: ${v.worst_name} · ${v.score}/100`
    : `${v.label} · good for everyone · ${v.score}/100`;
  $('selCounts').innerHTML =
    `<b class="c-ok">${c.working} ok</b><b class="c-tight">${c.shoulder} tight</b>` +
    `<b class="c-hard">${c.off_hours} off</b><b class="c-bad">${c.asleep} asleep</b>`;
  $('live').textContent = `${v.score} out of 100. ${c.working} in working hours, ${c.shoulder} shoulder, ${c.off_hours} off hours, ${c.asleep} asleep.`;
  $('fairMeter').style.width = `${Math.round(v.equity * 100)}%`;
  $('fairText').innerHTML = v.worst_name
    ? `Equity ${Math.round(v.equity * 100)}% — <b>${v.worst_name}</b> carries the most of this one.`
    : `Equity ${Math.round(v.equity * 100)}% — the cost is shared.`;
  for (const b of ['icsBtn', 'textBtn', 'shareBtn', 'detailsBtn']) $(b).disabled = false;
  paintTimes(v);
}

function paintTimes(v) {
  const host = $('sheetTimes');
  if (!host) return;
  host.innerHTML = '';
  for (const p of v.persons) {
    const r = document.createElement('div');
    r.className = `r ${p.quality === 'asleep' ? 'bad' : ''}`;
    r.innerHTML = `<span class="who"></span><span class="when clock">${p.local_date} ${p.local_start}–${p.local_end}</span>
                   <span class="q">${qGlyph(p.quality)}</span>`;
    r.querySelector('.who').textContent = `${p.name} · ${p.city}`;
    r.title = p.reason;
    host.appendChild(r);
  }
}
const qGlyph = (q) => ({ working: '🟢 working', shoulder: '🟡 shoulder', offhours: '⚪ off hours', asleep: '🔴 asleep' }[q] || q);

function paintSlots() {
  const host = $('slotList');
  host.innerHTML = '';
  $('slotsFor').textContent = `· ${state.durationMin} min`;
  if (!state.slots.length) {
    host.innerHTML = '<div class="empty" style="padding:12px">Add a person to get suggestions.</div>';
    return;
  }
  for (const s of state.slots) {
    const el = document.createElement('div');
    el.className = 'slot';
    el.tabIndex = 0;
    el.setAttribute('role', 'button');
    const isSel = s.start_utc === state.startUtc;
    if (isSel) el.setAttribute('aria-current', 'true');
    const c = s.counts;
    el.innerHTML =
      `<div class="top"><span class="t">${s.is_anchor ? '' : `${esc(shortDate(s.start_utc))} `}${fmtClock(s.start_utc)}–${fmtClock(s.start_utc + s.duration_min * 60)}</span>
        <span class="score" title="${esc(s.label)}">${s.score}</span></div>
       <div class="sub">${s.persons.map((p) => `${p.name.split(' ')[0]} ${p.local_start}`).join(' · ')}</div>
       <div class="sub"><span class="counts"><b class="c-ok">${c.working}</b><b class="c-tight">${c.shoulder}</b><b class="c-hard">${c.off_hours}</b><b class="c-bad">${c.asleep}</b></span> <b>${esc(s.label)}</b> · ${esc(s.headline)}</div>`;
    const go = () => { state.autoSlot = false; state.startUtc = s.start_utc; paintSelection(); paintVerdict(); paintSlots(); };
    el.addEventListener('click', go);
    el.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(); } });
    host.appendChild(el);
  }
}

function paintPeople() {
  const host = $('peopleList');
  host.innerHTML = '';
  if (!state.people.length) {
    host.innerHTML = `<div class="empty" style="padding:14px"><h3>Nobody yet</h3>Press <kbd>⌘K</kbd> to add someone.</div>`;
    return;
  }
  state.people.forEach((p, i) => {
    const row = document.createElement('div');
    row.className = 'person-row';
    row.innerHTML =
      `<div class="top">
         <b class="nm"></b>
         <select data-f="role" aria-label="Role">
           ${['organizer', 'required', 'optional', 'peripheral'].map((r) => `<option ${p.role === r ? 'selected' : ''}>${r}</option>`).join('')}
         </select>
         <button class="btn ghost" data-f="rm" style="padding:2px 7px" aria-label="Remove ${esc(p.name)}">✕</button>
       </div>
       <div class="city clock">${esc(p.city)} · ${esc(p.tz)}</div>
       <div class="hours">
         <input type="time" data-f="ws" value="${hm(p.work_start_min)}" step="900" aria-label="Work start" style="padding:3px 5px">
         <span class="arrow">→</span>
         <input type="time" data-f="we" value="${hm(p.work_end_min)}" step="900" aria-label="Work end" style="padding:3px 5px">
         <span title="Flexibility beyond the declared window">±</span>
         <input type="number" data-f="fx" value="${p.flex_min}" min="0" max="240" step="15" style="inline-size:58px;padding:3px 5px" aria-label="Flex minutes">
       </div>`;
    row.querySelector('.nm').textContent = p.name;
    row.addEventListener('change', (e) => {
      const f = e.target.dataset.f;
      if (!f) return;
      const t = state.people[i];
      if (f === 'role') t.role = e.target.value;
      if (f === 'ws') t.work_start_min = Number(e.target.value.slice(0, 2)) * 60 + Number(e.target.value.slice(3, 5));
      if (f === 'we') t.work_end_min = Number(e.target.value.slice(0, 2)) * 60 + Number(e.target.value.slice(3, 5));
      if (f === 'fx') t.flex_min = clamp(Number(e.target.value) || 0, 0, 240);
      persistPeople();
      paint({ slots: true });
    });
    row.querySelector('[data-f=rm]').addEventListener('click', () => {
      state.people.splice(i, 1);
      state.people.forEach((pp, j) => (pp.id = j));
      persistPeople();
      paint({ slots: true });
    });
    host.appendChild(row);
  });
}
const hm = (min) => `${pad(Math.floor(min / 60) % 24)}:${pad(min % 60)}`;
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

async function paint({ slots = true } = {}) {
  if (!E.isReady()) return;
  state.report = E.buildDay({
    anchorDate: state.anchorDate, viewerTz: state.viewerTz, durationMin: state.durationMin,
    people: state.people, spanDays: state.spanDays, now: nowSec(),
  });
  if (state.report.error) { toast(`Engine: ${state.report.error}`); return; }
  // Say it out loud rather than showing a grid where nobody works and implying it is
// a normal day. The engine already reached into the next working day for suggestions.
  const restDay = state.report.days.some((d) => d.rest_day);
  $('restBanner')?.remove();
  if (restDay) {
    const b = document.createElement('div');
    b.id = 'restBanner';
    b.className = 'empty';
    b.style.cssText = 'padding:10px 16px;text-align:start;display:flex;gap:10px;align-items:center;flex-wrap:wrap;border-block-end:1px solid var(--hair)';
    b.innerHTML = `<span>Most of your team is <b>off</b> on ${esc(state.report.days[0].date_label)}. Suggestions reach into the next working day.</span>`;
    const jump = document.createElement('button');
    jump.className = 'btn';
    jump.textContent = 'Next day →';
    jump.addEventListener('click', () => shiftDate(1));
    b.appendChild(jump);
    $('grid').prepend(b);
  }
  if (state.startUtc == null && state.report.days[0]) {
    state.startUtc = state.report.days[0].grid_start + 9 * 3600;
  }
  activeGridStart = activeDay()?.grid_start ?? 0;
  paintRows();
  paintPeople();
  paintVerdict();
  if (slots) {
    const s = E.suggestSlots({
      anchorDate: state.anchorDate, viewerTz: state.viewerTz, people: state.people,
      now: nowSec(), durationMin: state.durationMin, k: 6, spanHours: 24 * state.spanDays,
    });
    state.slots = Array.isArray(s) ? s : [];
    // First visit: land on the best slot for this team. Opening on an arbitrary 09:00 that
    // the engine rates "brutal" makes the tool look like it is arguing with you.
    if (state.autoSlot && state.slots.length) {
      state.autoSlot = false;
      const best = state.slots.reduce((a, b) => (b.score > a.score ? b : a));
      if (best.start_utc !== state.startUtc) {
        state.startUtc = best.start_utc;
        paintRows();
        paintVerdict();
      }
    }
    paintSlots();
  }
  syncUrl();
}

/* ------------------------------------------------------------------ palette */

let results = [], cursor = 0, dsLoading = false;

async function openPalette() {
  $('palette').dataset.open = '';
  $('q').value = '';
  results = []; cursor = 0;
  $('results').innerHTML = '';
  $('q').focus();
  if (!E.isDatasetLoaded() && !dsLoading) {
    dsLoading = true;
    $('dsState').textContent = 'loading city index…';
    try {
      const info = await E.loadDataset();
      $('dsState').textContent = `${info.cities ?? 0} cities · ${info.zones ?? 0} zones`;
    } catch (e) {
      $('dsState').textContent = 'city index unavailable';
      $('dsState').title = String(e);
    }
  } else {
    $('dsState').textContent = '';
  }
}
function closePalette() { delete $('palette').dataset.open; }

async function runSearch() {
  const q = $('q').value.trim();
  if (q.length < 1) { $('results').innerHTML = ''; results = []; return; }
  // The engine needs the dataset; until it lands, offer the typed zone directly.
  results = E.search(q, 9);
  cursor = 0;
  const host = $('results');
  host.innerHTML = '';
  const items = [...results];
  // Always offer the raw zone if it looks like one and is not already in the list.
  if (q.includes('/') ) {
    const probe = { name: q, country: '', tz: q, is_zone: true, local_time: '', local_date: '', offset_min: 0 };
    if (!items.some((i) => i.tz === q)) items.unshift(probe);
  }
  if (!items.length) {
    host.innerHTML = `<li style="color:var(--muted)">No match. Type an IANA zone instead, e.g. <b>Asia/Colombo</b>.</li>`;
    return;
  }
  items.forEach((h, i) => {
    const li = document.createElement('li');
    li.setAttribute('role', 'option');
    li.setAttribute('aria-selected', i === cursor ? 'true' : 'false');
    li.innerHTML = `<span class="nm"></span><span class="lt clock">${esc(h.local_time)} ${esc(h.local_date)}</span><span class="z">${esc(h.tz)}</span>`;
    li.querySelector('.nm').innerHTML = `${esc(h.name)}${h.country ? `<small>${esc(h.country)}</small>` : ''}`;
    li.addEventListener('click', () => addFromHit(h));
    host.appendChild(li);
  });
  results = items;
}

function moveCursor(d) {
  if (!results.length) return;
  cursor = (cursor + d + results.length) % results.length;
  [...$('results').children].forEach((li, i) => li.setAttribute('aria-selected', i === cursor ? 'true' : 'false'));
  $('results').children[cursor]?.scrollIntoView({ block: 'nearest' });
}

function addFromHit(h, replaceAll = false) {
  const person = makePerson({ name: h.name, city: h.name, tz: h.tz, lat: h.lat, lon: h.lon, role: state.people.length ? 'required' : 'organizer' }, 0);
  if (replaceAll) state.people = [person];
  else state.people.push(person);
  state.people.forEach((p, i) => (p.id = i));
  persistPeople();
  closePalette();
  toast(`${h.name} added`);
  paint({ slots: true });
}

/* ------------------------------------------------------------------ share */

function shareUrl() {
  const enc = E.encodeState({
    anchorDate: state.anchorDate, viewerTz: state.viewerTz, durationMin: state.durationMin,
    startUtc: state.startUtc, people: state.people, group: state.group || null,
  });
  return `${location.origin}/#${enc}`;
}

function syncUrl() {
  const enc = E.encodeState({
    anchorDate: state.anchorDate, viewerTz: state.viewerTz, durationMin: state.durationMin,
    startUtc: state.startUtc, people: state.people, group: state.group || null,
  });
  history.replaceState(null, '', `#${enc}`);
  if (state.startUtc != null) {
    document.title = `${fmtClock(state.startUtc)}–${fmtClock(state.startUtc + state.durationMin * 60)} · ${state.people.length} people — Time to Meet`;
  }
  clearTimeout(syncUrl._t);
}

async function copy(text, what) {
  try {
    await navigator.clipboard.writeText(text);
    toast(`${what} copied`);
  } catch {
    const ta = document.createElement('textarea');
    ta.value = text; document.body.appendChild(ta); ta.select();
    document.execCommand('copy'); ta.remove();
    toast(`${what} copied`);
  }
}

function downloadIcs() {
  const ics = E.renderIcs({
    startUtc: state.startUtc, durationMin: state.durationMin, viewerTz: state.viewerTz,
    people: state.people, summary: $('sheetSummary').value || 'Meeting', link: shareUrl(),
  });
  const blob = new Blob([ics], { type: 'text/calendar;charset=utf-8' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `meeting-${state.anchorDate}-${fmtClock(state.startUtc).replace(':', '')}.ics`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 4000);
  toast('Calendar file downloaded');
}

function openSheet() {
  if (state.startUtc == null) return;
  $('sheetTitle').textContent = `${fmtClock(state.startUtc)}-${fmtClock(state.startUtc + state.durationMin * 60)} your time`;
  paintTimes(state.verdict || E.scoreWindow({
    startUtc: state.startUtc, durationMin: state.durationMin, viewerTz: state.viewerTz, people: state.people,
  }));
  $('sheetText').value = E.renderText({
    startUtc: state.startUtc, durationMin: state.durationMin, viewerTz: state.viewerTz,
    people: state.people, link: shareUrl(),
  });
  $('sheet').dataset.open = '';
  $('sheetSummary').focus();
}

/* ------------------------------------------------------------------ groups */

const loadGroups = () => { try { return JSON.parse(localStorage.getItem(LS.groups) || '{}'); } catch { return {}; } };
function persistPeople() {
  localStorage.setItem(LS.people, JSON.stringify(state.people));
  localStorage.setItem(LS.viewer, state.viewerTz);
}

function groupMenu() {
  const groups = loadGroups();
  const names = Object.keys(groups).sort();
  const chosen = prompt(
    `Saved groups:\n${names.length ? names.map((n) => `  ${n} (${groups[n].people.length})`).join('\n') : '  (none yet)'}\n\nType a name to load it, "new NAME" to save the current people, or "del NAME" to remove.`
  );
  if (!chosen) return;
  const cmd = chosen.trim();
  if (cmd.startsWith('new ')) {
    const n = cmd.slice(4).trim();
    if (!n) return;
    groups[n] = { people: state.people, viewerTz: state.viewerTz };
    localStorage.setItem(LS.groups, JSON.stringify(groups));
    state.group = n; localStorage.setItem(LS.group, n);
    $('groupName').textContent = n;
    toast(`Saved "${n}"`);
  } else if (cmd.startsWith('del ')) {
    delete groups[cmd.slice(4).trim()];
    localStorage.setItem(LS.groups, JSON.stringify(groups));
    toast('Deleted');
  } else if (groups[cmd]) {
    state.people = groups[cmd].people.map(makePerson);
    state.people.forEach((p, i) => (p.id = i));
    state.group = cmd; localStorage.setItem(LS.group, cmd);
    $('groupName').textContent = cmd;
    persistPeople();
    paint({ slots: true });
  } else {
    toast(`No group called "${cmd}"`);
  }
}

/* ------------------------------------------------------------------ chrome */

let toastTimer;
function toast(msg) {
  document.querySelector('.toast')?.remove();
  const t = document.createElement('div');
  t.className = 'toast';
  t.textContent = msg;
  document.body.appendChild(t);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.remove(), 2600);
}

function syncDurationButtons() {
  for (const b of $('durSeg').children) b.setAttribute('aria-pressed', String(Number(b.dataset.dur) === state.durationMin));
  for (const b of $('spanSeg').children) b.setAttribute('aria-pressed', String(Number(b.dataset.span) === state.spanDays));
}

function setMode(mode) {
  state.mode = mode;
  document.body.dataset.mode = mode;
  $('viewToggle').textContent = mode === 'roster' ? 'Plan' : 'Roster';
  document.documentElement.style.setProperty('--row-h', mode === 'roster' ? '34px' : '52px');
  $('rail_slots')?.style.setProperty('display', mode === 'roster' ? 'none' : '');
  paintSelection();
}

/* ------------------------------------------------------------------ boot */

async function boot() {
  // Wire chrome before the engine lands so the page is interactive immediately.
  $('searchBtn').addEventListener('click', openPalette);
  $('addBtn').addEventListener('click', openPalette);
  $('groupBtn').addEventListener('click', groupMenu);
  $('themeBtn').addEventListener('click', () => {
    const next = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = next;
    localStorage.setItem(LS.theme, next);
  });
  $('helpBtn').addEventListener('click', () =>
    toast('⌘K add · ←/→ move slot · ⇧←/⇧→ by hour · +/- duration · T today · R roster · ? help'));
  $('viewToggle').addEventListener('click', () => setMode(state.mode === 'roster' ? 'plan' : 'roster'));
  $('prevDay').addEventListener('click', () => shiftDate(-1));
  $('nextDay').addEventListener('click', () => shiftDate(1));
  $('todayBtn').addEventListener('click', () => {
    state.anchorDate = isoOf(new Date());
    $('dateInput').value = state.anchorDate;
    state.startUtc = null;
    state.autoSlot = true;
    paint();
  });
  $('dateInput').addEventListener('change', (e) => {
    // Never hand an empty value to the engine - that is what killed the old build.
    if (!e.target.value) { e.target.value = state.anchorDate; return; }
    state.anchorDate = e.target.value;
    state.startUtc = null;
    state.autoSlot = true;
    paint();
  });
  $('viewerChip').addEventListener('click', async () => {
    const t = prompt('Your timezone (IANA name):', state.viewerTz);
    if (!t) return;
    state.viewerTz = t.trim();
    $('viewerChip').firstElementChild.textContent = state.viewerTz;
    persistPeople();
    paint();
  });
  for (const b of $('durSeg').children) {
    b.addEventListener('click', () => {
      state.durationMin = Number(b.dataset.dur);
      syncDurationButtons();
      paint();
    });
  }
  for (const b of $('spanSeg').children) {
    b.addEventListener('click', () => { state.spanDays = Number(b.dataset.span); syncDurationButtons(); paint(); });
  }
  $('shareBtn').addEventListener('click', () => copy(shareUrl(), 'Link'));
  $('icsBtn').addEventListener('click', downloadIcs);
  $('textBtn').addEventListener('click', () => copy(E.renderText({
    startUtc: state.startUtc, durationMin: state.durationMin, viewerTz: state.viewerTz,
    people: state.people, link: shareUrl(),
  }), 'Text'));
  $('detailsBtn').addEventListener('click', openSheet);
  $('sheetClose').addEventListener('click', () => delete $('sheet').dataset.open);
  $('sheet').addEventListener('click', (e) => { if (e.target === $('sheet')) delete $('sheet').dataset.open; });
  $('sheetIcs').addEventListener('click', downloadIcs);
  $('sheetCopyText').addEventListener('click', () => copy($('sheetText').value, 'Text'));
  $('sheetCopyLink').addEventListener('click', () => copy(shareUrl(), 'Link'));
  $('sheetEmbed').addEventListener('click', () => {
    $('sheetText').value = `<iframe src="${shareUrl()}&embed=1" width="100%" height="260" title="Team timezone hours" loading="lazy"></iframe>`;
    toast('Embed code written to the box');
  });
  $('sheetSummary').addEventListener('input', () => {
    $('sheetText').value = E.renderText({
      startUtc: state.startUtc, durationMin: state.durationMin, viewerTz: state.viewerTz,
      people: state.people, link: shareUrl(),
    });
  });

  $('q').addEventListener('input', runSearch);
  $('q').addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); moveCursor(1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); moveCursor(-1); }
    else if (e.key === 'Enter') { e.preventDefault(); if (results[cursor]) addFromHit(results[cursor], e.shiftKey); }
    else if (e.key === 'Escape') closePalette();
  });
  $('palette').addEventListener('click', (e) => { if (e.target === $('palette')) closePalette(); });

  document.addEventListener('keydown', (e) => {
    // Escape is the universal dismiss and must work even when focus is inside a field -
    // otherwise a dialog you just opened can only be closed by clicking.
    if (e.key === 'Escape') { closePalette(); delete $('sheet').dataset.open; return; }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); openPalette(); return; }
    if (e.target.matches('input, textarea, select')) return;
    if (e.key === '?') $('helpBtn').click();
    else if (e.key.toLowerCase() === 'd') $('detailsBtn').click();
    else if (e.key.toLowerCase() === 't') $('todayBtn').click();
    else if (e.key.toLowerCase() === 'r') setMode(state.mode === 'roster' ? 'plan' : 'roster');
  });

  let lastStep = 0;
addEventListener('resize', () => {
  const prev = lastStep;
  measure();
  lastStep = hourStep(30);
  if (lastStep !== prev) paint({ slots: false });
  else { paintRuler(); paintSelection(); }
});
  addEventListener('hashchange', () => boot(true));

  // Clock ticks: the "their now" carets and the header clock are the alive signal.
  setInterval(() => {
    $('nowClock').textContent = new Intl.DateTimeFormat('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false, timeZone: state.viewerTz }).format(new Date());
    if (nowSec() % 60 === 0 && E.isReady()) paint();
  }, 1000);

  // State: URL hash wins, then localStorage, then defaults.
  const saved = localStorage.getItem(LS.people);
  state.anchorDate = isoOf(new Date());
  state.people = (saved ? JSON.parse(saved) : DEFAULTS).map(makePerson);

  await E.init();
  const fromUrl = E.decodeState(location.hash);
  if (fromUrl && !fromUrl.isEmpty) {
    if (fromUrl.anchorDate) state.anchorDate = fromUrl.anchorDate;
    if (fromUrl.viewerTz) state.viewerTz = fromUrl.viewerTz;
    if (fromUrl.durationMin) state.durationMin = fromUrl.durationMin;
    // A shared link names an exact moment. Never second-guess the sender.
    if (fromUrl.startUtc) { state.startUtc = fromUrl.startUtc; state.autoSlot = false; }
    if (fromUrl.people?.length) state.people = fromUrl.people.map(makePerson);
    if (fromUrl.group) { state.group = fromUrl.group; $('groupName').textContent = fromUrl.group; }
  }
  $('dateInput').value = state.anchorDate;
  $('viewerChip').firstElementChild.textContent = state.viewerTz;
  syncDurationButtons();
  if (new URLSearchParams(location.search).get('embed') === '1') setMode('roster');
  await paint();
  // Opening on a day when half the team is asleep shows a grid of nothing. If the visitor
  // did not ask for a specific date (no hash), land them on the next day they can
  // actually meet. "Today" still means today when they press it.
  if (!fromUrl || fromUrl.isEmpty) {
    for (let i = 0; i < 4 && state.report?.days?.[0]?.rest_day; i++) {
      shiftDate(1, false);
      await paint({ slots: false });
    }
    if (state.report?.days?.[0]?.rest_day === false) {
      toast('Opened on your next working day for this team');
    }
    await paint();
  }
}

function shiftDate(d, repaint = true) {
  const dt = new Date(state.anchorDate + 'T12:00:00');
  dt.setDate(dt.getDate() + d);
  state.anchorDate = isoOf(dt);
  $('dateInput').value = state.anchorDate;
  state.startUtc = null;
  // Changing the day is asking "and on that day?", so re-pick that day's best hour
  // instead of holding a time that was chosen for a different one.
  state.autoSlot = true;
  if (repaint) paint();
}

boot().catch((e) => {
  console.error(e);
  const rows = $('rows');
  if (rows) rows.innerHTML = `<div class="empty"><h3>The engine failed to load</h3>${esc(String(e))}</div>`;
});
