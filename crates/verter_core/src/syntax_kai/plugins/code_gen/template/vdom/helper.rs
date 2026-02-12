use crate::utils::vue::{PatchFlag, PatchFlags};

/// Build the patch flag + dynamic props suffix for an element close.
///
/// Returns something like `, 9 /* TEXT, PROPS */, ["id"]` or an empty string
/// if the patch flag is zero.
pub fn build_patch_flag_suffix(
    patch_flag: PatchFlag,
    dynamic_props: &[String],
    is_production: bool,
) -> String {
    if patch_flag.0 == 0 {
        return String::new();
    }

    let mut suffix = String::new();
    suffix.push_str(", ");

    // Numeric value
    suffix.push_str(&patch_flag.0.to_string());

    // Dev-mode comment with flag names
    if !is_production {
        suffix.push_str(" /* ");
        let names = patch_flag_names(patch_flag);
        suffix.push_str(&names.join(", "));
        suffix.push_str(" */");
    }

    // Dynamic props array
    if !dynamic_props.is_empty() {
        suffix.push_str(", [");
        for (i, prop) in dynamic_props.iter().enumerate() {
            if i > 0 {
                suffix.push_str(", ");
            }
            // Dynamic arg expressions (e.g. `"on" + _ctx.event`) already start
            // with `"` — emit them verbatim without extra quoting.
            if prop.starts_with('"') {
                suffix.push_str(prop);
            } else {
                suffix.push('"');
                suffix.push_str(prop);
                suffix.push('"');
            }
        }
        suffix.push(']');
    }

    suffix
}

/// Returns the list of flag names set in a PatchFlag bitmask.
fn patch_flag_names(flag: PatchFlag) -> Vec<&'static str> {
    if flag.is_special() {
        return vec![flag.name()];
    }

    let all_flags = [
        PatchFlags::Text,
        PatchFlags::Class,
        PatchFlags::Style,
        PatchFlags::Props,
        PatchFlags::FullProps,
        PatchFlags::NeedHydration,
        PatchFlags::StableFragment,
        PatchFlags::KeyedFragment,
        PatchFlags::UnkeyedFragment,
        PatchFlags::NeedPatch,
        PatchFlags::DynamicSlots,
        PatchFlags::DevRootFragment,
    ];

    let mut names = Vec::new();
    for f in all_flags {
        if flag.contains(f) {
            names.push(f.name());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_patch_flag_suffix_empty() {
        assert_eq!(build_patch_flag_suffix(PatchFlag::empty(), &[], false), "");
    }

    #[test]
    fn test_build_patch_flag_suffix_single_flag_dev() {
        let flag = PatchFlags::Text.into_flag();
        let result = build_patch_flag_suffix(flag, &[], false);
        assert_eq!(result, ", 1 /* TEXT */");
    }

    #[test]
    fn test_build_patch_flag_suffix_combined_dev() {
        let flag = PatchFlags::Text.into_flag().add(PatchFlags::Props);
        let result = build_patch_flag_suffix(flag, &["id".to_string()], false);
        assert_eq!(result, ", 9 /* TEXT, PROPS */, [\"id\"]");
    }

    #[test]
    fn test_build_patch_flag_suffix_production() {
        let flag = PatchFlags::Text.into_flag().add(PatchFlags::Props);
        let result = build_patch_flag_suffix(flag, &["id".to_string()], true);
        assert_eq!(result, ", 9, [\"id\"]");
    }
}
