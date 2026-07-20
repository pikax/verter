//! xxh3-based content hashing, semantic hashing, and profile/style-override hashing.
//!
//! ## Hash Algorithm Rationale
//!
//! Three hash algorithms serve different purposes in the Verter codebase:
//!
//! | Algorithm | Used By | Purpose | Persisted? |
//! |-----------|---------|---------|------------|
//! | **XXH3-128** | `verter_session` | Content and semantic hashing for compile cache invalidation | No (in-process only) |
//! | **SHA-256** | `verter_compiler`, `verter_semantic::analysis` | Scope IDs (`data-v-{hash}`), CSS Modules class names, export signatures, type resolution fingerprinting | Scope IDs and CSS Modules output **yes** (embedded in compiled CSS/HTML); export/type sigs **no** |
//! | **DefaultHasher** | `verter_session` | `CompileProfile` and `StyleOverride` cache keys | No (in-process, non-deterministic across Rust versions) |
//!
//! SHA-256 is used for scope IDs and CSS Modules to match `@vue/compiler-sfc` output.
//! XXH3 is used for internal cache invalidation where speed matters more than compatibility.
//! DefaultHasher is used only for transient in-process cache keys — never persisted.
//! FxHash (`rustc-hash`) is used for in-memory `HashMap`/`HashSet` throughout (not shown here).

use std::hash::{Hash, Hasher};

use rustc_hash::FxHashMap;

use crate::types::{
    CompileProfile, ContentOverride, DescriptorMin, Hash16, SliceHashes, StyleOverrideEntry,
};

pub(crate) fn hash_16(input: &[u8]) -> Hash16 {
    xxhash_rust::xxh3::xxh3_128(input).to_le_bytes()
}

pub(crate) fn semantic_hash(slices: &SliceHashes, descriptor: &DescriptorMin) -> Hash16 {
    // Build a buffer of all the data to hash, then hash once.
    let mut buf = Vec::with_capacity(128);
    if let Some(script) = slices.script {
        buf.extend_from_slice(&script);
    }
    if let Some(template) = slices.template {
        buf.extend_from_slice(&template);
    }
    for h in &slices.styles {
        buf.extend_from_slice(h);
    }
    for h in &slices.custom {
        buf.extend_from_slice(h);
    }
    buf.extend_from_slice(&descriptor.script_count.to_le_bytes());
    buf.extend_from_slice(&descriptor.template_count.to_le_bytes());
    buf.extend_from_slice(&descriptor.style_count.to_le_bytes());
    buf.extend_from_slice(&descriptor.custom_count.to_le_bytes());
    buf.push(descriptor.vapor as u8);
    for fp in &descriptor.script_attr_fingerprints {
        buf.extend_from_slice(fp.as_bytes());
        buf.push(0);
    }
    for fp in &descriptor.template_attr_fingerprints {
        buf.extend_from_slice(fp.as_bytes());
        buf.push(0);
    }
    for fp in &descriptor.style_attr_fingerprints {
        buf.extend_from_slice(fp.as_bytes());
        buf.push(0);
    }
    for fp in &descriptor.custom_attr_fingerprints {
        buf.extend_from_slice(fp.as_bytes());
        buf.push(0);
    }
    xxhash_rust::xxh3::xxh3_128(&buf).to_le_bytes()
}

