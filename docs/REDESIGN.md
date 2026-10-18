# Time to Meet — redesign proposal

Working doc. Nothing here is implemented yet. Every claim marked **[measured]** was
verified against the live site or the repo on 2026-10-17.

---

## 0. Where it stands today

**[measured]** Repo: Yew `0.18.0` (2020-era `ComponentLink`/`create(link)` API),
`chrono-tz 0.5.3`, `simsearch`, 4 source files, 361 lines of Rust, one 690 KB CSV,
`docs/` (build output) committed to git, no CI, no tests, no lints, no `Trunk.toml`.

**[measured]** What a visitor actually downloads on first visit:

| asset | raw | transfer (gzip) | brotli potential | cache |
|---|---|---|---|---|
| `index.html` | 463 B | 463 B | – | `max-age=600` |
| `index_bg.wasm` | 2,117,301 B | **410,324 B** | 267,698 B | `max-age=600` **[!]** |
| `index.js` | 19,957 B | 4,539 B | – | `max-age=600` |

`[!]` the wasm filename is content-hashed **and still cached for 10 minutes**. Every
returning visitor re-fetches 410 KB. That alone explains a lot of the "just about
usable" feel.

Of the 410 KB: ~142 KB gz is the 24,568-row city CSV compiled into the binary, and
the string table contains ~6,900 `America/*` entries — the entire IANA database.
Then at boot the app inserts all 24,568 rows into a `SimSearch` trie **on the main
thread, before first paint**.

### P0 bug (live, reproducible, verified in-browser)

`src/model.rs`:

```rust
ChangeData::Value(date_string) => {
    self.props.ref_date = NaiveDate::from_str(date_string.as_str()).unwrap();
```

Clear the date input (a single stray backspace on an empty field, or "pick a date"
→ cancel) and `from_str("")` returns `Err` → `unwrap()` panics → the wasm instance
traps. The DOM stays on screen looking healthy, but the app is dead:

```
th_before: 4 columns
click:     clicked:X   (remove-city "X")
th_after:  4 columns   ← no effect
verdict:   app is dead; every subsequent interaction is silently ignored
```

There is no error boundary, no recovery, no console message the user can act on.
A user who hits this concludes the site is broken and leaves. Same class of bug:
`tz.parse().unwrap()`, `from_local_date(..).unwrap()` (panics on DST-ambiguous
local times), `time.format("%H%M").parse().unwrap()`.

### The UX defect underneath all of that

The table is **transposed**. Rows are hours, columns are cities, and every cell
prints an absolute wall clock:

```
        Bengaluru      Mountain View   Shanghai       Singapore
0       9:00 AM 17 Oct 8:30 PM 16 Oct  11:30 AM 17 Oct 11:30 AM 17 Oct
1       10:00 AM 17 Oct 9:30 PM 16 Oct 12:30 PM 17 Oct 12:30 PM 17 Oct
...     (24 rows, user scans all of them, holding 4 clocks in working memory)
```

To answer "when can all four of us talk?", the reader must scan 24×4 = 96 cells,
notice that the *row index* is only meaningful for column 1, silently absorb that
Mountain View is on the previous day, and then mentally intersect four
working-day windows. That is arithmetic being done by a human that a computer
should do. The tool presents **data**; the user still has to produce the **answer**.

Supporting evidence from the live DOM/snapshot:

- search result buttons read `NEW DELHI,IN,ASIA/KOLKATA` — raw storage keys, uppercase,
  comma-joined, no offset, no live time, no way to disambiguate the 9 cities named
  "Springfield".
- `highlight_biz_time = false` — the working-hours feature is switched off in the
  shipped build, so the table is 96 undifferentiated cells.
- the date input renders empty (`mm/dd/yyyy`) while the table is computed from
  `today()`; nothing on screen tells you which date you're looking at.
- `tr:hover { background: #ffff99 }` — highlight on a *row of the table*, which in
  this transposed layout highlights "one instant", the least useful thing to highlight.
- the anchor is `tz.from_local_date(ref_date).and_hms(9,0,0)` on the **first city in
  the list** — so the whole grid silently re-anchors (and reorders its meaning) when
  you add or remove a row, and is anchored on a *local wall clock*, which is the
  classic DST-bug seed.
- no URL state. You cannot send the answer to anyone. There is no `.ics`. The task
  — "get the meeting on everyone's calendar" — is left 100% unfinished by the app.

---

## 1. The reframe

> Don't show times. Show **quality of times**. Answer "is this a good idea?",
> not "what time is it there?" — and then finish the job.

Three moves, in priority order:

1. **Transpose the layout.** One shared absolute time axis on *x*, one person per
   row, coloured by human cost. Overlap stops being something you compute and
   becomes something you *see*. (This is why World Time Buddy won; we have to match
   it here before anything else matters.)
2. **Add judgment.** A scoring engine that ranks candidate slots, explains itself
   per person, and reports the *worst-off* attendee, not just the average. Nobody
   does this well. It is also, conveniently, the hard algorithmic core — which is
   exactly where Rust should live.
3. **Close the loop.** `.ics`, a shareable link, and a Slack-ready text block, one
   click from the selected slot. Today the app ends mid-task.

### Jobs to be done (and today's coverage)

| # | job | today |
|---|---|---|
| 1 | "It's 3pm for me — is that reasonable for São Paulo?" | ✗ (must read a table) |
| 2 | **5-party standup: nobody before 7am or after 10pm** | ✗ (manual intersection) |
| 3 | "Put the thread's 3 people on a calendar" | ✗ |
| 4 | "On-call handoff Delhi↔Austin, and does it drift over the year?" | ✗ |
| 5 | "Family call India↔US — I know we're a day apart" | ✗ (date confusion) |
| 6 | "Conference talk in EU, when do I dial in?" | ~ |
| 7 | "My team's hours at a glance, shareable" | ✗ |
| 8 | "Deadline is EOD Friday Tokyo — when is that for me?" | ✗ |
| 9 | **"Is now a good time to ping them?"** | ✗ (needs 4 clicks + reading) |

Jobs 2, 3 and 9 are the whole product. Job 9 in particular is a one-glance answer
that no one serves well, and it's the reason people come back daily.

---

## 2. Information architecture

