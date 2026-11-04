//! GUI-owned palette: akar's dark theme with darker backgrounds.

use akar_components::{AKAR_THEME_DARK, AkarTheme};

/// Darker than akar's stock dark theme. `base_100` is the main column (pure
/// black), `base_200` the left column, cards, and hover box, and `base_300`
/// borders, scrollbar thumbs, and hover/pressed fills. Everything else
/// (text, semantic colours, sizes) is akar's.
pub const DAFTPROMPT_THEME: AkarTheme = AkarTheme {
    base_100: 0x000000ff,
    base_200: 0x0a0a0bff,
    base_300: 0x26262bff,
    ..AKAR_THEME_DARK
};
