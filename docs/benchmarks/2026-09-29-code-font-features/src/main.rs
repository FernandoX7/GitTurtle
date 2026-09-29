//! Per-line shaping time of the bundled code font, DejaVu Sans Mono, with the
//! font features each GitTurtle build hands the text system for it, and a
//! glyph check that a ligature font still draws code as typed. See README.md.
//!
//! Each line is shaped the way GPUI 0.3.4's Linux text system does it
//! (`gpui-pre-wgpu`'s `layout_line_no_separators`): one span of the run's
//! attributes over a default `AttrsList`, `ShapeLine::new` with advanced
//! shaping and a tab width of 4, then `layout_to_buffer` without wrapping,
//! ellipsis, alignment or hinting. GPUI clones the font's features into the
//! attributes of every run, so this does too.

use cosmic_text::{
    Attrs, AttrsList, Ellipsize, Family, FeatureTag, FontFeatures, FontSystem, Hinting, LayoutLine,
    ShapeBuffer, ShapeLine, Shaping, Stretch, Style, Weight, Wrap, fontdb,
};
use std::{path::PathBuf, process::exit, time::Instant};

/// The bundled family, as `desktop_text::BUNDLED_CODE_FAMILY` names it.
const BUNDLED_FAMILY: &str = "DejaVu Sans Mono";
/// The default code text size in pixels (`appearance::DEFAULT_CODE_TEXT_SIZE`).
const FONT_SIZE: f32 = 12.0;
const WARM_UP_ROUNDS: usize = 2;
/// The pairs whose ligatures code text keeps off; the acceptance names the
/// first two.
const PAIRS: [&str; 7] = ["->", "!=", "=>", "==", "<=", "--", "//"];
const REQUIRED_PAIRS: [&str; 2] = ["->", "!="];

struct Options {
    corpus: PathBuf,
    fonts: PathBuf,
    lines: usize,
    samples: usize,
    ligature_font: Option<PathBuf>,
    glyphs_only: bool,
    json: Option<PathBuf>,
}

/// One feature list as a GitTurtle build hands it to the text system.
struct Config {
    name: &'static str,
    /// The `code_font_features_for` result, as GPUI's `(tag, value)` list.
    tags: &'static [(&'static str, u16)],
}

/// What origin/main's `.code_font` hands the text system for every family.
const BASE: Config = Config {
    name: "base",
    tags: &[("calt", 0), ("liga", 0)],
};
/// What this change hands it for the bundled family.
const CANDIDATE: Config = Config {
    name: "candidate",
    tags: &[],
};

fn features(tags: &[(&str, u16)]) -> FontFeatures {
    // GPUI's `cosmic_font_features`: an empty list sets nothing.
    let mut features = FontFeatures::new();
    for (tag, value) in tags {
        let tag: [u8; 4] = tag.as_bytes().try_into().expect("four-byte tag");
        features.set(FeatureTag::new(&tag), *value as u32);
    }
    features
}

/// Shapes one line as GPUI's `layout_line` does and returns its glyph ids.
fn shape(
    fonts: &mut FontSystem,
    scratch: &mut ShapeBuffer,
    family: &str,
    features: &FontFeatures,
    line: &str,
) -> Vec<u16> {
    let attrs = Attrs::new()
        .metadata(0)
        .family(Family::Name(family))
        .stretch(Stretch::Normal)
        .style(Style::Normal)
        .weight(Weight::NORMAL)
        .font_features(features.clone());
    let mut list = AttrsList::new(&Attrs::new());
    list.add_span(0..line.len(), &attrs);
    let shaped = ShapeLine::new(fonts, line, &list, Shaping::Advanced, 4);
    let mut layout: Vec<LayoutLine> = Vec::with_capacity(1);
    shaped.layout_to_buffer(
        scratch,
        FONT_SIZE,
        None,
        Wrap::None,
        Ellipsize::None,
        None,
        &mut layout,
        None,
        Hinting::Disabled,
    );
    layout
        .first()
        .map(|line| line.glyphs.iter().map(|glyph| glyph.glyph_id).collect())
        .unwrap_or_default()
}

fn main() {
    let options = options();
    let mut db = fontdb::Database::new();
    for face in [
        "DejaVuSansMono.ttf",
        "DejaVuSansMono-Bold.ttf",
        "DejaVuSansMono-Oblique.ttf",
        "DejaVuSansMono-BoldOblique.ttf",
    ] {
        let path = options.fonts.join(face);
        db.load_font_file(&path)
            .unwrap_or_else(|error| fail(&format!("{}: {error}", path.display())));
    }
    let ligature_family = options.ligature_font.as_ref().map(|path| {
        let bundled: Vec<fontdb::ID> = db.faces().map(|face| face.id).collect();
        db.load_font_file(path)
            .unwrap_or_else(|error| fail(&format!("{}: {error}", path.display())));
        let face = db
            .faces()
            .find(|face| !bundled.contains(&face.id))
            .unwrap_or_else(|| fail(&format!("{} holds no font face", path.display())));
        face.families[0].0.clone()
    });
    if !db
        .faces()
        .any(|face| face.families.iter().any(|(name, _)| name == BUNDLED_FAMILY))
    {
        fail(&format!(
            "{} holds no {BUNDLED_FAMILY}",
            options.fonts.display()
        ));
    }
    let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
    let mut scratch = ShapeBuffer::default();

    if let Some(family) = &ligature_family {
        check_ligature_font(&mut fonts, &mut scratch, family);
    }
    if options.glyphs_only {
        return;
    }
    measure(&options, &mut fonts, &mut scratch);
}

