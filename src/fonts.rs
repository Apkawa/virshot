//! System font fallback for scripts the bundled egui fonts cannot render.
//!
//! The shipped binary trims `eframe`'s `system_fonts` feature (size budget, spec §5),
//! so the only faces egui has are `Ubuntu-Light` and `Hack`. Coverage measured against
//! the bundled `epaint_default_fonts` cmap: `en`/`ru`/`es` need nothing extra
//! (Ubuntu-Light carries Latin **and** Cyrillic outlines), while `zh-CN` misses 74 keys,
//! `ja` 73 and `ko` 73 — CJK ideographs, kana and Hangul syllables have no glyph at all
//! and render as `.notdef` boxes (spec §2: localized UI). Unicode itself is not the
//! problem: the catalog, `t!` and egui text are UTF-8 end to end; only the font coverage
//! is missing.
//!
//! Instead of enabling `system_fonts` (+0.53 MB measured on the Windows-gnu release) or
//! embedding font subsets (0.4–1 MB in the binary), this module installs an
//! [`egui::Context::add_font_provider`] that reads a **system-installed** font file on
//! demand. The binary stays byte-identical in size, and nothing is loaded until a glyph
//! that no installed face covers actually appears:
//!
//! - egui asks providers only after every font of the family missed, at most once per
//!   `(family, character)` (`epaint-0.37.0/src/text/font_provider.rs:43-52`), so Latin
//!   and Cyrillic keep rendering from Ubuntu-Light — the fallback never changes them.
//! - A discovered font is appended to the family's fallback chain (the `FontPriority` is
//!   ignored by egui: discovered fonts always come last).
//! - One file per script is read into an owned `Blob` and cached, so a 20 MB `.ttc` is
//!   read once, not per glyph. RAM is spent only while CJK/Hangul is on screen — the
//!   dropdown shows all six endonyms at once, so opening it loads all three scripts.
//!
//! Honesty limits: the candidate paths are the fonts Windows ships, not verified here
//! (no Windows host in this sandbox) — a missing file falls through to the next
//! candidate, and if none reads the provider returns `None` and the glyph stays tofu,
//! exactly as before. Linux candidates exist only so the dev host can preview CJK.

use egui::{
    FallbackRequest, FontData, FontFamily, FontInsert, FontPriority, FontProvider, InsertFontFamily,
};
use std::collections::HashMap;
use std::sync::Mutex;

/// Script of a glyph none of the bundled faces covers. Only the scripts the shipped
/// locales (`locales/app.yml`) need.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Script {
    Korean,
    Japanese,
    Chinese,
}

impl Script {
    /// Name used in logs and as the egui font name.
    fn label(self) -> &'static str {
        match self {
            Script::Korean => "korean",
            Script::Japanese => "japanese",
            Script::Chinese => "chinese",
        }
    }
}

/// A font file to try, and which face of it to use (`.ttc` collections carry several).
#[derive(Clone, Debug)]
pub(crate) struct FontCandidate {
    path: String,
    index: u32,
}

impl FontCandidate {
    pub(crate) fn new(path: impl Into<String>, index: u32) -> Self {
        Self {
            path: path.into(),
            index,
        }
    }
}

/// The script `c` belongs to, by Unicode block. Kana is checked before ideographs:
/// kana is unambiguously Japanese, while ideographs are shared across locales and are
/// served by the Chinese face (see the module header for the honesty limit).
pub(crate) fn script_of(c: char) -> Option<Script> {
    let code = c as u32;
    if [
        0x1100..=0x11FF,
        0x3131..=0x318E,
        0xA960..=0xA97F,
        0xAC00..=0xD7A3,
    ]
    .iter()
    .any(|range| range.contains(&code))
    {
        return Some(Script::Korean);
    }
    if [0x3040..=0x30FF, 0x31F0..=0x31FF]
        .iter()
        .any(|range| range.contains(&code))
    {
        return Some(Script::Japanese);
    }
    if [
        0x3000..=0x303F,
        0x3400..=0x4DBF,
        0x4E00..=0x9FFF,
        0xF900..=0xFAFF,
    ]
    .iter()
    .any(|range| range.contains(&code))
    {
        return Some(Script::Chinese);
    }
    None
}