```
                                   ┌────────────────────────────┐
   URL hash is the single source   │  /#v=1&d=2026-03-03&dur=90 │  ← shareable, no backend
   of truth for all app state      │  &z=Asia%2FBengaluru&a=... │
                                   └─────────────┬──────────────┘
        ┌────────────────────────────────────────┼────────────────────────────────────┐
        ▼                                        ▼                                    ▼
┌───────────────┐              ┌────────────────────────────┐            ┌──────────────────────┐
│ 1. PLAN (grid)│──slot picked─▶ 2. DECIDE (ranked slots)   │──confirmed─▶ 3. ACT             │
│  day / 48h    │              │  score, fairness, reasons  │            │  .ics / link / text  │
│  3-day / week │              │  worst-off attendee flag   │            │  + per-attendee line │
└──────┬────────┘              └────────────────────────────┘            └──────────────────────┘
       │  ⌘K
       ▼
┌───────────────┐   save    ┌──────────────────┐   share read-only  ┌──────────────────┐
│ city / zone   │──────────▶│ GROUPS (roster)  │───────────────────▶│ /g/hq-sync  page   │
│ search + "me" │           │ localStorage     │                    │ (state in URL)     │
└───────────────┘           └──────────────────┘                    └──────────────────┘
       │
       ▼
┌───────────────┐
│ 4. CONVERT    │  instant one-off: "3pm BLR → SFO?"  (no grid, always 1 keystroke away)
└───────────────┘
```

Views: **Day** (default, 24 h) · **48 h** (for BLR↔SFO, where the only overlap sits
on the seam) · **3-day** · **Week strip** (recurring + DST drift) · **Roster**
(team home page) · **Convert** (instant, no grid).

---

## 3. Wireframes

### 3.1 Primary screen — desktop ≥ 1280 px

```
┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ⧗ Time to Meet      ▾ Bengaluru HQ (7)      ⌘K add person        ◐ dark   ?   ⧉ share   ⇩ ics                    │
├──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                                  │
│  ◀  Tue 3 Mar 2026  ▶   [Today]      duration ▾ [30m][1h][90m]▌[2h]     view ▾ [Day|48h|3d|Week]     ⌫ snap 15m  │
│                                                                                                                  │
│  your timezone: Asia/Kolkata · UTC+5:30 · now 14:32 ──────────────────────────────────────────── ▲ now          │
├──────────────────────────────────────────────────────────────────────────────┬─────────────────────────────────────┤
│      00        02        04        06        08        10        12        14│ 16        18        20        22   │
│      ┆         ┆         ┆         ┆         ┆         ┆         ┆         ┆│ ┆         ┆         ┆         ┆   │
│      ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒│▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒   │
│      ▒▒▒▒▒  E A R T H   S H A D O W   (real solar terminator)  ▒▒▒▒▒▒▒▒▒▒▒▒│▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒   │
│      ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒│▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒   │
│ ┌────────────┬───────────────────────────────────────────────────────────────┼───────────────────────────────────┐│
│ │ ▾ YOU      │████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒██████████││
│ │  BLR +5:30 │  22   01    04    07    10    13    16    19    22   01   04  │  07    10    13    16    19   22  ││
│ │  9–18 IST  │  ── your local day: Tue 3 Mar ───────────────────────────────▶│──────────────────────────────────▶││
│ │  🟢 8h     │████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒██████████││
│ ├────────────┼───────────────────────────────────────────────────────────────┼───────────────────────────────────┤│
│ │ ▾ Ana      │▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████│████████████▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓││
│ │  SFO −7    │  00   03    06    09    12    15    18    21    00   03   06  │  09    12    15    18    21   00  ││
│ │  8–16 PDT  │  ◀── Mon 2 Mar ────────────────────┴── Tue 3 Mar ─────────────▶│───────────▶ Wed 4 Mar            ││
│ │  🟡 6.5h   │        ✳ crosses midnight                 ▲ her now 02:32     │                                   ││
│ ├────────────┼───────────────────────────────────────────────────────────────┼───────────────────────────────────┤│
│ │ ▾ Ravi     │ … each row is 46 px; local hour ruler printed inside the row …│                                   ││
│ ├────────────┼───────────────────────────────────────────────────────────────┼───────────────────────────────────┤│
│ │ ▾ Mei      │                                                               │                                   ││
│ ├────────────┼───────────────────────────────────────────────────────────────┼───────────────────────────────────┤│
│ │ ＋ add     │                                                               │                                   ││
│ └────────────┴───────────────────────────────────────────────────────────────┴───────────────────────────────────┘│
│             ┌────────────────────────────────────┐                                                                   │
│             │▐ 14:15 – 15:45 IST  ✎  ✕           │  ← drag the body, grab either edge to resize, snap to 15 m       │
│             │ 🟢 4   🟡 2   🔴 0    score 87      │                                                                   │
│             └────────────────────────────────────┘                                                                   │
│  ▓ working   ░ shoulder   ▒ off-hours   █ asleep   ✳ crosses midnight   ⚠ DST change on this date                   │
├──────────────────────────────────────────────────────────────────────────────┬─────────────────────────────────────┤
│  ✨ BEST SLOTS (90 min)                                             sort ▾   │  ATTENDEES          fairness ▓▓▓▓░░  │
│   1  Tue 21:30–23:00 IST   94  🟢5 🟡1   SFO 08:00 · LHR 16:00 · SYD 02:30 🔴 │   ☑ You    BLR  9–18   🕘 now 14:32  │
│   2  Tue 22:00–23:30 IST   88  🟢4 🟡1 🔴1  ⚠ Mei past her end of day         │   ☑ Ana    SFO  8–16   🕘 now 02:32  │
│   3  Wed 08:15–09:45 IST   81  🟢6        ⚠ you before your start             │   ☐ Mei    PVI  7–15   flex ±1 h    │
│  show all 14 ▸                                                             │   ＋ per-person hours · weekend · role │
├──────────────────────────────────────────────────────────────────────────────┴─────────────────────────────────────┤
│  Tue 3 Mar · 21:30–22:45 IST        [ ⇩ .ics ]  [ ⧉ copy link ]  [ ¶ copy for Slack ]     tab title follows ▸ 21:30│
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Design notes for the grid:

- **Two clock rulers.** The top ruler is the shared axis in *your* zone; every row
  carries its own inline local-hour ruler. The transposition step disappears — you
  read each person's time in their own frame while everything stays aligned.
- **Row label column is sticky-left** and carries: name, city, UTC offset, working
  hours, and a live "their now" caret that drifts across their row in real time.
  Job 9 ("is now a good time to ping them?") is answered by a glance at that caret.
- **Midnight is drawn heavier than hours** and every row labels its own date
  (`Mon 2 → Tue 3`). Date confusion, not offset confusion, is the #1 real-world
  timezone error, and today's UI hides it inside `8:30 PM 16 Oct`.
- **Earth shadow.** A soft band across the whole grid computed from the real solar
  terminator for the selected date. It is beautiful, it is instantly legible, and it
  makes the tool feel alive — it drifts one pixel-width per minute.
- **Hover a column → vertical crosshair** + one tooltip listing every attendee's
  local time at that instant. One hover replaces the entire 96-cell scan.
- **Rendering cost:** each row's availability band is a pure function of
  (attendee prefs, date, tz) → render it as **one element with a CSS
  `linear-gradient` with hard colour stops**, not 96 cells. 7 attendees = 7 band
  nodes + a handful of overlay labels, so drag is free (the band DOM is never
  touched while dragging) and screen readers don't get 672 gridcells.
- **Keyboard:** `←/→` move selection by snap step · `Shift+←/→` resize · `+/-`
  duration · `⌘K` search · `⌘1..9` focus row · `D` date · `V` view · `?` shortcut
  sheet · `Enter` open the action sheet.

### 3.2 Zoomed anatomy of one row

```
   sticky label (168px)                         band (one element, gradient stops)                     right rail gutter