/// A ligature font under the desktop families' features (`calt` and `liga`
/// off) draws each pair as its characters' own glyphs, and under its default
/// features joins at least the required pairs, so the check can tell.
fn check_ligature_font(fonts: &mut FontSystem, scratch: &mut ShapeBuffer, family: &str) {
    let off = features(BASE.tags);
    let default = FontFeatures::new();
    let face = fonts
        .db()
        .faces()
        .find(|face| face.families.iter().any(|(name, _)| name == family))
        .map(|face| face.id)
        .expect("the loaded ligature face");
    let font = fonts
        .get_font(face, Weight::NORMAL)
        .unwrap_or_else(|| fail(&format!("{family} does not load")));
    println!("glyphs: {family}, desktop code features calt=0 liga=0 against its defaults");
    let mut failed = false;
    for pair in PAIRS {
        let nominal: Vec<u16> = pair
            .chars()
            .map(|ch| font.as_swash().charmap().map(ch))
            .collect();
        let as_typed = shape(fonts, scratch, family, &off, pair);
        let joined = shape(fonts, scratch, family, &default, pair);
        let separate = as_typed == nominal && as_typed.len() == pair.chars().count();
        println!(
            "  {pair:3} code features: {} glyphs {as_typed:?} ({}); defaults: {} glyphs {joined:?} ({})",
            as_typed.len(),
            if separate { "separate" } else { "NOT separate" },
            joined.len(),
            if joined == nominal {
                "separate"
            } else {
                "ligature"
            },
        );
        failed |= !separate;
        if REQUIRED_PAIRS.contains(&pair) && joined == nominal {
            println!("  {pair} has no ligature in {family}, so it cannot show the features work");
            failed = true;
        }
    }
    if failed {
        fail("the ligature check failed");
    }
    println!("glyphs: ok");
}

