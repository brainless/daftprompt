// sqlite-vec load incantation (verified with sqlite-vec 0.0.1-alpha.33, rusqlite 0.31 bundled):
// unsafe {
//     rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
//         sqlite_vec::sqlite3_vec_init as *const (),
//     )));
// }
