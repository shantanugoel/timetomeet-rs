# legacy

The original Yew 0.18 app, kept for reference. It **cannot be rebuilt** with a
modern toolchain: `yew 0.18` pins `wasm-bindgen 0.2.74`, which rustc rejects
outright. It is also the source of the live wasm trap documented in
`docs/REDESIGN.md` §0.1 (mitigated in the static HTML, not here).

Not a workspace member. Do not revive it; the rewrite is in `../crates`.