┌──────────────────────┬──────────────────────────────────────────────────────────────────────────────┬────────────┐
│ ▾ YOU                │░░░░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒████████████│  🟢 8h00   │
│  Bengaluru · +5:30   │ 22   01    04    07    10    13    16    19    22   01    04    07    10   13 │  in-hours  │
│  09:00–18:00         │ ┌──────────────────────────────────────────────────────────────────────────┐ │            │
│  🕘 14:32 now ─────▲  │ └ local hour ruler, tabular-nums, 11px, muted ─────────────────────────────┘ │  ⋮ edit    │
└──────────────────────┴──────────────────────────────────────────────────────────────────────────────┴────────────┘
      ▲ caret = their real current time, moves once a minute
      ▲ 2px gaps between 15-min bands so the row reads as a tape, not a spreadsheet
      ▲ colour is never the only signal: each band also carries a glyph + word on hover/focus
```

### 3.3 Mobile (390 px) — the layout that makes or breaks it

```
┌───────────────────────────────┐
│ ⧗ Time to Meet        ⌘K   ☰  │
├───────────────────────────────┤
│  ◀ Tue 3 Mar ▶   [Today]      │
│  90m ▾   Day ▾   snap 15m ▾   │
├───────────────────────────────┤
│  ┌ You · BLR ────────── 🟢 ─┐ │
│  │ 14:32  now · 9–18        │ │
│  │ ▁▁▂▃▇▇▇▇▇▇▇▇▇▇▇▇▄▂▁▁ 24h  │ │   ← each person = a card; the band is the
│  └──────────────────────────┘ │     same gradient, just narrower
│  ┌ Ana · SFO ────────── 🟡 ─┐ │
│  │ 02:32  asleep ✳ Mon 2    │ │   ← "asleep" as a WORD, not just a colour
│  │ ▇▇▇▇▇▄▂▁▁▁▁▁▁▁▁▂▃▇▇▇▇ 24h │ │
│  └──────────────────────────┘ │
│  ┌ Mei · PVRI ───────── 🟢 ─┐ │
│  └──────────────────────────┘ │
│  ＋ add person                 │
├───────────────────────────────┤
│  ┌─── selected slot ─────────┐│
│  │  ◀  21:30 – 22:45 IST  ▶  ││   ← stepper instead of drag on touch:
│  │  90m   🟢4 🟡2 🔴0  87     ││     15-min ± stepper + a scrub bar
│  │  worst: Mei 22:45 (+45m)  ││
│  └───────────────────────────┘│
├───────────────────────────────┤
│  [ ⇩ .ics ]   [ ⧉ link ]  [ ¶ ]│   ← sticky action bar, always reachable
├───────────────────────────────┤
│  People │ Slots │ Share        │   ← rail becomes bottom tabs
│  ───────┴───────┴──────        │
└───────────────────────────────┘
```

Mobile rules: the grid keeps one shared axis but degrades to **cards + a scrub
stepper** rather than a horizontally-scrolling 96-column monster (horizontal scroll
inside a vertical page on touch is a trap). Optional "full grid" toggle for power
users, with sticky row labels and a scroll-shadow affordance. Drag is replaced by
tap-to-place + 15-min steppers, because 15 px/hour is below the touch target floor.

### 3.4 Slot action sheet — where the loop closes

```
                    ┌──────────────────────────────────────────────────────────┐
                    │  Tue 3 Mar 2026 · 16:00 UTC                          ✕   │
                    ├──────────────────────────────────────────────────────────┤
                    │  ┌ You ────────────────────────────────────────────────┐ │
                    │  │ Tue 3 Mar   21:30 – 22:45   IST (UTC+5:30)   🟢 work│ │
                    │  │ Ana         Tue 3 Mar   08:00 – 09:15   PDT (−7)    │ │
                    │  │ Ravi        Tue 3 Mar   19:00 – 20:15   GST (+4)    │ │
                    │  │ Mei         Wed 4 Mar   00:00 – 01:15   JST (+9)  🔴│ │  ← date rolls for her
                    │  └─────────────────────────────────────────────────────┘ │
                    │  ⚠ Wed 4 Mar for Mei · crosses her midnight              │
                    │  fairness  ▓▓▓▓▓▓▓░░░  72   worst-off: Mei (+75m past EOD)│
                    ├──────────────────────────────────────────────────────────┤
                    │  [ ⇩ Download .ics ]  [ ⧉ Copy link ]  [ ¶ Slack text ]  │
                    │  ☑ include attendee local times in description           │
                    │  ☑ add attendees as participants (mailto-free)           │
                    │  link: timetomeet.shantanugoel.com/#v=1&d=2026-03-03…    │
                    └──────────────────────────────────────────────────────────┘

ICS notes (the details that actually matter):
  • DTSTART/DTEND in UTC form (…T160000Z) → no VTIMEZONE block, no client bugs
  • DTSTAMP + stable UID (so re-downloading updates rather than duplicates)
  • DESCRIPTION carries every attendee's local time, pre-formatted, plus the link
  • SEQUENCE/UID derived from a hash of the slot → idempotent
  • no ORGANIZER unless the user opts in (privacy: it's a static site, we don't know them)
