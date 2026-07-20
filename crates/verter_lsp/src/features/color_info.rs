// Color information: extract CSS colors from style blocks for color picker.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Extract color information from CSS style blocks.
///
/// Scans style block content for CSS color values and returns them as
/// `ColorInformation` for the editor's color picker. Supports:
/// - Hex colors: `#rgb`, `#rrggbb`, `#rrggbbaa`
/// - `rgb()`/`rgba()` functions
/// - `hsl()`/`hsla()` functions
///
/// Color chips map ONLY to actual color VALUES: candidates inside comments or
/// strings, and hex-shaped tokens in selector position (`#bad { }` — a valid
/// ID selector that happens to be all hex digits), are never emitted.
pub fn document_colors(
    source: &str,
    blocks: &[SfcBlock],
    line_index: &LineIndex,
) -> Vec<ColorInformation> {
    let mut colors = Vec::new();

    for block in blocks {
        if block.tag_name != "style" {
            continue;
        }

        let (content_start, content_end) = block.content_range();
        let content = match source.get(content_start as usize..content_end as usize) {
            Some(c) => c,
            None => continue,
        };

        // Mask comment/string bytes so candidates inside them never chip.
        let mask = css_comment_string_mask(content);

        // Scan for hex colors
        scan_hex_colors(content, &mask, content_start, line_index, &mut colors);

        // Scan for rgb/rgba/hsl/hsla functions
        scan_color_functions(content, &mask, content_start, line_index, &mut colors);
    }

    colors
}

/// Byte mask over `content`: `true` for bytes inside a `/* ... */` comment or
/// a quoted string.
fn css_comment_string_mask(content: &str) -> Vec<bool> {
    let bytes = content.as_bytes();
    let mut mask = vec![false; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            mask[start..i].iter_mut().for_each(|m| *m = true);
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            mask[start..i].iter_mut().for_each(|m| *m = true);
            continue;
        }
        i += 1;
    }
    mask
}