fn measure(options: &Options, fonts: &mut FontSystem, scratch: &mut ShapeBuffer) {
    let lines = corpus(&options.corpus, options.lines);
    let bytes: usize = lines.iter().map(String::len).sum();
    let identity = lines.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, line| {
        line.bytes().chain(Some(b'\n')).fold(hash, |hash, byte| {
            (hash ^ byte as u64).wrapping_mul(0x100_0000_01b3)
        })
    });
    let configs = [BASE, CANDIDATE];
    let feature_lists: Vec<FontFeatures> = configs.iter().map(|c| features(c.tags)).collect();

    // Skipping the features must not change what the bundled font draws.
    let mut glyphs = 0;
    for line in &lines {
        let base = shape(fonts, scratch, BUNDLED_FAMILY, &feature_lists[0], line);
        let candidate = shape(fonts, scratch, BUNDLED_FAMILY, &feature_lists[1], line);
        if base != candidate {
            fail(&format!(
                "base and candidate draw different glyphs for {line:?}"
            ));
        }
        glyphs += base.len();
    }
    println!(
        "corpus: {} lines, {bytes} bytes, fnv1a {identity:016x}, {glyphs} glyphs, the same in both configurations",
        lines.len()
    );

    // Interleaved: every sample shapes the corpus once per configuration,
    // alternating which goes first.
    let mut totals: Vec<Vec<f64>> = vec![Vec::new(); configs.len()];
    for round in 0..WARM_UP_ROUNDS + options.samples {
        let order: [usize; 2] = if round % 2 == 0 { [0, 1] } else { [1, 0] };
        for index in order {
            let start = Instant::now();
            let mut count = 0;
            for line in &lines {
                count += shape(fonts, scratch, BUNDLED_FAMILY, &feature_lists[index], line).len();
            }
            let ms = start.elapsed().as_secs_f64() * 1e3;
            assert_eq!(count, glyphs);
            if round >= WARM_UP_ROUNDS {
                totals[index].push(ms);
            }
        }
    }

    println!(
        "samples: {} per configuration after {WARM_UP_ROUNDS} warm-up rounds, {FONT_SIZE} px",
        options.samples
    );
    let per_line = |ms: f64| ms * 1e3 / lines.len() as f64;
    let mut summaries = Vec::new();
    for (config, samples) in configs.iter().zip(&totals) {
        let mut sorted = samples.clone();
        sorted.sort_by(f64::total_cmp);
        // Nearest rank on the sorted samples.
        let rank = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
        let (p50, p95, max) = (rank(0.5), rank(0.95), sorted[sorted.len() - 1]);
        println!(
            "{:9} {:?}: ms per corpus p50 {p50:.2} p95 {p95:.2} max {max:.2}; us per line p50 {:.2} p95 {:.2} max {:.2}",
            config.name,
            config.tags,
            per_line(p50),
            per_line(p95),
            per_line(max),
        );
        println!(
            "{:9} raw ms per corpus: {}",
            "",
            join(samples.iter().map(|ms| format!("{ms:.3}")))
        );
        println!(
            "{:9} raw us per line:   {}",
            "",
            join(samples.iter().map(|ms| format!("{:.3}", per_line(*ms))))
        );
        summaries.push((config, samples, p50, p95, max));
    }
    let (base_p50, candidate_p50) = (summaries[0].2, summaries[1].2);
    println!(
        "candidate / base at p50: {:.3} ({:+.1}%)",
        candidate_p50 / base_p50,
        (candidate_p50 / base_p50 - 1.) * 100.
    );

    if let Some(path) = &options.json {
        let configurations = join(summaries.iter().map(|(config, samples, p50, p95, max)| {
            format!(
                "{{\"name\":\"{}\",\"features\":[{}],\"ms_per_corpus\":{{\"p50\":{p50:.3},\"p95\":{p95:.3},\"max\":{max:.3},\"raw\":[{}]}},\"us_per_line\":{{\"p50\":{:.3},\"p95\":{:.3},\"max\":{:.3},\"raw\":[{}]}}}}",
                config.name,
                join(config.tags.iter().map(|(tag, value)| format!("[\"{tag}\",{value}]"))),
                join(samples.iter().map(|ms| format!("{ms:.3}"))),
                per_line(*p50),
                per_line(*p95),
                per_line(*max),
                join(samples.iter().map(|ms| format!("{:.3}", per_line(*ms)))),
            )
        }));
        let json = format!(
            "{{\"family\":\"{BUNDLED_FAMILY}\",\"font_size_px\":{FONT_SIZE},\"warm_up_rounds\":{WARM_UP_ROUNDS},\"samples\":{},\"percentile\":\"nearest rank\",\"corpus\":{{\"lines\":{},\"bytes\":{bytes},\"fnv1a\":\"{identity:016x}\",\"glyphs\":{glyphs}}},\"configurations\":[{configurations}]}}\n",
            options.samples,
            lines.len(),
        );
        std::fs::write(path, json)
            .unwrap_or_else(|error| fail(&format!("{}: {error}", path.display())));
        println!("json: {}", path.display());
    }
}

/// The first `limit` non-empty lines of the `.rs` files directly in `dir`,
/// in path order.
fn corpus(dir: &PathBuf, limit: usize) -> Vec<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| fail(&format!("{}: {error}", dir.display())))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    files.sort();
    let mut lines = Vec::with_capacity(limit);
    for file in files {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|error| fail(&format!("{}: {error}", file.display())));
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            if lines.len() == limit {
                return lines;
            }
            lines.push(line.to_owned());
        }
    }
    if lines.len() < limit {
        fail(&format!(
            "{} has only {} non-empty lines",
            dir.display(),
            lines.len()
        ));
    }
    lines
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(",")
}

fn options() -> Options {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut options = Options {
        corpus: repository.join("crates/app/src"),
        fonts: repository.join("assets/fonts/dejavu-sans-mono"),
        lines: 5000,
        samples: 30,
        ligature_font: None,
        glyphs_only: false,
        json: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| usage(&format!("{arg} needs a value")))
        };
        match arg.as_str() {
            "--corpus" => options.corpus = value().into(),
            "--fonts" => options.fonts = value().into(),
            "--lines" => options.lines = number(&value()),
            "--samples" => options.samples = number(&value()),
            "--ligature-font" => options.ligature_font = Some(value().into()),
            "--json" => options.json = Some(value().into()),
            "--glyphs" => options.glyphs_only = true,
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    if options.glyphs_only && options.ligature_font.is_none() {
        usage("--glyphs needs --ligature-font");
    }
    options
}

fn number(text: &str) -> usize {
    match text.parse() {
        Ok(number) if number > 0 => number,
        _ => usage(&format!("{text} is not a positive number")),
    }
}

fn usage(problem: &str) -> ! {
    if !problem.is_empty() {
        eprintln!("code-font-features: {problem}");
    }
    eprintln!(
        "usage: code-font-features [--samples N] [--lines N] [--corpus DIR] [--fonts DIR] \
         [--ligature-font FILE] [--glyphs] [--json FILE]"
    );
    exit(if problem.is_empty() { 0 } else { 2 })
}

fn fail(problem: &str) -> ! {
    eprintln!("code-font-features: {problem}");
    exit(1)
}