```

### 3.5 Roster view (the retention hook + the distribution engine)

```
┌───────────────────────────────────────────────────────────────────────────────────────────────┐
│ ⧗ Bengaluru HQ · 7 people                    [ ⧉ share this page ]  [ ✎ edit ]  [ + add ]      │
├───────────────────────────────────────────────────────────────────────────────────────────────┤
│        00        06        12        18        00        06        12        18        00      │
│ You  BLR ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒  │
│ Ana  SFO ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒  │
│ Ravi DXB ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒  │
│ Mei  TYO ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒  │
│        ──────────────────────────────────────────────────────────────────────────────────────  │
│        common window:  Tue 13:30–15:00 IST  (nobody before 07:00, nobody after 21:00)  🟢 4/4  │
├───────────────────────────────────────────────────────────────────────────────────────────────┤
│  ☀ golden hour: 13:30–15:00 IST   ·   ⚠ 2 people's DST shifts before Apr 30 — recheck         │
└───────────────────────────────────────────────────────────────────────────────────────────────┘
   read-only link  →  embeddable:  <iframe src="…/g/hq-sync?embed=1" height="220">
```

The embeddable roster is the growth loop: every team that puts one on their wiki
hands out the domain.

### 3.6 ⌘K search — the single most-used surface

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ │blr                                                  ▲ ↑ ↓ ⏎ add  ⇧⏎ replace│
├─────────────────────────────────────────────────────────────────────────────┤
│  ➤ Use my timezone:  Asia/Kolkata · IST · 14:32                             │
│  ─────────────────────────────────────────────────────────────────────────  │
│  Bengaluru        IN  IST +5:30   🕘 14:32 Tue   🌅 06:18  🌇 18:12          │
│  Belgaum          IN  IST +5:30   🕘 14:32 Tue                               │
│  Berlin           DE  CET +1:00   🕘 10:02 Tue   🌅 07:04  🌇 17:38          │
│  Bilbao           ES  CET +1:00   🕘 10:02 Tue                               │
│  ── IANA zones ──                                                             │
│  Asia/Colombo     LK  +5:30       🕘 14:02 Tue                               │
└─────────────────────────────────────────────────────────────────────────────┘
   • fuzzy + prefix + transliteration + common misspellings ("blr", "bengalru", "hyderbad")
   • the live clock in each row is the confirmation that you picked the right city
   • recent & pinned first; results are ranked by population then string score
   • no raw "City,COUNTRY,IANA_ZONE" strings anywhere in the UI, ever
```

---

## 4. The scoring engine (the reason this beats the incumbents)

Pure function, pure Rust, unit-testable to death. For candidate window `(t, d)`:

```
cost_i(t) = piecewise over *their* local time-of-day and weekday:

   within stated working hours              0.00
   shoulder  (work ±1 h, or declared flex)  0.15 … 0.35
   07:00–09:00 or 18:00–22:00               0.45 … 0.70
   05:00–07:00 or 22:00–24:00               0.80 … 1.00
   00:00–05:00                              1.30 … 1.80   (never just "high": asymmetric)
   weekend / local-holiday weekday          +0.60   (Fri-Sat / Sat-Sun / Sun-Thu per locale)
   meeting clips across their midnight      +0.35
   partial overlap (they'd have to leave early) proportional, up to +0.80

weight_i   organizer 1.0 · required 1.0 · optional 0.3 · "nice to have" 0.15

aggregate    total      = Σ w_i · cost_i            (how bad on average)
             worst_off  = max_i cost_i              (how bad for the one person who suffers)
             equity     = 1 − stdev(cost_i) / 1.8   (is the pain shared or dumped on Mei?)
             coverage   = Σ w_i · in_hours_i / Σ w_i

rank by      score = 100 · ( 0.45·(1−worst_off/1.8) + 0.30·(1−total/W) + 0.15·equity + 0.10·coverage )
```

The important editorial choice: **`worst_off` dominates.** A slot that is fine for
four people and 03:00 for one is a bad slot, and average-based tools (and humans
doing mental arithmetic) systematically miss this. The UI must therefore always
surface *who* is worst off and *by how much* — that sentence is the product.

Every ranked slot returns per-person reasons, not just a number:

```
#2  Tue 22:00–23:30 IST   score 88   🟢4 🟡1 🔴1
    └ Mei (JST): 00:00–01:15 — starts at her midnight, 75 min past her stated end of day
    └ Ana (PDT): 08:30 — 30 min before her stated start (she declared ±1h flex) ✓
```

DST correctness rule, non-negotiable: **the engine iterates absolute UTC instants
and converts down to local**, never the other way round. Today's code anchors on
`from_local_date(first_city).and_hms(9,0,0)` — local → absolute — which is precisely
how timezone bugs are born. Golden tests must include: Lord Howe (+10:30/+11, 30-min
DST), Australia/Lindeman, America/Santiago, Asia/Tehran (+4:30), Asia/Kolkata
(+5:30), Pacific/Kiritimati (+14), Pacific/Apia (skipped a whole day in 2011),
Asia/Kashmir, and both the spring-forward gap and the fall-back ambiguity.

---

## 5. Visual design language

**Direction:** *mission control meets editorial.* Dense, quiet, precise — a
professional instrument, not a dashboard toy. The one loud element is the earth
shadow; everything else stays out of its way.

```
   dark (default)                                   light (not an afterthought)
   ─────────────────────────────────────────────    ─────────────────────────────
   bg      oklch(17% .02 265)   #0B0E14            bg      oklch(98% .004 265)
   surface oklch(21% .02 265)                      surface oklch(95% .006 265)
   hair    rgba(255,255,255,.07)                   hair    rgba(0,0,0,.08)
   text    oklch(93% .01 265)                      text    oklch(22% .02 265)

   working   oklch(72% .13 175)  ▓ teal            (luminance-ordered, so the ramp still
   shoulder  oklch(80% .14  85)  ░ amber            reads in greyscale & in forced-colors)
   off       oklch(45% .06 265)  ▒ slate-violet
   asleep    oklch(28% .07 275)  █ indigo + ✦      (never red-only for bad)
   danger    oklch(65% .19  15)  vermilion + ⚠
   select    oklch(88% .16 195)  cyan, 1px bright top edge + 0 0 24px glow
```

- **Type:** UI in a variable grotesque-sans (Inter Tight / Geist / Söhne). All clocks
  in a mono with `font-variant-numeric: tabular-nums` — non-negotiable; digits that
  jitter while you drag destroy the entire feel of the tool. Scale 11/12/13/15/20/28,
  tight tracking on numerals, generous on prose. `text-wrap: balance` on headings.