/// Whether `offset` sits in a declaration VALUE position: scanning backwards
/// (skipping masked comment/string bytes), the nearest structural byte is a
/// `:` — not a `;`, `{`, `}`, or the block start. A hex-shaped ID selector
/// (`#bad {`) fails this test; `color: #f00` passes.
fn is_value_position(content: &str, mask: &[bool], offset: usize) -> bool {
    let bytes = content.as_bytes();
    let mut i = offset;
    while i > 0 {
        i -= 1;
        if mask.get(i).copied().unwrap_or(false) {
            continue;
        }
        match bytes[i] {
            b':' => return true,
            b';' | b'{' | b'}' => return false,
            _ => {}
        }
    }
    false
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

/// Scan for hex color patterns (#rgb, #rrggbb, #rrggbbaa).
fn scan_hex_colors(
    content: &str,
    mask: &[bool],
    base_offset: u32,
    line_index: &LineIndex,
    colors: &mut Vec<ColorInformation>,
) {
    let bytes = content.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'#' && !mask.get(i).copied().unwrap_or(false) {
            let start = i;
            i += 1;

            // Count hex digits
            let hex_start = i;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            let hex_len = i - hex_start;
            let hex = &content[hex_start..i];

            // Validate: next char should not be alphanumeric (not a CSS ID like #app)
            let next_is_ident = i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-');

            // Only chip actual VALUES — a hex-shaped ID selector (`#bad {`)
            // is not a color.
            if !next_is_ident && is_value_position(content, mask, start) {
                if let Some(color) = parse_hex_color(hex, hex_len) {
                    let abs_start = base_offset + start as u32;
                    let abs_end = base_offset + i as u32;
                    if let (Some(s), Some(e)) = (
                        line_index.offset_to_position(abs_start),
                        line_index.offset_to_position(abs_end),
                    ) {
                        colors.push(ColorInformation {
                            range: Range { start: s, end: e },
                            color,
                        });
                    }
                }
            }
        } else {
            i += 1;
        }
    }
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

/// Scan for CSS color function calls: rgb(), rgba(), hsl(), hsla().
fn scan_color_functions(
    content: &str,
    mask: &[bool],
    base_offset: u32,
    line_index: &LineIndex,
    colors: &mut Vec<ColorInformation>,
) {
    for prefix in &["rgba(", "rgb(", "hsla(", "hsl("] {
        let mut search_from = 0;
        while let Some(pos) = content[search_from..].find(prefix) {
            let abs_pos = search_from + pos;
            // Ensure not preceded by alphanumeric (not part of another identifier)
            if abs_pos > 0 && content.as_bytes()[abs_pos - 1].is_ascii_alphanumeric() {
                search_from = abs_pos + prefix.len();
                continue;
            }
            // Never chip inside comments/strings, and only in VALUE position.
            if mask.get(abs_pos).copied().unwrap_or(false)
                || !is_value_position(content, mask, abs_pos)
            {
                search_from = abs_pos + prefix.len();
                continue;
            }

            let args_start = abs_pos + prefix.len();
            if let Some(paren_end) = content[args_start..].find(')') {
                let args = &content[args_start..args_start + paren_end];
                let func_end = args_start + paren_end + 1;

                let color = if prefix.starts_with("rgb") {
                    parse_rgb_args(args)
                } else {
                    parse_hsl_args(args)
                };

                if let Some(color) = color {
                    let abs_start = base_offset + abs_pos as u32;
                    let abs_end = base_offset + func_end as u32;
                    if let (Some(s), Some(e)) = (
                        line_index.offset_to_position(abs_start),
                        line_index.offset_to_position(abs_end),
                    ) {
                        colors.push(ColorInformation {
                            range: Range { start: s, end: e },
                            color,
                        });
                    }
                }

                search_from = func_end;
            } else {
                break;
            }
        }
    }
}

/// Parse rgb/rgba arguments: "255, 128, 0" or "255, 128, 0, 0.5".
fn parse_rgb_args(args: &str) -> Option<Color> {
    let parts: Vec<&str> = args.split(',').map(|s| s.trim()).collect();
    if parts.len() < 3 || parts.len() > 4 {
        return None;
    }

    let r: f32 = parts[0].parse().ok()?;
    let g: f32 = parts[1].parse().ok()?;
    let b: f32 = parts[2].parse().ok()?;
    let a: f32 = if parts.len() == 4 {
        parts[3].parse().ok()?
    } else {
        1.0
    };

    // Normalize: values could be 0-255 or percentages
    let (r, g, b) = if r > 1.0 || g > 1.0 || b > 1.0 {
        (r / 255.0, g / 255.0, b / 255.0)
    } else {
        (r, g, b)
    };

    Some(Color {
        red: r.clamp(0.0, 1.0),
        green: g.clamp(0.0, 1.0),
        blue: b.clamp(0.0, 1.0),
        alpha: a.clamp(0.0, 1.0),
    })
}

/// Parse hsl/hsla arguments: "120, 50%, 50%" or "120, 50%, 50%, 0.8".
fn parse_hsl_args(args: &str) -> Option<Color> {
    let parts: Vec<&str> = args.split(',').map(|s| s.trim()).collect();
    if parts.len() < 3 || parts.len() > 4 {
        return None;
    }

    let h: f32 = parts[0].trim_end_matches("deg").parse().ok()?;
    let s: f32 = parts[1].trim_end_matches('%').parse().ok()?;
    let l: f32 = parts[2].trim_end_matches('%').parse().ok()?;
    let a: f32 = if parts.len() == 4 {
        parts[3].parse().ok()?
    } else {
        1.0
    };

    let (r, g, b) = hsl_to_rgb(h / 360.0, s / 100.0, l / 100.0);

    Some(Color {
        red: r.clamp(0.0, 1.0),
        green: g.clamp(0.0, 1.0),
        blue: b.clamp(0.0, 1.0),
        alpha: a.clamp(0.0, 1.0),
    })
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
    use crate::documents::sfc_scanner::scan_sfc_blocks;

    #[test]
    fn test_hex_color_detection() {
        let source = "<style>\n.foo { color: #ff0000; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!(colors[0].color.green.abs() < 0.01);
        assert!(colors[0].color.blue.abs() < 0.01);
    }

    #[test]
    fn test_short_hex_color() {
        let source = "<style>\n.foo { color: #f00; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_rgb_function() {
        let source = "<style>\n.foo { color: rgb(255, 128, 0); }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!((colors[0].color.green - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_hsl_function() {
        let source = "<style>\n.foo { color: hsl(0, 100%, 50%); }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!(colors[0].color.green.abs() < 0.01);
    }

    #[test]
    fn test_no_colors_in_script() {
        let source = "<script>\nconst color = '#ff0000'\n</script>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
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
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.alpha - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_css_id_not_matched() {
        // #app is a CSS ID selector, not a color
        let source = "<style>\n#app { color: red; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
        assert!(
            colors.is_empty(),
            "CSS ID selector #app should not be detected as color"
        );
    }

    /// A hex-shaped ID selector (`#bad` — all hex digits) is NOT a color;
    /// the value inside its rule IS. Chips map only to actual color values.
    #[test]
    fn hex_shaped_id_selector_never_chips() {
        let source = "<style>\n#bad { color: #f00; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
        assert_eq!(colors.len(), 1, "only the value chips, not the selector");
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
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
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
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let colors = document_colors(source, &blocks, &line_index);
        assert!(colors.is_empty(), "comment/string colors must not chip");
    }
}
