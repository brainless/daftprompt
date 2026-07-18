// UI module tree after Task 2 of Epic 005.
//
// - `container` is the data model (ContainerType, CardData, constructors).
//   It is consumed by Tasks 5 (rendering) and 6 (search results). It does not
//   depend on the renderer or input pipeline, so it survives the akar migration
//   unchanged.
//
// - `render` holds the immediate-mode render functions wired into the per-frame
//   loop in `main.rs`. For Task 2 the bodies are stubs; Tasks 3/4/6 fill them in.
//
// The old `canvas.rs`, `drawer.rs`, `search.rs` modules (which depended on the
// removed `renderer.rs`/`input.rs` types) are re-implemented on top of akar in
// Tasks 3/4/6 — each task deletes the corresponding old module file.
pub mod adapter;
pub mod container;
pub mod render;
