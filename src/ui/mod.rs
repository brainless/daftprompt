// Stub during Task 1 migration. canvas/drawer/search modules depended on the
// removed renderer.rs / input.rs types. They are re-introduced (using akar) in
// Tasks 2–5. `container` is the only UI module whose data model is independent
// of the renderer/input, so it stays accessible.
pub mod container;
