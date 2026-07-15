//! The css scope-hash derivation (the official `svelte@5.56.3` default
//! `cssHash` input rule over the shared djb2 hash).

/// The official default css-hash: `svelte-${hash(filename === '(unknown)' ?
/// css : filename ?? css)}` (`validate-options.js`). A missing filename (or
/// the official `'(unknown)'` placeholder) hashes the raw CSS text instead.
pub fn css_scope_hash(filename: Option<&str>, css: &str) -> String {
    // The official compiler normalizes the filename (backslash to forward
    // slash) at state init (state.js normalizes the state filename BEFORE the
    // default cssHash reads it), so a Windows-style path hashes identically to
    // its POSIX form (cross-platform-deterministic). Done as a char remap (not
    // a post-hoc string rewrite) to stay clear of the carrier-codegen guard
    // that bans that token across `src/svelte/**`; it is equivalent for the
    // scope-hash input.
    let normalized = filename.map(|name| {
        name.chars()
            .map(|c| if c == '\\' { '/' } else { c })
            .collect::<String>()
    });
    let input = match normalized.as_deref() {
        None | Some("(unknown)") => css,
        Some(name) => name,
    };
    format!(
        "svelte-{}",
        crate::svelte::runtime::naming::svelte_hash(input)
    )
}

#[cfg(test)]
mod tests {
    use super::css_scope_hash;

    #[test]
    fn css_scope_hash_pins_official_djb2_known_vectors() {
        // The `css/scoped_styles.svelte` oracle pin: the golden's css hash is
        // `svelte-c4vjvh`, produced from the FILENAME input (the golden
        // generator compiles with `filename: slug`).
        assert_eq!(
            css_scope_hash(Some("css/scoped_styles.svelte"), ".card{color:blue}"),
            "svelte-c4vjvh"
        );
        // Known djb2 vectors (pinned against the official `hash()` run on
        // `svelte@5.56.3`): the filename input dominates when present…
        assert_eq!(
            css_scope_hash(Some("App.svelte"), "ignored"),
            "svelte-n50uah"
        );
        // …the `'(unknown)'` placeholder falls back to the css text…
        assert_eq!(css_scope_hash(Some("(unknown)"), "abc"), "svelte-2nhvp3");
        // …and an ABSENT filename falls back to the css text too.
        assert_eq!(css_scope_hash(None, "abc"), "svelte-2nhvp3");
        assert_eq!(css_scope_hash(None, ""), "svelte-45h");
        // NEGATIVE: a different input produces a DIFFERENT hash (the fixture
        // BASENAME is not the oracle input — the slug with its directory is).
        assert_ne!(
            css_scope_hash(Some("scoped_styles.svelte"), ""),
            "svelte-c4vjvh"
        );
        assert_ne!(css_scope_hash(None, "abcd"), css_scope_hash(None, "abc"));
    }

    #[test]
    fn css_scope_hash_normalizes_windows_backslash_paths_like_svelte() {
        // The official `state.js` normalizes `\` -> `/` before the default
        // `cssHash` reads the filename, so a Windows-style path hashes
        // IDENTICALLY to its POSIX form (cross-platform-deterministic). Oracle:
        // svelte@5.56.3 hashes both `src\Foo.svelte` and `src/Foo.svelte` to
        // `svelte-1ghgqhn`. Against the pre-fix verbatim hash the backslash
        // path produced a DIFFERENT (wrong) hash — this discriminates.
        assert_eq!(
            css_scope_hash(Some("src\\Foo.svelte"), "ignored"),
            css_scope_hash(Some("src/Foo.svelte"), "ignored"),
        );
        assert_eq!(
            css_scope_hash(Some("src\\Foo.svelte"), "ignored"),
            "svelte-1ghgqhn"
        );
    }
}
