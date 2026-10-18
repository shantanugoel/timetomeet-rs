/* tslint:disable */
/* eslint-disable */

/**
 * One day (or several) of grid rows: bands, local rulers, midnights, solar, DST flags.
 */
export function build_day(date: string, viewer_tz: string, people_json: string, now: bigint, span_days: number): string;

/**
 * URL hash -> state JSON. Never fails; unknown or malformed fields are dropped.
 */
export function decode_state(hash: string): string;

/**
 * URL hash from app state (JSON of `{anchorDate,viewerTz,durationMin,startUtc,people,groupName}`).
 */
export function encode_state(state_json: string): string;

/**
 * Load the compiled city index. Returns `{"cities":N,"zones":M}` or `{"error":...}`.
 */
export function init_dataset(bytes: Uint8Array): string;

/**
 * RFC 5545 payload as a string.
 */
export function render_ics(start_utc: bigint, duration_min: number, viewer_tz: string, people_json: string, opts_json: string): string;

/**
 * Slack/Teams/email plain text.
 */
export function render_text(start_utc: bigint, duration_min: number, viewer_tz: string, people_json: string, link: string): string;

/**
 * Score one window. Called on every pointermove while dragging, so it stays cheap.
 */
export function score_window(start_utc: bigint, duration_min: number, viewer_tz: string, people_json: string): string;

/**
 * City / zone search with live offsets. Empty result if the dataset is not loaded yet.
 */
export function search_cities(query: string, k: number, now: bigint): string;

/**
 * Ranked candidate slots.
 */
export function suggest_slots(date: string, viewer_tz: string, people_json: string, now: bigint, duration_min: number, k: number, span_hours: number): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly build_day: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: bigint, i: number) => void;
    readonly decode_state: (a: number, b: number, c: number) => void;
    readonly encode_state: (a: number, b: number, c: number) => void;
    readonly init_dataset: (a: number, b: number, c: number) => void;
    readonly render_ics: (a: number, b: bigint, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => void;
    readonly render_text: (a: number, b: bigint, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => void;
    readonly score_window: (a: number, b: bigint, c: number, d: number, e: number, f: number, g: number) => void;
    readonly search_cities: (a: number, b: number, c: number, d: number, e: bigint) => void;
    readonly suggest_slots: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: bigint, i: number, j: number, k: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number) => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
