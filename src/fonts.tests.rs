//! Unit tests for [`super`]: script classification, candidate order, and the lazy
//! provider. Fixtures are plain bytes in `./.tmp/` — the provider only reads files, it
//! never parses them (egui parses the `FontData` it receives).

use super::*;
use std::fs;
use std::path::PathBuf;

/// Scratch fixture directory for one case (git-ignored `./.tmp/`).
fn fixture_dir(case: &str) -> PathBuf {
    let dir = PathBuf::from("./.tmp").join(format!("fonts.tests.{case}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn request<'a>(cluster: &'a str, family: &'a FontFamily) -> FallbackRequest<'a> {
    FallbackRequest { cluster, family }
}

fn provider_over(paths: Vec<FontCandidate>) -> SystemScriptFonts {
    SystemScriptFonts::with_candidates(Box::new(move |_script| paths.clone()))
}

#[test]
fn script_of_hangul_syllable_is_korean() {
    let c = "한국어".chars().next().expect("Hangul syllable");
    assert_eq!(script_of(c), Some(Script::Korean));
}

#[test]
fn script_of_hangul_jamo_is_korean() {
    assert_eq!(script_of('ᄀ'), Some(Script::Korean));
}

#[test]
fn script_of_katakana_is_japanese() {
    assert_eq!(script_of('カ'), Some(Script::Japanese));
}

#[test]
fn script_of_hiragana_is_japanese() {
    assert_eq!(script_of('あ'), Some(Script::Japanese));
}

#[test]
fn script_of_cjk_ideograph_is_chinese() {
    assert_eq!(script_of('设'), Some(Script::Chinese));
}

#[test]
fn script_of_cjk_punctuation_is_chinese() {
    assert_eq!(script_of('、'), Some(Script::Chinese));
}

#[test]
fn script_of_latin_is_none() {
    assert_eq!(script_of('J'), None);
    assert_eq!(script_of('%'), None);
}

#[test]
fn script_of_cyrillic_is_none() {
    // Russian ships from the bundled Ubuntu-Light (measured: 0 missing glyphs), so the
    // fallback must never claim Cyrillic.
    assert_eq!(script_of('Р'), None);
    assert_eq!(script_of('ю'), None);
}

#[test]
fn candidates_korean_lists_malgun_first() {
    let candidates = candidates_for(Script::Korean);
    assert!(candidates[0].path.ends_with("malgun.ttf"));
}

#[test]
fn candidates_chinese_lists_yahei_first() {
    let candidates = candidates_for(Script::Chinese);
    assert_eq!(candidates[0].path, "C:\\Windows\\Fonts\\msyh.ttc");
    assert_eq!(candidates[0].index, 0);
}

#[test]
fn candidates_japanese_lists_yu_gothic_first() {
    let candidates = candidates_for(Script::Japanese);
    assert!(candidates[0].path.ends_with("yugothic.ttf"));
    assert!(candidates.iter().any(|c| c.path.ends_with("msyh.ttc")));
}

#[test]
fn candidates_end_with_a_linux_dev_host_font() {
    for script in [Script::Korean, Script::Japanese, Script::Chinese] {
        let candidates = candidates_for(script);
        let last = candidates.last().expect("at least one candidate");
        assert!(
            last.path.starts_with("/usr/share/fonts/"),
            "{script:?} last: {}",
            last.path
        );
    }
}

#[test]
fn provider_latin_cluster_installs_nothing() {
    let dir = fixture_dir("latin");
    let file = dir.join("anything.ttf");
    fs::write(&file, b"font-bytes").expect("fixture write");
    let provider = provider_over(vec![FontCandidate::new(file.display().to_string(), 0)]);
    assert!(
        provider
            .font_for(&request("JPEG", &FontFamily::Proportional))
            .is_none()
    );
    assert!(
        provider
            .font_for(&request("WebP", &FontFamily::Monospace))
            .is_none()
    );
    assert!(
        provider
            .font_for(&request("日本語", &FontFamily::Proportional))
            .is_some()
    );
}

#[test]
fn provider_reads_first_readable_candidate() {
    let dir = fixture_dir("first-readable");
    let skipped = dir.join("missing.ttf");
    let readable = dir.join("readable.ttf");
    fs::write(&readable, b"font-bytes").expect("fixture write");
    let provider = provider_over(vec![
        FontCandidate::new(skipped.display().to_string(), 0),
        FontCandidate::new(readable.display().to_string(), 0),
    ]);
    let insert = provider
        .font_for(&request("设置", &FontFamily::Proportional))
        .expect("second candidate readable");
    assert_eq!(insert.name, "fallback-chinese");
    assert_eq!(insert.data, FontData::from_owned(b"font-bytes".to_vec()));
    assert_eq!(insert.families.len(), 1);
    assert_eq!(insert.families[0].family, FontFamily::Proportional);
    assert_eq!(insert.families[0].priority, FontPriority::Lowest);
}

#[test]
fn provider_inserts_into_the_requesting_family() {
    let dir = fixture_dir("family");
    let file = dir.join("ko.ttf");
    fs::write(&file, b"ko-bytes").expect("fixture write");
    let provider = provider_over(vec![FontCandidate::new(file.display().to_string(), 0)]);
    let insert = provider
        .font_for(&request("저장", &FontFamily::Monospace))
        .expect("readable");
    assert_eq!(insert.name, "fallback-korean");
    assert_eq!(insert.families[0].family, FontFamily::Monospace);
}

#[test]
fn provider_caches_one_read_per_script() {
    let dir = fixture_dir("cached-read");
    let han = dir.join("han.ttf");
    let hangul = dir.join("hangul.ttf");
    fs::write(&han, b"han-bytes").expect("fixture write");
    fs::write(&hangul, b"hangul-bytes").expect("fixture write");
    let provider = provider_over(vec![
        FontCandidate::new(han.display().to_string(), 0),
        FontCandidate::new(hangul.display().to_string(), 0),
    ]);
    let first = provider
        .font_for(&request("设置", &FontFamily::Proportional))
        .expect("han readable");
    // The cache must answer later requests even after the file is gone: no re-read.
    fs::remove_file(&han).expect("fixture remove");
    let again = provider
        .font_for(&request("分辨率", &FontFamily::Proportional))
        .expect("cached");
    assert_eq!(again.data, first.data);
    // A different script still reads its own candidate.
    let korean = provider
        .font_for(&request("저장", &FontFamily::Proportional))
        .expect("hangul readable");
    assert_eq!(korean.data, FontData::from_owned(b"hangul-bytes".to_vec()));
    assert_ne!(korean.name, first.name);
}

#[test]
fn provider_missing_all_candidates_caches_the_miss() {
    let dir = fixture_dir("missing-all");
    let provider = provider_over(vec![FontCandidate::new(
        dir.join("absent.ttf").display().to_string(),
        0,
    )]);
    assert!(
        provider
            .font_for(&request("设置", &FontFamily::Proportional))
            .is_none()
    );
    // A file appearing later must not resurrect the script: egui asks once per
    // character anyway, and the miss is cached so the filesystem is not re-read.
    fs::write(dir.join("absent.ttf"), b"late-bytes").expect("fixture write");
    assert!(
        provider
            .font_for(&request("分辨率", &FontFamily::Proportional))
            .is_none()
    );
}

#[cfg(not(windows))]
#[test]
fn provider_dev_host_loads_a_real_cjk_font() {
    // The Linux dev host has no `C:\Windows\Fonts`: the last candidate must be a real
    // installed font, so a local preview of zh/ja/ko renders instead of tofu. Glyph
    // coverage of that file is measured separately (`./.tmp` cmap probes).
    let candidate = candidates_for(Script::Chinese)
        .last()
        .expect("dev host candidate")
        .clone();
    let provider = provider_over(vec![candidate]);
    let Some(insert) = provider.font_for(&request("设置", &FontFamily::Proportional)) else {
        // Not a failure: this host simply has no dev-host CJK font installed.
        return;
    };
    assert!(
        insert.data.bytes().len() > 1_000_000,
        "dev host CJK font too small: {}",
        insert.data.bytes().len()
    );
}