- **Shape:** 10 px cards, 4 px bands, 2 px gutters, 1 px hairlines only. No drop
  shadows except the selection glow. No glassmorphism.
- **Motion:** FLIP when rows reorder/add; selection window springs with rubber-banding
  at the axis ends; earth shadow drifts once a minute; date changes use
  **View Transitions** (cross-fade + 8 px x-slide); a one-shot shimmer + "golden
  hour" label when a slot is perfect for everyone. Everything behind
  `prefers-reduced-motion`.
- **Modern CSS, no framework:** CSS Grid + subgrid for the axis alignment, container
  queries for the rail (the rail is a component, not a breakpoint), `color-mix()`
  for the band ramp, `light-dark()`, logical properties only (RTL for free),
  `@supports` for progressive enhancement, `text-wrap: pretty`, `scrollbar-gutter`.
  No Tailwind, no runtime CSS-in-JS — the CSS is ~14 KB and it is the design system.
- **Details that signal craft:** tab title mirrors the selection (`21:30–22:45 IST ·
  4 ok — Time to Meet`); favicon becomes a mini clock at the selected hour; sunrise/
  sunset glyphs per city; a `⚠ DST` flag appears on the date axis the moment the
  selected day contains a transition for any attendee; numbers roll during drag;
  `forced-colors` mode swaps fills for outlined blocks + glyphs.

### Accessibility (a real requirement, not a section)

- `role="grid"` / `row` / `gridcell`, each cell `aria-label` = `"Ana, San Francisco,
  08:00 Monday 2 March, within working hours"`.
- Plus a visually-hidden real `<table>` summary so a screen-reader user gets the
  whole answer in one read instead of traversing 672 cells.
- Quality encoded by **luminance + glyph + word**, never hue. ≥4.5:1 text, ≥3:1 fills.
- Everything reachable by keyboard; `:focus-visible` rings tuned for dark; skip link;
  44 px touch targets; `aria-live="polite"` on the verdict line so the score is
  announced as you drag.
- Honours `prefers-color-scheme`, `prefers-reduced-motion`, `prefers-reduced-transparency`,
  `forced-colors`, and `text-spacing`. 200 %-zoom layout survives.
- i18n-ready: per-person 12/24 h, locale date formats, first-day-of-week, per-locale
  weekend, transliterated city search.

---

## 6. Rust architecture

Three credible paths, honestly weighed:

| | A. modernize in place | B. Leptos/Dioxus CSR | **C. Rust core + thin shell** |
|---|---|---|---|
| stack | Yew 0.21 hooks | Leptos 0.8 `csr` | `tt-core` → wasm + HTML/CSS/TS glue |
| diff from today | small | full rewrite | full rewrite |
| wasm cost | ~350 KB gz | ~300 KB gz | **~120–200 KB gz** |
| 60 fps drag | fights the VDOM | good, still closures | native pointer events, band DOM untouched |
| testability of logic | medium (needs `wasm-bindgen-test`) | medium | **excellent: `cargo test` on host, no browser** |
| free by-products | – | – | **`tt` CLI, `tt-core` on crates.io, server-side preview/OG** |
| "100 % Rust in the browser" | ✔ | ✔ ✔ | ✘ (Rust does the thinking, not the painting) |
| ceiling | medium | high | highest |
| effort | M | L | L |