/// Hash a CompileProfile to a u64 for use as an in-memory cache key.
/// Uses DefaultHasher which is NOT guaranteed stable across Rust versions —
/// these hashes must never be persisted or compared across process restarts.
pub(crate) fn compile_profile_hash(profile: &CompileProfile) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    profile.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn style_override_hash(overrides: &FxHashMap<usize, StyleOverrideEntry>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut entries: Vec<_> = overrides.iter().collect();
    entries.sort_by_key(|(idx, _)| **idx);
    for (idx, entry) in entries {
        idx.hash(&mut hasher);
        entry.code.as_ref().hash(&mut hasher);
        if let Some(sm) = &entry.source_map {
            sm.as_ref().hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Hash the content of template/script overrides for cache invalidation.
pub(crate) fn content_override_hash(
    template: Option<&ContentOverride>,
    script: Option<&ContentOverride>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Some(t) = template {
        "template".hash(&mut hasher);
        t.code.as_ref().hash(&mut hasher);
        if let Some(sm) = &t.source_map {
            sm.as_ref().hash(&mut hasher);
        }
    }
    if let Some(s) = script {
        "script".hash(&mut hasher);
        s.code.as_ref().hash(&mut hasher);
        if let Some(sm) = &s.source_map {
            sm.as_ref().hash(&mut hasher);
        }
    }
    hasher.finish()
}

pub(crate) fn diff_indices<T: PartialEq>(old: &[T], new: &[T]) -> Vec<usize> {
    let max = old.len().max(new.len());
    let mut out = Vec::new();
    for i in 0..max {
        if old.get(i) != new.get(i) {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_indices_same_content() {
        let a = vec![1, 2, 3];
        let b = vec![1, 2, 3];
        assert!(diff_indices(&a, &b).is_empty());
    }

    #[test]
    fn diff_indices_one_changed() {
        let a = vec![1, 2, 3];
        let b = vec![1, 9, 3];
        assert_eq!(diff_indices(&a, &b), vec![1]);
    }

    #[test]
    fn diff_indices_new_longer() {
        let a = vec![1, 2];
        let b = vec![1, 2, 3];
        assert_eq!(diff_indices(&a, &b), vec![2]);
    }

    #[test]
    fn diff_indices_old_longer() {
        let a = vec![1, 2, 3];
        let b = vec![1, 2];
        assert_eq!(diff_indices(&a, &b), vec![2]);
    }

    #[test]
    fn diff_indices_both_empty() {
        let a: Vec<i32> = vec![];
        let b: Vec<i32> = vec![];
        assert!(diff_indices(&a, &b).is_empty());
    }

    // ═══════════════════════════════════════════════════════════
    // Additional hash tests
    // ═══════════════════════════════════════════════════════════

    use std::sync::Arc;

    /// @ai-generated - semantic_hash: None script ≠ Some(empty) script
    #[test]
    fn semantic_hash_none_vs_empty_script() {
        let slices_none = SliceHashes {
            script: None,
            ..SliceHashes::default()
        };
        let slices_empty = SliceHashes {
            script: Some(hash_16(b"")),
            ..SliceHashes::default()
        };
        let desc_none = DescriptorMin::default();
        let desc_some = DescriptorMin {
            script_count: 1,
            ..DescriptorMin::default()
        };
        let h_none = semantic_hash(&slices_none, &desc_none);
        let h_empty = semantic_hash(&slices_empty, &desc_some);
        assert_ne!(h_none, h_empty);
    }

    /// @ai-generated - compile_profile_hash: same profile → same hash
    #[test]
    fn compile_profile_hash_stability() {
        let profile = CompileProfile {
            is_production: true,
            hmr_strategy: crate::HmrStrategy::Vite,
            ..CompileProfile::default()
        };
        let h1 = compile_profile_hash(&profile);
        let h2 = compile_profile_hash(&profile);
        assert_eq!(h1, h2);
    }

    /// @ai-generated - Adding scoped attribute changes semantic hash
    /// even with identical style content
    #[test]
    fn semantic_hash_changes_with_attribute_fingerprint() {
        let slices = SliceHashes {
            styles: vec![hash_16(b".a{}")],
            ..SliceHashes::default()
        };
        let desc_no_scoped = DescriptorMin {
            style_count: 1,
            style_attr_fingerprints: vec!["lang=css\n".to_string()],
            ..DescriptorMin::default()
        };
        let desc_scoped = DescriptorMin {
            style_count: 1,
            style_attr_fingerprints: vec!["lang=css\nscoped=true\n".to_string()],
            ..DescriptorMin::default()
        };
        let h1 = semantic_hash(&slices, &desc_no_scoped);
        let h2 = semantic_hash(&slices, &desc_scoped);
        assert_ne!(
            h1, h2,
            "adding scoped attribute should change semantic hash"
        );
    }

    /// @ai-generated - hash_16 is deterministic and produces distinct hashes
    #[test]
    fn hash_16_deterministic_and_distinct() {
        let h1 = hash_16(b"hello");
        let h2 = hash_16(b"hello");
        let h3 = hash_16(b"world");
        assert_eq!(h1, h2, "same input should produce same hash");
        assert_ne!(h1, h3, "different inputs should produce different hashes");
        assert_ne!(
            h1, [0u8; 16],
            "hash should not be all zeros for non-empty input"
        );
    }

    /// @ai-generated - Different CompileProfile fields produce different hashes
    #[test]
    fn compile_profile_hash_differs_for_different_profiles() {
        let base = CompileProfile::default();
        let ssr = CompileProfile {
            ssr: true,
            ..CompileProfile::default()
        };
        let prod = CompileProfile {
            is_production: true,
            ..CompileProfile::default()
        };
        let custom_element = CompileProfile {
            custom_element: true,
            ..CompileProfile::default()
        };
        let sourcemap = CompileProfile {
            source_map: true,
            ..CompileProfile::default()
        };

        let h_base = compile_profile_hash(&base);
        let h_ssr = compile_profile_hash(&ssr);
        let h_prod = compile_profile_hash(&prod);
        let h_custom_element = compile_profile_hash(&custom_element);
        let h_sm = compile_profile_hash(&sourcemap);

        assert_ne!(h_base, h_ssr, "ssr diff should produce different hash");
        assert_ne!(
            h_base, h_prod,
            "production diff should produce different hash"
        );
        assert_ne!(
            h_base, h_custom_element,
            "custom-element script policy must produce a separate compile slot"
        );
        assert_ne!(
            h_base, h_sm,
            "source_map diff should produce different hash"
        );
    }

    #[test]
    fn compile_profile_hash_folds_the_svelte_css_hash_override() {
        // cssHash cache identity: the resolved Svelte cssHash override is a
        // COMPILE-OUTPUT POLICY dimension, so two profiles differing ONLY in the
        // override MUST produce different profile hashes — the session compile
        // slot is keyed by this u64 (and the Content-mode key embeds it), so two
        // overrides over identical source can NEVER share a cached output.
        let base = CompileProfile::default();
        let override_a = CompileProfile {
            svelte_css_hash_override: Some("svelte-A".to_string()),
            ..CompileProfile::default()
        };
        let override_b = CompileProfile {
            svelte_css_hash_override: Some("svelte-B".to_string()),
            ..CompileProfile::default()
        };
        let h_base = compile_profile_hash(&base);
        let h_a = compile_profile_hash(&override_a);
        let h_b = compile_profile_hash(&override_b);
        assert_ne!(h_base, h_a, "a present override must move the profile hash");
        assert_ne!(
            h_a, h_b,
            "distinct overrides must produce distinct profile hashes (no shared slot)"
        );
        // Determinism: the same override hashes identically.
        assert_eq!(h_a, compile_profile_hash(&override_a));
    }

    /// @ai-generated - style_override_hash: insertion order doesn't matter
    #[test]
    fn style_override_hash_order_independent() {
        let mut map1 = FxHashMap::default();
        map1.insert(
            0,
            StyleOverrideEntry {
                index: 0,
                code: Arc::from(".a{}"),
                source_map: None,
            },
        );
        map1.insert(
            1,
            StyleOverrideEntry {
                index: 1,
                code: Arc::from(".b{}"),
                source_map: None,
            },
        );

        let mut map2 = FxHashMap::default();
        map2.insert(
            1,
            StyleOverrideEntry {
                index: 1,
                code: Arc::from(".b{}"),
                source_map: None,
            },
        );
        map2.insert(
            0,
            StyleOverrideEntry {
                index: 0,
                code: Arc::from(".a{}"),
                source_map: None,
            },
        );

        assert_eq!(style_override_hash(&map1), style_override_hash(&map2));
    }
}