/// Candidate files for a script, best match first: what Windows ships, then Linux dev
/// host fallbacks. A `.ttc` is a collection, and `read_fonts` picks the face by index
/// (`read-fonts-0.37.0/src/lib.rs:334-345`), so index 0 is the first face — for
/// `msyh.ttc` that is Microsoft YaHei.
pub(crate) fn candidates_for(script: Script) -> Vec<FontCandidate> {
    match script {
        Script::Korean => vec![
            FontCandidate::new("C:\\Windows\\Fonts\\malgun.ttf", 0),
            FontCandidate::new("C:\\Windows\\Fonts\\malgunsl.ttf", 0),
            // Dev host only: a CJK font with Hangul coverage.
            FontCandidate::new("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", 0),
        ],
        Script::Japanese => vec![
            FontCandidate::new("C:\\Windows\\Fonts\\yugothic.ttf", 0),
            FontCandidate::new("C:\\Windows\\Fonts\\yugothui.ttf", 0),
            FontCandidate::new("C:\\Windows\\Fonts\\msyh.ttc", 0),
            // Dev host only: IPA Gothic (Japanese), then the pan-CJK fallback.
            FontCandidate::new("/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf", 0),
            FontCandidate::new("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", 0),
        ],
        Script::Chinese => vec![
            FontCandidate::new("C:\\Windows\\Fonts\\msyh.ttc", 0),
            FontCandidate::new("C:\\Windows\\Fonts\\msyh.ttf", 0),
            FontCandidate::new("C:\\Windows\\Fonts\\simsun.ttc", 0),
            // Dev host only: a pan-CJK font covering ideographs, kana and Hangul.
            FontCandidate::new("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", 0),
        ],
    }
}

/// A file already read for one script.
struct CachedFont {
    name: String,
    data: FontData,
}

/// [`egui::FontProvider`] that loads a system font for a script on first miss and
/// caches the result — including a miss, so egui's one-request-per-character never
/// re-reads the filesystem.
pub(crate) struct SystemScriptFonts {
    loaded: Mutex<HashMap<Script, Option<CachedFont>>>,
    /// Seam for tests: candidate lists are normally [`candidates_for`].
    candidates: Box<dyn Fn(Script) -> Vec<FontCandidate> + Send + Sync>,
}

impl Default for SystemScriptFonts {
    fn default() -> Self {
        Self::with_candidates(Box::new(candidates_for))
    }
}

impl SystemScriptFonts {
    pub(crate) fn with_candidates(
        candidates: Box<dyn Fn(Script) -> Vec<FontCandidate> + Send + Sync>,
    ) -> Self {
        Self {
            loaded: Mutex::new(HashMap::new()),
            candidates,
        }
    }

    /// Read the first candidate that exists for `script`. A `.ttc` carries several
    /// faces, so the candidate's `index` is kept — `FontData::from_blob` hands it to
    /// the parser (`epaint-0.37.0/src/text/font_data.rs:36-42`).
    fn load(&self, script: Script) -> Option<CachedFont> {
        for candidate in (self.candidates)(script) {
            match std::fs::read(&candidate.path) {
                Ok(bytes) => {
                    tracing::info!(
                        "font fallback for {} loaded: {} ({} bytes)",
                        script.label(),
                        candidate.path,
                        bytes.len()
                    );
                    let data = match candidate.index {
                        0 => FontData::from_owned(bytes),
                        index => FontData::from_blob(std::sync::Arc::new(bytes), index),
                    };
                    return Some(CachedFont {
                        name: format!("fallback-{}", script.label()),
                        data,
                    });
                }
                Err(err) => tracing::debug!("font fallback candidate {}: {err}", candidate.path),
            }
        }
        tracing::warn!("no system font found for the {} script", script.label());
        None
    }
}

impl FontProvider for SystemScriptFonts {
    fn font_for(&self, request: &FallbackRequest<'_>) -> Option<FontInsert> {
        let script = script_of(request.cluster.chars().next()?)?;
        let mut loaded = self.loaded.lock().ok()?;
        if loaded.contains_key(&script) {
            return loaded
                .get(&script)
                .and_then(|cached| cached.as_ref())
                .map(|cached| insert_for(cached, request.family));
        }
        let found = self.load(script);
        let insert = found
            .as_ref()
            .map(|cached| insert_for(cached, request.family));
        loaded.insert(script, found);
        insert
    }
}

/// The cached font as an insert into the family that asked for it, as a fallback.
/// egui appends discovered fonts after the configured faces regardless of priority
/// (`epaint-0.37.0/src/text/font_provider.rs:47-50`); `Lowest` states the intent.
fn insert_for(cached: &CachedFont, family: &FontFamily) -> FontInsert {
    FontInsert {
        name: cached.name.clone(),
        data: cached.data.clone(),
        families: vec![InsertFontFamily {
            family: family.clone(),
            priority: FontPriority::Lowest,
        }],
    }
}

/// Install the fallback provider on the app's egui context. Called at window creation
/// (`gui::run`), before the first frame; the provider is shared by every viewport of
/// this context, so the settings window, the toast chips and the overlay all localize.
pub(crate) fn install(ctx: &egui::Context) {
    ctx.add_font_provider(std::sync::Arc::new(SystemScriptFonts::default()));
}

#[cfg(test)]
#[path = "fonts.tests.rs"]
mod tests;
