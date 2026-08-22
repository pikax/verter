// Color information: extract CSS colors from style blocks for color picker.

use tower_lsp_server::ls_types::*;

use verter_semantic::analysis::style::{AnalyzedColorCandidate, ColorCandidateKind, NumericArg};
use verter_session::FileAnalysisSnapshot;

use crate::documents::carrier_structure::CarrierBlockView;
use crate::documents::line_index::LineIndex;

/// Extract color information from CSS style blocks.
///
/// Reads pre-classified color-literal candidates from the shared style
/// syntax authority's own parse (`CssAnalysis.declarations[i].color_candidates`,
/// derived from the declaration value's typed `ComponentValueTree`) and
/// returns them as `ColorInformation` for the editor's color picker.
/// Supports:
/// - Hex colors: `#rgb`, `#rrggbb`, `#rrggbbaa`
/// - `rgb()`/`rgba()` functions
/// - `hsl()`/`hsla()` functions
///
/// Color chips map ONLY to actual color VALUES: comment/string exclusion is
/// structural (the parse never visits `ComponentValue::Comment`/`String`
/// content when collecting candidates), and hex-shaped tokens in selector
/// position (`#bad { }`) never appear here at all — declarations are only
/// recorded for rule BODY statements, never selectors.
///
/// **Fail-closed.** The established association authority is the sealed
/// `StyleBlockAnalysis.block_ref`, joined against the block's live
/// `CarrierBlockView.block_ref` exactly as `css/mod.rs::selector_hover`
/// already does. When `analysis` is `None`, OR no `analysis.styles` entry
/// joins to the block's live `block_ref` (stale — the block was
/// reparsed/re-identified since `analysis` was computed), OR a declaration
/// is absent from `declarations` (incomplete/unparsed), that block/
/// declaration contributes ZERO color chips — never a fabricated result
/// from a fallback scan.
pub fn document_colors(
    source: &str,
    blocks: &[CarrierBlockView],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Vec<ColorInformation> {
    let mut colors = Vec::new();

    let Some(analysis) = analysis else {
        return colors;
    };

    for block in blocks {
        if block.tag_name != "style" {
            continue;
        }
        // `<style src="...">` content is external and DEFERRED: Vue ignores
        // any stray inline bytes, so chips must never be fabricated from
        // them — the block is unavailable to CSS features.
        if block.attr("src").is_some() {
            continue;
        }

        // Sealed full-identity join: missing or foreign producer identity
        // (a stale analysis) fails closed rather than mis-binding through a
        // reused ordinal/local id.
        let block_ref = block.block_ref.artifact_block_ref();
        let Some(style) = analysis
            .styles
            .iter()
            .find(|style| style.block_ref.as_ref() == Some(block_ref))
        else {
            continue;
        };
        let Some(css) = style.css.as_ref() else {
            continue;
        };

        for declaration in &css.declarations {
            for candidate in &declaration.color_candidates {
                let Some(color) = color_from_candidate(source, candidate) else {
                    continue;
                };
                if let (Some(s), Some(e)) = (
                    line_index.offset_to_position(candidate.span.start),
                    line_index.offset_to_position(candidate.span.end),
                ) {
                    colors.push(ColorInformation {
                        range: Range { start: s, end: e },
                        color,
                    });
                }
            }
        }
    }

    colors
}

/// Resolve a single pre-classified candidate to a concrete `Color`.
fn color_from_candidate(source: &str, candidate: &AnalyzedColorCandidate) -> Option<Color> {
    match candidate.kind {
        ColorCandidateKind::Hex => {
            // The Hash token's span includes the leading `#`.
            let start = (candidate.span.start as usize).checked_add(1)?;
            let end = candidate.span.end as usize;
            let hex = source.get(start..end)?;
            parse_hex_color(hex, hex.len())
        }
        ColorCandidateKind::Function => {
            let is_rgb = candidate
                .function_name
                .as_deref()
                .is_some_and(|name| name.starts_with("rgb"));
            if is_rgb {
                color_from_rgb_numeric_args(&candidate.numeric_args)
            } else {
                color_from_hsl_numeric_args(&candidate.numeric_args)
            }
        }
    }
}

/// Build a `Color` from `rgb()`/`rgba()`'s own numeric arguments, read
/// directly from the parse's `ComponentValue` tree (comments already
/// excluded structurally at the producer) — never by re-slicing the
/// candidate's raw byte span and `.split(',')`/`.parse()`ing it.
fn color_from_rgb_numeric_args(args: &[NumericArg]) -> Option<Color> {
    if args.len() < 3 || args.len() > 4 {
        return None;
    }

    // Legacy `rgb()`/`rgba()` r/g/b channels have no "0-1 fractional number" form: per CSS Color
    // Level 4, a numeric (non-percentage) channel is ALWAYS on the 0-255 scale, and a
    // `Percentage` channel is ALWAYS `/100` — there is no ambiguity to sniff a magnitude
    // heuristic for now that `NumericArg` already distinguishes `Number` from `Percentage` at the
    // parse layer (the prior `> 1.0` heuristic was a workaround for not having that distinction;
    // it also mis-scaled `rgb(1, 0, 0)`, which per spec is `1/255` red, not full red).
    let channel = |arg: NumericArg| -> f32 {
        match arg {
            NumericArg::Percentage(v) => (v / 100.0) as f32,
            NumericArg::Number(v) => (v / 255.0) as f32,
        }
    };

    let r = channel(args[0]);
    let g = channel(args[1]);
    let b = channel(args[2]);
    let a = args.get(3).copied().map_or(1.0, alpha_channel);

    Some(Color {
        red: r.clamp(0.0, 1.0),
        green: g.clamp(0.0, 1.0),
        blue: b.clamp(0.0, 1.0),
        alpha: a.clamp(0.0, 1.0),
    })
}

/// The alpha channel's own scale (shared by `rgba()` and `hsla()`) — distinct from the
/// r/g/b/h/s/l channels: per CSS Color Level 4 a bare `Number` alpha is ALREADY on the 0-1 scale
/// (`rgba(255, 0, 0, 0.5)`'s alpha is `0.5`, not `0.5 / 255`), while a `Percentage` alpha divides
/// by 100 (`hsla(0, 100%, 50%, 50%)`'s alpha is `0.5`) — both forms must agree for the same
/// magnitude.
fn alpha_channel(arg: NumericArg) -> f32 {
    match arg {
        NumericArg::Number(v) => v as f32,
        NumericArg::Percentage(v) => (v / 100.0) as f32,
    }
}

/// Build a `Color` from `hsl()`/`hsla()`'s own numeric arguments, same
/// source discipline as [`color_from_rgb_numeric_args`]. Hue/saturation/
/// lightness here use the bare magnitude regardless of `Number` vs
/// `Percentage` (matching this function's pre-existing convention — a legacy bare-number
/// saturation/lightness, e.g. `hsl(0, 50, 50%)`, means the same thing as the equivalent
/// `Percentage`, e.g. `hsl(0, 50%, 50%)`, and hue has no percentage form to disambiguate from).
/// The alpha channel is the one HSL argument with a real `Number`-vs-`Percentage` scale
/// distinction — see [`alpha_channel`].
fn color_from_hsl_numeric_args(args: &[NumericArg]) -> Option<Color> {
    if args.len() < 3 || args.len() > 4 {
        return None;
    }

    let magnitude = |arg: NumericArg| -> f32 {
        match arg {
            NumericArg::Number(v) | NumericArg::Percentage(v) => v as f32,
        }
    };

    let h = magnitude(args[0]);
    let s = magnitude(args[1]);
    let l = magnitude(args[2]);
    let a = args.get(3).copied().map_or(1.0, alpha_channel);

    let (r, g, b) = hsl_to_rgb(h / 360.0, s / 100.0, l / 100.0);

    Some(Color {
        red: r.clamp(0.0, 1.0),
        green: g.clamp(0.0, 1.0),
        blue: b.clamp(0.0, 1.0),
        alpha: a.clamp(0.0, 1.0),
    })
}

/// Generate color presentations for a given color.
///
/// Returns the color in hex, rgb, and hsl formats.
pub fn color_presentations(color: &Color) -> Vec<ColorPresentation> {
    let r = (color.red * 255.0).round() as u8;
    let g = (color.green * 255.0).round() as u8;
    let b = (color.blue * 255.0).round() as u8;
    let a = color.alpha;

    let mut presentations = Vec::with_capacity(3);

    // Hex format
    if (a - 1.0).abs() < f32::EPSILON {
        presentations.push(ColorPresentation {
            label: format!("#{r:02x}{g:02x}{b:02x}"),
            text_edit: None,
            additional_text_edits: None,
        });
    } else {
        let alpha_byte = (a * 255.0).round() as u8;
        presentations.push(ColorPresentation {
            label: format!("#{r:02x}{g:02x}{b:02x}{alpha_byte:02x}"),
            text_edit: None,
            additional_text_edits: None,
        });
    }

    // RGB format
    if (a - 1.0).abs() < f32::EPSILON {
        presentations.push(ColorPresentation {
            label: format!("rgb({r}, {g}, {b})"),
            text_edit: None,
            additional_text_edits: None,
        });
    } else {
        presentations.push(ColorPresentation {
            label: format!("rgba({r}, {g}, {b}, {a:.2})"),
            text_edit: None,
            additional_text_edits: None,
        });
    }

    // HSL format
    let (h, s, l) = rgb_to_hsl(color.red, color.green, color.blue);
    let h_deg = (h * 360.0).round() as u32;
    let s_pct = (s * 100.0).round() as u32;
    let l_pct = (l * 100.0).round() as u32;

    if (a - 1.0).abs() < f32::EPSILON {
        presentations.push(ColorPresentation {
            label: format!("hsl({h_deg}, {s_pct}%, {l_pct}%)"),
            text_edit: None,
            additional_text_edits: None,
        });
    } else {
        presentations.push(ColorPresentation {
            label: format!("hsla({h_deg}, {s_pct}%, {l_pct}%, {a:.2})"),
            text_edit: None,
            additional_text_edits: None,
        });
    }

    presentations
}

/// Parse a hex color string (without `#` prefix) into a Color.
fn parse_hex_color(hex: &str, len: usize) -> Option<Color> {
    match len {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some(Color {
                red: (r * 17) as f32 / 255.0,
                green: (g * 17) as f32 / 255.0,
                blue: (b * 17) as f32 / 255.0,
                alpha: 1.0,
            })
        }
        4 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            let a = u8::from_str_radix(&hex[3..4], 16).ok()?;
            Some(Color {
                red: (r * 17) as f32 / 255.0,
                green: (g * 17) as f32 / 255.0,
                blue: (b * 17) as f32 / 255.0,
                alpha: (a * 17) as f32 / 255.0,
            })
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color {
                red: r as f32 / 255.0,
                green: g as f32 / 255.0,
                blue: b as f32 / 255.0,
                alpha: 1.0,
            })
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color {
                red: r as f32 / 255.0,
                green: g as f32 / 255.0,
                blue: b as f32 / 255.0,
                alpha: a as f32 / 255.0,
            })
        }
        _ => None,
    }
}

