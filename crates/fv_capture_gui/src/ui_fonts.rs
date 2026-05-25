use std::sync::Arc;

use eframe::egui;

const UI_FONT_NAME: &str = "M PLUS 1";
const UI_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/MPLUS1.ttf");

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

fn definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        UI_FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_static(UI_FONT_BYTES)),
    );

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(names) = fonts.families.get_mut(&family) {
            names.retain(|name| name != UI_FONT_NAME);
            names.insert(0, UI_FONT_NAME.to_owned());
        }
    }

    fonts
}

#[cfg(test)]
mod tests {
    use ab_glyph::{Font, FontArc};

    use super::{UI_FONT_BYTES, UI_FONT_NAME, definitions};

    #[test]
    fn bundled_ui_font_is_registered_first() {
        let fonts = definitions();

        assert!(fonts.font_data.contains_key(UI_FONT_NAME));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            assert_eq!(
                fonts.families.get(&family).unwrap().first().unwrap(),
                UI_FONT_NAME
            );
        }
    }

    #[test]
    fn bundled_ui_font_contains_japanese_glyphs() {
        let font = FontArc::try_from_slice(UI_FONT_BYTES).expect("bundled UI font should parse");
        for ch in ['録', '画', '範', '囲', '開', '始'] {
            assert_ne!(font.glyph_id(ch).0, 0, "missing glyph for {ch}");
        }
    }
}
