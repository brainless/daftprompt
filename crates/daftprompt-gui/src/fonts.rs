//! Bundled fonts and the two font roles the GUI draws with.
//!
//! * **UI** is Inter (labels, headings, badges, buttons, sidebar chrome, status
//!   text, panel card chrome).
//! * **Text** is Source Serif 4 (the composer and the source or answer text the
//!   user reads: span quotes, unlabelled text, and clarify target/question/
//!   explanation text).
//!
//! Both are SIL OFL; the license texts sit beside the font files in
//! `assets/fonts/`. akar offers no way to change its default sans-serif family,
//! so every call that draws text names its family explicitly through
//! [`Fonts`]. If a font fails to load, that role falls back to akar's generic
//! sans-serif or serif family and the error is logged; nothing panics.

use std::sync::OnceLock;

use akar_components::{FontFamily, FontWeight, TextStyle};
use akar_core::TextPipeline;
use glyphon::{Attrs, Family, Weight};

const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const SOURCE_SERIF_REGULAR: &[u8] = include_bytes!("../assets/fonts/SourceSerif4-Regular.ttf");

/// Which font a piece of text uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Ui,
    Text,
}

/// Loaded family handles and names for the two roles.
#[derive(Clone, Debug)]
pub struct Fonts {
    ui: FontFamily,
    text: FontFamily,
    ui_name: Option<String>,
    text_name: Option<String>,
}

static FONTS: OnceLock<Fonts> = OnceLock::new();

impl Fonts {
    /// Generic families only, used before (or without) loading.
    pub fn fallback() -> Self {
        Self {
            ui: FontFamily::SansSerif,
            text: FontFamily::Serif,
            ui_name: None,
            text_name: None,
        }
    }

    /// Loads the bundled fonts into `pipeline`. A role whose faces fail to load
    /// keeps its generic fallback; errors are logged to stderr.
    pub fn load(pipeline: &mut TextPipeline) -> Self {
        let mut fonts = Self::fallback();
        if let Some((family, name)) = load_role(
            pipeline,
            "Inter",
            &[INTER_REGULAR, INTER_MEDIUM, INTER_SEMIBOLD],
        ) {
            fonts.ui = family;
            fonts.ui_name = Some(name);
        }
        if let Some((family, name)) = load_role(pipeline, "Source Serif 4", &[SOURCE_SERIF_REGULAR])
        {
            fonts.text = family;
            fonts.text_name = Some(name);
        }
        fonts
    }

    /// Installs these fonts for the draw helpers. Only the first call has effect.
    pub fn install(self) {
        let _ = FONTS.set(self);
    }

    pub fn family(&self, role: Role) -> FontFamily {
        match role {
            Role::Ui => self.ui,
            Role::Text => self.text,
        }
    }

    /// A `TextStyle` carrying the role's family and the given weight.
    pub fn style(&self, role: Role, weight: FontWeight) -> TextStyle {
        TextStyle {
            font_family: Some(self.family(role)),
            font_weight: Some(weight),
            ..TextStyle::empty()
        }
    }

    /// glyphon attributes for transient text drawn straight through the pipeline.
    pub fn attrs(&self, role: Role, weight: u16) -> Attrs<'_> {
        let (name, generic) = match role {
            Role::Ui => (self.ui_name.as_deref(), Family::SansSerif),
            Role::Text => (self.text_name.as_deref(), Family::Serif),
        };
        Attrs::new()
            .family(name.map_or(generic, Family::Name))
            .weight(Weight(weight))
    }
}

/// The installed fonts, or the generic fallback before installation (tests).
pub fn current() -> &'static Fonts {
    static FALLBACK: OnceLock<Fonts> = OnceLock::new();
    FONTS
        .get()
        .unwrap_or_else(|| FALLBACK.get_or_init(Fonts::fallback))
}

fn load_role(
    pipeline: &mut TextPipeline,
    label: &str,
    faces: &[&'static [u8]],
) -> Option<(FontFamily, String)> {
    let mut handle = None;
    for data in faces {
        match pipeline.load_font_bytes(data.to_vec()) {
            Ok(loaded) => match handle {
                None => handle = Some(loaded),
                Some(first) if first != loaded => {
                    eprintln!("daftprompt-gui: {label} faces resolved to different families");
                }
                Some(_) => {}
            },
            Err(error) => eprintln!("daftprompt-gui: could not load a {label} face: {error}"),
        }
    }
    let handle = handle?;
    let name = pipeline.family_name(handle)?.to_owned();
    Some((FontFamily::Named(handle), name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use akar_core::AkarCore;

    #[test]
    fn bundled_fonts_load_as_distinct_named_families() {
        let mut core = AkarCore::mock();
        let fonts = Fonts::load(&mut core.text_pipeline);
        let (FontFamily::Named(ui), FontFamily::Named(text)) = (fonts.ui, fonts.text) else {
            panic!("both roles should load as named families");
        };
        assert_ne!(ui, text);
        assert_eq!(fonts.ui_name.as_deref(), Some("Inter"));
        assert_eq!(fonts.text_name.as_deref(), Some("Source Serif 4"));
        assert_eq!(core.text_pipeline.family_name(ui), Some("Inter"));
        assert_eq!(core.text_pipeline.family_name(text), Some("Source Serif 4"));
    }

    #[test]
    fn bad_font_data_falls_back_without_panicking() {
        let mut core = AkarCore::mock();
        assert!(load_role(&mut core.text_pipeline, "Broken", &[b"not a font"]).is_none());
        assert!(load_role(&mut core.text_pipeline, "Empty", &[b""]).is_none());
    }
}