/// Convert HSL to RGB. All inputs/outputs in 0..1 range.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (l, l, l);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);

    (r, g, b)
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

/// Convert RGB to HSL. All inputs/outputs in 0..1 range.
fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if (max - r).abs() < f32::EPSILON {
        let mut h = (g - b) / d;
        if g < b {
            h += 6.0;
        }
        h
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    (h / 6.0, s, l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::carrier_structure::test_carrier_blocks;

    /// Build a `FileAnalysisSnapshot` whose `styles` entries are sealed to
    /// every `<style>` block's own live `block_ref` — the same join
    /// `document_colors` performs. Mirrors `css/mod.rs`'s test-only
    /// `build_style_for_block` helper (private to that module).
    fn build_analysis(source: &str, blocks: &[CarrierBlockView]) -> FileAnalysisSnapshot {
        let mut styles = Vec::new();
        for block in blocks.iter().filter(|b| b.tag_name == "style") {
            if block.attr("src").is_some() {
                continue;
            }
            let (content_start, content_end) = block.content_range();
            let css_content = &source[content_start as usize..content_end as usize];
            let mut analysis = verter_semantic::analysis::style::build_css_style_analysis(
                css_content,
                verter_semantic::analysis::style::VueStyleInput::default(),
                false,
                false,
                None,
                content_start,
            );
            analysis.block_ref = Some(block.block_ref.artifact_block_ref().clone());
            styles.push(analysis);
        }
        FileAnalysisSnapshot {
            styles: styles.into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_hex_color_detection() {
        let source = "<style>\n.foo { color: #ff0000; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!(colors[0].color.green.abs() < 0.01);
        assert!(colors[0].color.blue.abs() < 0.01);
    }

    /// `<style src="...">` content is external and DEFERRED: any stray inline
    /// bytes inside the block are ignored by Vue (the external file replaces
    /// the block content), so color chips must never be fabricated from them —
    /// the block is unavailable to CSS features, not an empty success.
    #[test]
    fn external_src_style_block_yields_no_color_chips() {
        let source = "<style src=\"./theme.css\">\n.stray { color: #ff0000; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert!(
            colors.is_empty(),
            "external src style must be unavailable, never fabricated chips: {colors:?}"
        );
    }

    #[test]
    fn test_short_hex_color() {
        let source = "<style>\n.foo { color: #f00; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_rgb_function() {
        let source = "<style>\n.foo { color: rgb(255, 128, 0); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!((colors[0].color.green - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_hsl_function() {
        let source = "<style>\n.foo { color: hsl(0, 100%, 50%); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!(colors[0].color.green.abs() < 0.01);
    }

    #[test]
    fn test_no_colors_in_script() {
        let source = "<script>\nconst color = '#ff0000'\n</script>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert!(colors.is_empty());
    }

    #[test]
    fn test_color_presentations() {
        let color = Color {
            red: 1.0,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        };
        let presentations = color_presentations(&color);
        assert_eq!(presentations.len(), 3);
        assert_eq!(presentations[0].label, "#ff0000");
        assert_eq!(presentations[1].label, "rgb(255, 0, 0)");
        assert!(presentations[2].label.starts_with("hsl("));
    }

    #[test]
    fn test_hex_with_alpha() {
        let source = "<style>\n.foo { color: #ff000080; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.alpha - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_css_id_not_matched() {
        // #app is a CSS ID selector, not a color
        let source = "<style>\n#app { color: red; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert!(
            colors.is_empty(),
            "CSS ID selector #app should not be detected as color"
        );
    }

    /// A hex-shaped ID selector (`#bad` — all hex digits) is NOT a color;
    /// the value inside its rule IS. Chips map only to actual color values.
    /// (Structural: declarations are only recorded for rule BODY statements,
    /// so a selector can never contribute a candidate in the first place.)
    #[test]
    fn hex_shaped_id_selector_never_chips() {
        let source = "<style>\n#bad { color: #f00; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1, "only the value chips, not the selector");
        let start_off = line_index
            .position_to_offset(&colors[0].range.start)
            .unwrap() as usize;
        let end_off = line_index.position_to_offset(&colors[0].range.end).unwrap() as usize;
        assert_eq!(&source[start_off..end_off], "#f00");
    }

    /// A pseudo-class colon (`a:hover`) is NOT a declaration colon: a
    /// hex-shaped ID selector after a pseudo-class never chips; the value
    /// inside the rule still does.
    #[test]
    fn pseudo_class_colon_never_makes_a_selector_a_value_position() {
        let source = "<style>\na:hover #bad { color: #f00; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(
            colors.len(),
            1,
            "only the value chips, not the pseudo-class'd selector"
        );
        let start_off = line_index
            .position_to_offset(&colors[0].range.start)
            .unwrap() as usize;
        let end_off = line_index.position_to_offset(&colors[0].range.end).unwrap() as usize;
        assert_eq!(&source[start_off..end_off], "#f00");
    }

    /// The FIRST rule's chip range maps exactly onto the color VALUE — never
    /// onto the class name above it (the observed decorator defect).
    #[test]
    fn first_rule_chip_maps_exactly_to_the_color_value() {
        let source = "<style>\n.first {\n  color: #abc;\n}\n.second {\n  background: rgb(1, 2, 3);\n}\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 2);
        let texts: Vec<&str> = colors
            .iter()
            .map(|c| {
                let s = line_index.position_to_offset(&c.range.start).unwrap() as usize;
                let e = line_index.position_to_offset(&c.range.end).unwrap() as usize;
                &source[s..e]
            })
            .collect();
        assert!(texts.contains(&"#abc"), "got {texts:?}");
        assert!(texts.contains(&"rgb(1, 2, 3)"), "got {texts:?}");
        // Negative: no chip range starts on a class-name line.
        for c in &colors {
            let s = line_index.position_to_offset(&c.range.start).unwrap() as usize;
            assert!(
                !source[s..].starts_with(".first") && !source[s..].starts_with(".second"),
                "a chip must never land on a selector"
            );
        }
    }

    /// Colors inside comments and strings never chip.
    #[test]
    fn colors_in_comments_and_strings_never_chip() {
        let source =
            "<style>\n/* #fff */\n.x { content: '#0f0'; }\n.y { /* rgb(1,2,3) */ color: red; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert!(colors.is_empty(), "comment/string colors must not chip");
    }

    /// Discriminating positive (A22): a comment INSIDE a color function's
    /// argument list. Verified by direct execution of the pre-change
    /// `scan_color_functions`/`parse_rgb_args` logic against this exact
    /// input: it locates `rgb(` correctly (unmasked, in value position),
    /// but `parse_rgb_args` parses the RAW argument substring including the
    /// comment text, so `.parse::<f32>()` fails and NO chip was emitted —
    /// a genuine false negative for an unambiguous literal color. Reading
    /// `numeric_args` (which skips `Comment` entries structurally at the
    /// producer) correctly extracts `(255, 0, 0)` and emits the chip.
    #[test]
    fn document_colors_comment_inside_color_function_args_still_chips() {
        let source = "<style>\n.foo { color: rgb(255, /* not blue */ 0, 0); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(
            colors.len(),
            1,
            "a comment between color function arguments must not suppress the chip"
        );
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!(colors[0].color.green.abs() < 0.01);
        assert!(colors[0].color.blue.abs() < 0.01);
    }

    /// Fail-closed (A22): `analysis: None` emits zero chips, never a
    /// fabricated result from a fallback scan.
    #[test]
    fn document_colors_none_analysis_fails_closed() {
        let source = "<style>\n.foo { color: #ff0000; }\n</style>";
        let blocks = test_carrier_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, None, &line_index);
        assert!(colors.is_empty(), "analysis: None must fail closed");
    }

    /// Fail-closed (A22): a STALE `analysis` whose `styles[].block_ref` does
    /// not match the live block's `block_ref` emits zero chips — never the
    /// stale analysis's own chips. Mirrors
    /// `css/mod.rs::selector_hover_refuses_stale_artifact_analysis_with_matching_local_id`.
    #[test]
    fn document_colors_stale_analysis_block_ref_mismatch_fails_closed() {
        let current = "<style>.foo{color:#f00}</style>";
        let stale = "<style>.foo{color:#0f0}</style>";
        let blocks = test_carrier_blocks(current);
        let stale_blocks = test_carrier_blocks(stale);

        // Build the stale analysis sealed to the STALE artifact's own block_ref.
        let analysis = build_analysis(stale, &stale_blocks);
        let line_index = LineIndex::new_utf16(current);

        let colors = document_colors(current, &blocks, Some(&analysis), &line_index);
        assert!(
            colors.is_empty(),
            "a stale analysis must fail closed, never mis-bind through a naked ordinal: {colors:?}"
        );
    }

    /// A declaration whose OWN value parses cleanly still chips even when its
    /// enclosing rule is unterminated at EOF (`Recover` mode marks only the
    /// still-open block/rule frame `StyleCompleteness::Recovered`; a child
    /// declaration that already finished before the missing `}` was
    /// discovered keeps `Complete` — verified directly against
    /// `verter_css_syntax::style_ir`'s per-node completeness determination).
    /// This is live-editing feedback, not a fail-closed case: the value text
    /// itself is unambiguous, so withholding the chip would be a false
    /// negative, not a safety property.
    #[test]
    fn document_colors_declaration_in_unterminated_rule_still_chips() {
        let source = "<style>\n.foo { color: #f00\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(
            colors.len(),
            1,
            "a declaration that itself parsed cleanly must still chip even though \
             its enclosing rule is unterminated: {colors:?}"
        );
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!(colors[0].color.green.abs() < 0.01);
        assert!(colors[0].color.blue.abs() < 0.01);
    }

    /// Fail-closed (A22): a declaration whose OWN parse hits a diagnostic
    /// while its own frame is still open — an unterminated function inside
    /// the value (`rgb(` never closed before the rule's `}`) — marks the
    /// `Declaration` node itself `StyleCompleteness::Recovered` (not merely
    /// an ancestor), so it is absent from `CssAnalysis.declarations` and
    /// contributes zero chips. Distinct from the EOF-after-clean-value case
    /// above, where the declaration's own frame already closed `Complete`
    /// before the enclosing rule's missing-`}` diagnostic fired.
    #[test]
    fn document_colors_incomplete_declaration_fails_closed() {
        let source = "<style>\n.foo { color: rgb( }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert!(
            colors.is_empty(),
            "a declaration whose own parse recovered from a diagnostic must contribute \
             zero chips: {colors:?}"
        );
    }

    /// A22: CSS relative-color syntax (`rgb(from red 255 0 0)`) is out of scope and must not
    /// fabricate a chip. The pre-fix producer silently skipped the `from`/`red` identifiers and
    /// still extracted `[255, 0, 0]` from the surrounding numbers, so `document_colors` wrongly
    /// chipped a shape it does not actually support.
    #[test]
    fn document_colors_relative_color_syntax_never_chips() {
        let source = "<style>\n.foo { color: rgb(from red 255 0 0); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert!(
            colors.is_empty(),
            "relative-color syntax must not fabricate a chip: {colors:?}"
        );
    }

    /// A22: a nested math function (`calc()`) inside a color function's argument list is out of
    /// scope and must not fabricate a chip either.
    #[test]
    fn document_colors_nested_calc_never_chips() {
        let source = "<style>\n.foo { color: rgb(calc(255), 0, 0); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert!(
            colors.is_empty(),
            "a nested calc() argument must not fabricate a chip: {colors:?}"
        );
    }

    /// A22: `rgb(100%, 0%, 0%)` is pure red at the PERCENTAGE scale (`/100`), not the 0-255
    /// scale. The pre-fix producer discarded percentage-ness after parsing, so the `r > 1.0`
    /// 0-255-vs-0-1 heuristic misread `100.0 > 1.0` as "0-255 scale" and divided by 255 instead
    /// of 100, producing a near-black color instead of red.
    #[test]
    fn document_colors_rgb_percentage_args_use_percentage_scale() {
        let source = "<style>\n.foo { color: rgb(100%, 0%, 0%); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1, "got {colors:?}");
        assert!(
            (colors[0].color.red - 1.0).abs() < 0.01,
            "expected full red, got {:?}",
            colors[0].color
        );
        assert!(colors[0].color.green.abs() < 0.01);
        assert!(colors[0].color.blue.abs() < 0.01);
    }

    /// A22 round 3: a numeric (non-percentage) `rgb()` channel has no "0-1 fractional number"
    /// form — per spec it is ALWAYS the 0-255 scale, even when its magnitude is `<= 1.0`. The
    /// pre-fix `> 1.0` magnitude-sniffing heuristic misread `rgb(1, 0, 0)` as already-full-scale
    /// (`1 <= 1.0`) and produced full red instead of a near-black `1/255` red.
    #[test]
    fn document_colors_rgb_plain_number_channel_is_always_0_255_scale() {
        let source = "<style>\n.foo { color: rgb(1, 0, 0); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1, "got {colors:?}");
        assert!(
            (colors[0].color.red - 1.0 / 255.0).abs() < 0.001,
            "expected r ≈ 1/255, got {:?}",
            colors[0].color
        );
        assert!(
            (colors[0].color.red - 1.0).abs() > 0.01,
            "must NOT be full red: {:?}",
            colors[0].color
        );
    }

    /// A22 round 3: a mixed `rgb(100%, 1, 0%)` — the plain-number green channel must still use
    /// the 0-255 scale (`1/255`) even though the red channel is a `Percentage` at `100.0`, which
    /// the old cross-channel `> 1.0` heuristic would have (wrongly) used to decide the whole
    /// channel scale.
    #[test]
    fn document_colors_rgb_mixed_percentage_and_plain_number_channels() {
        let source = "<style>\n.foo { color: rgb(100%, 1, 0%); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1, "got {colors:?}");
        let c = &colors[0].color;
        assert!(
            (c.red - 1.0).abs() < 0.01,
            "expected full red channel, got {c:?}"
        );
        assert!(
            (c.green - 1.0 / 255.0).abs() < 0.001,
            "expected g ≈ 1/255, got {c:?}"
        );
        assert!(c.blue.abs() < 0.01, "expected zero blue, got {c:?}");
    }

    /// A22 round 3: `rgba()`'s alpha channel is on its OWN scale (a bare `Number` is already
    /// 0-1), never the r/g/b channel scale. The pre-fix code divided alpha by the SAME 0-255-or-
    /// 0-1 decision as r/g/b, so `rgba(255, 0, 0, 0.5)` produced alpha ≈ `0.5 / 255` instead of
    /// `0.5`.
    #[test]
    fn document_colors_rgba_number_alpha_is_0_1_scale_not_255() {
        let source = "<style>\n.foo { color: rgba(255, 0, 0, 0.5); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 1, "got {colors:?}");
        assert!(
            (colors[0].color.alpha - 0.5).abs() < 0.001,
            "expected alpha == 0.5, got {:?}",
            colors[0].color
        );
    }

    /// A22 round 3: `hsla()`'s `Number` alpha and `Percentage` alpha must agree for the same
    /// magnitude (`50%` == `0.5`), and neither divides by the h/s/l scale.
    #[test]
    fn document_colors_hsla_number_and_percentage_alpha_agree() {
        let source = "<style>\n.a { color: hsla(0, 100%, 50%, 50%); }\n.b { color: hsla(0, 100%, 50%, 0.5); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 2, "got {colors:?}");
        for c in &colors {
            assert!(
                (c.color.alpha - 0.5).abs() < 0.001,
                "expected alpha == 0.5 for both `Percentage` and `Number` alpha forms, got {:?}",
                c.color
            );
        }
    }

    /// Positive control pairing the relative-color/calc/percentage cases above: plain `rgb()`
    /// and comment-containing `rgb()` shapes must keep chipping unchanged.
    #[test]
    fn document_colors_plain_and_comment_rgb_still_chip() {
        let source = "<style>\n.a { color: rgb(255, 0, 0); }\n.b { color: rgb(255, /* x */ 0, 0); }\n</style>";
        let blocks = test_carrier_blocks(source);
        let analysis = build_analysis(source, &blocks);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, Some(&analysis), &line_index);
        assert_eq!(colors.len(), 2, "got {colors:?}");
        for c in &colors {
            assert!(
                (c.color.red - 1.0).abs() < 0.01,
                "expected full red, got {:?}",
                c.color
            );
            assert!(c.color.green.abs() < 0.01);
            assert!(c.color.blue.abs() < 0.01);
        }
    }
}