**Recommendation: C.** For a pointer-heavy, 60 fps, drag-and-scrub instrument, the
view layer wants raw DOM + real CSS + real pointer events, and the *hard* part of
this product is the time math and the optimizer — which is exactly what Rust is
superb at and what deserves to be tested on the host with `cargo test`. Rust stays
the identity of the project (the tagline becomes *"Rust engine, 200 KB, works on a
plane"*), and the same core then powers a CLI and a published crate.

If "every pixel rendered by Rust" is a requirement you want to keep, take **B** and
accept ~150 KB extra and `wasm-bindgen-test` for the logic.

```
   ┌─────────────────────────────  cargo workspace  ─────────────────────────────┐
   │                                                                             │
   │  crates/tt-core            pure Rust, no wasm, no DOM, no js_sys            │
   │   ├─ instant.rs    absolute UTC instant; never anchor on wall clock         │
   │   ├─ zone.rs       IANA offsets + DST (chrono-tz behind a facade; one API)  │
   │   ├─ person.rs     city, tz, working hours, flex, role, weekend locale      │
   │   ├─ day.rs        band decomposition → [(start,end,quality)]  (gradient!)  │
   │   ├─ score.rs      cost/aggregate/rank + per-person reasons                 │
   │   ├─ solar.rs      sunrise/sunset/terminator (NOAA, ~40 lines)              │
   │   ├─ search.rs     compiled city index, prefix+fuzzy, pop-ranked            │
   │   ├─ ics.rs        RFC 5545 output, UTC form, stable UID                    │
   │   └─ url.rs        versioned state codec (#v=1&…)  ← round-trip fuzz-tested │
   │        ▲                       ▲                        ▲                  │
   │        │                      │                        │                  │
   │  crates/tt-wasm        crates/tt-cli              crates/tt-dataset        │
   │   wasm-bindgen facade   `tt plan "blr,sfo,tyo"`     build-dataset:         │
   │   ~6 typed fns,         `tt now -z Asia/Kolkata`    GeoNames + IANA →      │
   │   serde-wasm-bindgen    `tt slots --dur 90 --json`  15k cities →           │
   │   (no DOM at all)       `tt ics -o m.ics`           80 KB brotli blob      │
   │                                                                             │
   │  app/   index.html · style/*.css (~14 KB) · src/*.ts (~10 KB glue)          │
   │         owns: DOM, pointer/drag, URL hash, localStorage, SW, a11y           │
   └─────────────────────────────────────────────────────────────────────────────┘

   wasm API surface (deliberately tiny — 6 functions, all pure):
     init(dataset: &[u8]) -> void
     build_day(day, people) -> Vec<Band>          // → CSS gradient stops
     score_window(instant, dur, people) -> Verdict // score, worst_off, reasons[]
     suggest(day, dur, people, k) -> Vec<Slot>     // ranked best slots
     search(query, k) -> Vec<City>                 // with live offsets
     render_ics(slot, people, opts) -> String
```

### Dataset (kills ~142 KB gz and the boot stall)

Today: 24,568-row CSV `include_str!`'d, then 24,568 `SimSearch::insert` calls before
first paint. Instead: `tt-dataset` compiles GeoNames `cities15000` (public domain,
CC-BY 4.0 — needs the attribution line in the footer) + the IANA zone list into a
**pre-sorted, prefix-bucketed binary blob** with `name_id/country/tz_id/lat/lon/pop`,
brotli'd to **≤ 80 KB**, fetched as a separate stream so it can be cached
independently of the engine. Search becomes an O(prefix) binary scan over typed
arrays — no trie construction at runtime, sub-millisecond results, no boot stall.
Arbitrary IANA zones stay searchable (`Asia/Colombo`), plus a
**"Use my timezone"** button from `Intl.DateTimeFormat().resolvedOptions().timeZone`.

### Performance budgets (enforced in CI, not aspirational)

| metric | today | budget |
|---|---|---|
| first-visit transfer | ~415 KB gz | **≤ 250 KB gz / ≤ 180 KB br** |
| wasm transfer | 410 KB gz | ≤ 150 KB gz |
| hashed-asset cache | `max-age=600` | `max-age=31536000, immutable` |
| LCP (Moto G / Slow 4G) | ~4–6 s | **< 1.2 s** |
| TBT | main-thread trie build | **< 100 ms** |
| CLS | 0 (nothing moves) | < 0.05 |
| search keystroke → results | unmeasured | < 8 ms |
| drag frame budget | n/a | 16 ms, band DOM never touched |

Wasm-free first paint: a 2 KB inline script renders the shell + "your timezone:
Asia/Kolkata · 14:32" and the date, so the site is *useful* before the engine
arrives; the grid hydrates in. `<link rel=modulepreload>` + brotli-precompressed
assets + `Link:` headers from `_headers`.

---

## 7. Deployment

Zone `shantanugoel.com` is on Cloudflare nameservers (ashley/ivan) **[measured]**,
`timetomeet` is a proxied record whose origin is GitHub Pages (`x-github-request-id`
present) **[measured]**, and wrangler is authenticated with `pages: write` and
`zone: read` **[measured]**. Three existing Pages projects already live in the account.

**Move to Cloudflare Pages, keep GitHub Pages as the instant rollback.**

```
                    ┌──────────── GitHub push / PR ────────────┐
                    ▼                                          ▼
        ┌───────────────────────┐                 ┌──────────────────────────┐
        │ GitHub Actions        │                 │ CI: cargo fmt/clippy/test │
        │  trunk build --release│                 │  trunk build --release    │
        │  brotli precompress   │                 │  + Lighthouse budget gate │
        │  write _headers       │                 │  + wasm-bindgen-test      │
        └───────┬───────┬───────┘                 └──────────────────────────┘
                │       └──────────────▶ dist/ ──▶ wrangler pages deploy
                ▼                                    │        │
   ┌────────────────────────┐      preview deploys ◀─┘        └──▶ production
   │ GitHub Pages (CNAME)   │      <hash>.timetomeet-rs              timetomeet
   │  kept as fallback      │      .pages.dev (per PR)              .pages.dev
   └───────────┬────────────┘                                        │
               │                                                     │
        DNS: timetomeet.shantanugoel.com  (proxied, Cloudflare) ──────┘
        flip CNAME target → timetomeet.pages.dev  ·  rollback = flip it back
```

Why Pages over a Worker: it's static, so Pages gives brotli, per-PR preview URLs
(invaluable for a visual tool), `_headers`/`_redirects`, and no per-request cost.
Keep the `CNAME` file so GitHub Pages never breaks. A **Worker earns its place
later**, for exactly two things: per-link OG/social images, and an optional
`/api/resolve` short-link service if we ever want short URLs. Ship it static first.

`_headers`:
```
/*
  Cache-Control: no-cache
  Content-Security-Policy: default-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'none'
  Cross-Origin-Resource-Policy: same-origin
  Referrer-Policy: same-origin
  X-Content-Type-Options: nosniff
  Permissions-Policy: camera=(), microphone=(), geolocation=()
/*.js *.wasm *.css *.data
  Cache-Control: public, max-age=31536000, immutable
  Cross-Origin-Resource-Policy: cross-origin
```

PWA: manifest + a hand-rolled ~1 KB service worker (precache the shell,
stale-while-revalidate the dataset, **network-first for HTML** so we never serve a
stale app). Installable, works offline on a plane — a genuine feature for a travel
tool, and free on static hosting.

Analytics: Cloudflare Web Analytics or Plausible (no cookie banner, no consent
banner — which is itself a UX win). Track: slots copied, `.ics` downloads, people
added per session, mobile share. "Just about usable" only becomes "really useful"
if we can see whether people finish the task.

---

## 8. Phased plan — each phase ships to the live domain on its own

**P0 · stop the bleeding — S (a weekend), no redesign required**
1. Remove every `unwrap()` on user input; invalid date → keep previous value, show inline hint.
2. Show the effective date; prefill the date input with `today()`.
3. Result buttons: `Bengaluru · IN · IST +5:30 · 14:32` instead of `BENGALURU,IN,ASIA/KOLKATA`.
4. Turn `highlight_biz_time` on (it's already written!) and actually finish the 2-line version of it.
5. `Cache-Control: immutable` on hashed assets + brotli precompression (CF Pages or a Pages `_headers`).
6. `docs/` out of git → CI-built. Add `Trunk.toml`, `rustfmt.toml`, clippy `-D warnings`, GitHub Actions, and the first 20 unit tests (DST goldens).
7. Add `<meta name="color-scheme">`, a real favicon/OG card, and a 1-line subtitle.

**P1 · the Grid — L.** Transpose to the horizontal timeline. Per-row local rulers,
working-hours bands, earth shadow, sticky labels, live "their now" carets, drag +
resize + snap, hover crosshair with all-local-times tooltip, mobile card mode, URL
hash state + copy link. *This phase alone takes it from "just about usable" to
"better than World Time Buddy for planning".*

**P2 · judgment — M.** `tt-core` scoring engine, ranked slots panel, fairness meter,
per-person reasons, per-person working hours/flex/role/weekend, saved groups in
`localStorage`, `⌘K` palette + compiled dataset.

**P3 · close the loop — M.** `.ics`, Slack/Teams text block, read-only shared
roster link, Roster view, PWA/offline, embeddable widget.

**P4 · depth & reach — M.** 48 h/3-day/week views, recurring-meeting DST-drift
warnings, sunrise/sunset + solar terminator, `tt` CLI, `tt-core` on crates.io,
i18n, CF Worker for OG images per shared link, Lighthouse budget in CI.

**What gets cut:** the "How to use this" wall of text (→ one coachmark + `?` sheet;
a good tool is self-evident), the 24-row table, raw IANA strings in the UI, the CSV
as a source file, and `docs/` in git.

---

## 9. Locked decisions (2026-10-17)

| decision | choice |
|---|---|
| view layer | **C — `tt-core` in Rust → wasm (6 pure fns) + ~14 KB CSS + ~10 KB TS glue.** Rust does the thinking, the platform does the painting. |
| first ship | **Everything, P0→P3** in one production cutover (see §10 for how the blind window is de-risked) |
| hosting | **Cloudflare Pages primary, GitHub Pages kept as one-record rollback** |
| data | **100 % static** — state in the URL hash, teams in `localStorage`, read-only rosters as long links. No backend, no accounts, no cost, no privacy policy. |
| dataset | GeoNames `cities15000` (CC-BY 4.0 → footer attribution line) — open, ~15 k cities, ~80 KB brotli |

## 10. Build plan for P0→P3

**De-risking "nothing ships until the end".** One production cutover is right for the
domain, but we will not fly blind: every milestone below deploys to a **Cloudflare Pages
preview URL** (`https://<hash>.timetomeet-rs.pages.dev`) so it can be opened on a phone
the day it lands. `timetomeet.shantanugoel.com` keeps serving the current site (plus the
§0.1 stopgap) until M8 flips the DNS record. Rollback after cutover = flip one record.

```
M0  scaffold        workspace, tt-core types, CI (fmt/clippy/test), Pages preview pipeline,
    (S)             brotli precompress + _headers.  <- proves the deploy path end-to-end
    |
M1  time engine     tt-core: Instant/Zone/day-band decomposition. DST golden tests:
    (M)             Lord Howe, Lindeman, Santiago, Tehran, Kolkata, Kiritimati, Apia,
    |               spring-forward gap, fall-back ambiguity. cargo test on host.
    |
M2  dataset         tt-dataset: GeoNames+IANA -> prefix-bucketed blob <=80 KB br.
    (S)             tt-core::search over typed arrays.  K-palette with live clocks.
    |
M3  the grid        horizontal timeline, per-row local rulers, bands as CSS gradients,
    (L)             sticky labels, live "their now" carets, earth shadow, hover crosshair.
    |               wasm-free first paint.  <- first moment it feels like a new product
    |
M4  interaction     drag/resize/snap, keyboard model, touch scrub stepper, aria-live verdict.
    (M)             60 fps budget: band DOM never touched while dragging.
    |
M5  judgment        score.rs: cost/aggregate/rank + per-person reasons. ranked slots panel,
    (M)             fairness meter, worst-off sentence. per-person hours/flex/role/weekend.
    |
M6  loop closure    url.rs codec (round-trip fuzz) + copy link, ics.rs (UTC form, stable
    (M)             UID), Slack text block, localStorage groups, Roster view, embed.
    |
M7  polish          PWA/SW, light+dark, a11y pass (screen-reader summary table, forced-
    (M)             colors, 200% zoom), motion pass behind prefers-reduced-motion,
                    Lighthouse budget gate in CI (<=250 KB gz, LCP<1.2s, TBT<100ms).
    |
M8  cutover         GH Pages build kept alive, Pages custom hostname, flip DNS, watch,
    (S)             delete the stopgap, write it up.
```

Order rationale: M0 first because the deploy/cache path is on the critical path and is
cheap to prove. M1/M2 before M3 because the grid is a pure function of the time engine
and the dataset — building the view on an unproven engine means rewriting it twice.

## 0.1 Stopgap for the live panic (built + verified, NOT yet deployed)

The old tree **cannot be rebuilt**: Yew 0.18 pins `wasm-bindgen 0.2.74`, which rustc
1.99 hard-rejects —

```
error: older versions of the `wasm-bindgen` crate are incompatible with
current versions of Rust; please update to `wasm-bindgen` v0.2.88
```

so the panic is not fixable by a code change. It *is* fixable in the committed static
HTML, because the browser coerces any invalid `input[type=date]` value to `""` — exactly
the string the panic keyed on. A capture-phase guard on `document` runs before Yew's
listener on the input, so the empty value never reaches the wasm.

**Reproduction on the pristine committed artifact** (`git show HEAD:docs/index.html`,
served locally, aliveness tested with trusted clicks only):

```
responsive on fresh load:      4 -> 3 columns   ok
<empty the date input, dispatch change>
responsive afterwards:         3 -> 3 columns   DEAD, permanently
console: panicked at 'called `Result::unwrap()` on an `Err` value:
         ParseError(TooShort)', src/model.rs:72:85
```

**Same sequence against the patched artifact:**

```
date value after empty attempt: "2026-10-18"   (guard restored it)
removals after the attempt:     4->3, 3->2, 2->1   all responsive
wasm panic messages:            0
verdict: GUARD VERIFIED
```

**Correction to §0:** the `X` remove is *not* broken. My first probe used a synthetic
`element.click()`, which does not reliably drive this build's Yew listener; with a
trusted click, removal works (4->3->2). Two things were conflated: (a) the real,
permanent, date-triggered wasm trap, and (b) my own unreliable probe method. Only (a)
is a product defect. Recorded so nobody chases a phantom.

A "the wasm trapped" watchdog banner was written, tested, and **removed**: it had to
guess aliveness from render timing and false-alarmed on a healthy app. The guard closes
the only UI-reachable trap, so the heuristic bought risk rather than safety.
---

# Part 5 — Delivery report (M0–M7 shipped to preview)

Preview: **https://timetomeet-42n.pages.dev** · production
(`timetomeet.shantanugoel.com`) is still the guarded legacy build, untouched.

## What was built

```
crates/tt-core     pure Rust engine: instants, zones, bands, scoring, solar, ICS, URL state
crates/tt-wasm     thin JSON facade, 9 exports, no DOM
crates/tt-cli      the same engine in a terminal (dogfooding harness, and a real feature)
crates/tt-dataset  GeoNames + IANA -> flat binary index
app/               index.html, one stylesheet, two ES modules
tools/             check-styles.mjs, check-wasm-abi.mjs, smoke.mjs
```

Measured, not estimated:

| | legacy | rewrite |
|---|---|---|
| first paint over the wire | 410 KB gz | **181 KB br** |
| engine | 410 KB wasm, unbuildable | 158 KB br wasm |
| city index | none | 460 KB, lazy, on first search |
| tests | 0 | 45 |
| CI | none | fmt + clippy -D warnings + test + contracts + budgets |
| date input | permanently traps the wasm | cannot trap the engine |

## Decisions the plan got wrong

**The earth shadow was cut.** §4 called for one night-side gradient across the grid. It is
semantically impossible: the terminator sits at a different x for every row, because every
row is a different zone. Rendered, it read as a grey box over the data. The per-row
sunrise/sunset glyphs say the same thing truthfully, and the bands already encode who is
asleep. `darkness_profile` stays in the engine — it is correct and tested — it just is not
painted, because there is no honest single place to paint it.

**Weekend dates were a first-run bug, not an edge case.** The environment's "today" is
Sunday, so the first thing any reviewer saw was a grid where nobody works and six
equally-bad suggestions ranked like advice. Two fixes: the engine widens the suggestion
horizon to 72h when half the weighted team rests that day and labels the day each
suggestion belongs to; and a fresh visitor lands on the next working day (a shared link
always wins — the sender picked that moment deliberately).

**Opening on 09:00 was wrong.** The default selection scored 26/100 next to a 68/100
suggestion, so the tool opened by arguing with the user. It now opens on the best slot for
that team, and ◀ ▶ re-picks per day.

**Numbers needed words.** "58" invites a search for a scale. `score_label` says
"workable".

## Bugs found by testing, all of them invisible in unit tests

1. `#[serde(rename_all = "lowercase")]` makes `OffHours` -> `"offHours"` != `"offhours"`.
   One undefined `var()` invalidates the whole `background-image`, so every band rendered as
   an empty white row with no error anywhere. Fixed by naming the CSS custom properties
   after the wire values, so there is no mapping layer to get wrong.
2. `wasm_bindgen` maps `i64` to BigInt. `search_cities(query, k, now)` was passed a Number,
   which threw inside a promise: the palette silently showed "no results" for a query the
   engine answers correctly. Caught by `tools/check-wasm-abi.mjs`, which now reads the Rust
   signatures and every JS call site and fails the build.
3. `_headers` had `Content-Type: application/wasm` on `/pkg/*`, which also matched the
   JS glue. The browser refused to `import()` it: blank page, one vague console error,
   and it only happened in production. Caught by `tools/smoke.mjs`, which now runs after
   every deploy and asserts the content types the CDN actually returned.
4. The asset version hash covered only the sources, so a Rust-only change republished a new
   wasm under an unchanged URL — invisible to every client, because `/pkg/*` is immutable.
   The hash now covers the built wasm, the built dataset, `_headers` and `sw.js`.
5. `Escape` was swallowed when focus was in a field, so a dialog could only be closed by
   clicking. The universal dismiss now runs before the "don't hijack typing" guard.
6. Absolutely-positioned layers without `inset-inline-end` shrink to zero width, so every
   percentage inside them resolved against nothing: the selection collapsed to a 2px line
   at x=0.
7. Hour labels were emitted at a fixed density, so on a 230px phone grid 24 labels of 22px
   overlapped into a smear. Density is now derived from the measured band width.

Also: five files silently contained a stray `</content>` tag from my own tool calls, and one
patch modified the CSS string and never wrote it back. Both looked like "the CSS is
ignored". `node --check` and the style contract now exist for that reason.

## Known gaps, with numbers

- **The city index is not compressed in transit.** Cloudflare Pages does not compress
  `.bin`, so the first search transfers 460 KB instead of the 216 KB it is brotli'd to on
  disk. It is lazy, cached for 7 days, and off the first-paint path. Two candidate fixes:
  ship it as a `.js` module holding base64 (CF compresses `.js`; ~220 KB on the wire), or
  raise the population floor to 100k (6 283 cities, ~240 KB raw). Neither is free; neither
  was worth blocking the cutover.
- **Mobile is a horizontally-scrolling grid with sticky labels**, not the card layout in
  §4.5. It is usable (tap a band to place the meeting, tap a suggestion to accept it) and it
  is what World Time Buddy ships. The card layout is unbuilt.
- **No `sitemap.xml`** although `robots.txt` points at one.
- **Cutover happened; see below.** Rollback is a DNS edit back to GitHub Pages.

## Cutover (done)

`timetomeet.shantanugoel.com` now serves the rewrite from Cloudflare Pages. Verified on the
real domain rather than a preview: 15/15 smoke checks, rows + slots + verdict, lazy dataset
("12323 cities · 597 zones"), a deep link (`#v=1&d=2026-12-25&z=Europe/Stockholm&dur=90&s=…`)
restoring date, duration and selection, `Escape` closing the palette both from the button and
with the caret in the field, zero console errors, and no `[data-yew-id]` anywhere.

The token had `pages: write` and `ssl_certs: write` but **no DNS scope at all** — even
*reading* the record returned `Authentication error`. Attaching the custom domain from the
Pages dashboard did the DNS record and the certificate in one step, which is the better
order anyway: editing DNS first leaves a window where the name resolves somewhere Pages does
not serve.

**Decision reversed: the stopgap stays.** The plan said delete the `docs/index.html` guard at
cutover. That was wrong. `docs/` is now the rollback artifact, and the guard is the only
thing between a blank date input and a permanently dead page *if the record is ever pointed
back at GitHub Pages*. It is twelve lines and fires only on an empty date. It gets deleted
together with the GitHub Pages deployment, not before — the marker comment now says so.

Rollback: point the `timetomeet` record back at `CNAME shantanugoel.github.io`, Proxied. Back
in about a minute. Leave the GitHub Pages deployment alone for a week.

Two traps found on the way, both invisible in a desktop screenshot:

1. `.actionbar` had `position: sticky; inset-block-end: 0` on mobile and did nothing,
   because the same media query set `body { overflow: auto }`. That makes `<body>` its own
   scroll container with a height equal to its content, so nothing scrolls *inside* it and
   sticky-bottom has no scrollport to stick to — `Copy link`, the primary CTA, sat below the
   fold. `html, body { overflow: visible }` hands scrolling back to the viewport.
2. Three of my own verification probes were wrong and passed anyway: `#actionbar` does not
   exist (it is a class), `element.hidden` is not how the palette closes, and `scrollWidth`
   clamps to the content box under `overflow: hidden` so it can never report clipping. The
   clip check now compares the last visible child's right edge against the label's content
   box. Nothing here is believed unless the probe could have failed.
- `Asia/Calcutta` is shown as typed rather than normalised to `Asia/Kolkata`.

## What the guard is for

`docs/index.html` still carries the date-input stopgap on production. It is deleted by the
cutover commit, not before: it is the only thing standing between a mistyped date and a
permanently dead page for every visitor until DNS moves.
