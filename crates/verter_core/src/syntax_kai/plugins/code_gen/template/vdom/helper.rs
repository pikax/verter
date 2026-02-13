use crate::utils::vue::{PatchFlag, PatchFlags};

/// Build the patch flag + dynamic props suffix for an element close.
///
/// Returns something like `, 9 /* TEXT, PROPS */, ["id"]` or an empty string
/// if the patch flag is zero.
pub fn build_patch_flag_suffix(
    patch_flag: PatchFlag,
    dynamic_props: &[&str],
    is_production: bool,
) -> String {
    if patch_flag.0 == 0 {
        return String::new();
    }
    let mut suffix = String::new();
    write_patch_flag_suffix(&mut suffix, patch_flag, dynamic_props, is_production);
    suffix
}

/// Append the patch flag + dynamic props suffix into an existing buffer.
/// Avoids allocating a new String when the caller already has a buffer.
pub fn write_patch_flag_suffix(
    buf: &mut String,
    patch_flag: PatchFlag,
    dynamic_props: &[&str],
    is_production: bool,
) {
    if patch_flag.0 == 0 {
        return;
    }

    buf.push_str(", ");

    // Numeric value — inline formatting avoids std::fmt machinery overhead.
    push_i16(buf, patch_flag.0);

    // Dev-mode comment with flag names
    if !is_production {
        buf.push_str(" /* ");
        let mut first = true;
        for name in patch_flag_names_iter(patch_flag) {
            if !first {
                buf.push_str(", ");
            }
            buf.push_str(name);
            first = false;
        }
        buf.push_str(" */");
    }

    // Dynamic props array
    if !dynamic_props.is_empty() {
        buf.push_str(", [");
        for (i, prop) in dynamic_props.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            // Dynamic arg expressions (e.g. `"on" + _ctx.event`) already start
            // with `"` — emit them verbatim without extra quoting.
            if prop.starts_with('"') {
                buf.push_str(prop);
            } else {
                buf.push('"');
                buf.push_str(prop);
                buf.push('"');
            }
        }
        buf.push(']');
    }
}

/// Fast signed integer-to-string append without std::fmt overhead.
#[inline]
fn push_i16(buf: &mut String, n: i16) {
    if n < 0 {
        buf.push('-');
        push_u32(buf, (-(n as i32)) as u32);
    } else {
        push_u32(buf, n as u32);
    }
}

/// Fast unsigned integer-to-string append without std::fmt overhead.
#[inline]
pub(crate) fn push_u32(buf: &mut String, mut n: u32) {
    if n == 0 {
        buf.push('0');
        return;
    }
    // Max u32 is 4294967295 (10 digits)
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    while n > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    // SAFETY: digits are always valid ASCII
    buf.push_str(unsafe { std::str::from_utf8_unchecked(&tmp[i..]) });
}

/// Iterate flag names without Vec allocation.
fn patch_flag_names_iter(flag: PatchFlag) -> impl Iterator<Item = &'static str> {
    const ALL_FLAGS: [PatchFlags; 12] = [
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

    let is_special = flag.is_special();
    let special_name = flag.name();

    std::iter::once(special_name)
        .take(if is_special { 1 } else { 0 })
        .chain(
            ALL_FLAGS
                .iter()
                .filter(move |f| !is_special && flag.contains(**f))
                .map(|f| f.name()),
        )
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
        let result = build_patch_flag_suffix(flag, &["id"], false);
        assert_eq!(result, ", 9 /* TEXT, PROPS */, [\"id\"]");
    }

    #[test]
    fn test_build_patch_flag_suffix_production() {
        let flag = PatchFlags::Text.into_flag().add(PatchFlags::Props);
        let result = build_patch_flag_suffix(flag, &["id"], true);
        assert_eq!(result, ", 9, [\"id\"]");
    }
}
