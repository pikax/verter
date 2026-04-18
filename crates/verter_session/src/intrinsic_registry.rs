//! TypeScript `intrinsic` declaration registry (Phase 2.1).
//!
//! The plan requires that named utility types like `Pick`, `Omit`, `Partial`,
//! and userland aliases resolve through the normal declaration path —
//! there is **no reserved-name handling**. The resolver only takes the
//! internal intrinsic execution path when a declaration's resolved body is
//! literally `= intrinsic;`.
//!
//! [`IntrinsicRegistry`] holds the mapping from resolved-declaration
//! identity to the in-tree implementation handler. Discovery scanners
//! (active TypeScript SDK, `typescript@latest` audit) walk the
//! `type X<...> = intrinsic;` declarations in `lib*.d.ts` and compare
//! them against this registry. Missing implementations fail the audit.
//!
//! ## Contract
//!
//! - Keys are the **resolved-declaration name** (e.g. `"Uppercase"`,
//!   `"NoInfer"`). Userland aliases that happen to share the same name
//!   never reach the intrinsic path — lookup happens only *after* normal
//!   declaration resolution yields `= intrinsic`.
//! - Entries are registered at runtime (plan) rather than via a macro so
//!   implementations can be added in lockstep with resolver changes.
//! - Missing intrinsics at user runtime return
//!   [`IntrinsicLookup::Unsupported`] so the caller can emit a structured
//!   diagnostic / opaque node rather than crashing. The repo audit uses
//!   [`IntrinsicRegistry::audit_has_entry`] to enforce hard-fail semantics
//!   in maintenance CI.

use std::sync::Arc;

use parking_lot::RwLock;
use rustc_hash::FxHashMap;

/// Well-known intrinsic implementation identity. The resolver chooses the
/// matching arm after the declaration resolves to `= intrinsic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicImpl {
    /// String-case intrinsics (`Uppercase`, `Lowercase`, `Capitalize`,
    /// `Uncapitalize`).
    UppercaseString,
    LowercaseString,
    CapitalizeString,
    UncapitalizeString,
    /// `NoInfer<T>` — suppresses inference through the containing call.
    NoInfer,
    /// `BuiltinIteratorReturn` — placeholder; newer SDKs add new
    /// intrinsics without the resolver noticing, so we document its
    /// existence here even when the implementation is a thin passthrough.
    BuiltinIteratorReturn,
}

/// Lookup outcome. The resolver maps `Found` to the implementation path
/// and `Unsupported` to an opaque diagnostic node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntrinsicLookup {
    /// Registered intrinsic with an implementation handle.
    Found(IntrinsicImpl),
    /// The SDK declared a `= intrinsic` alias the registry has not learned
    /// about yet. At end-user runtime this must downgrade to a structured
    /// diagnostic per the plan; in repo / CI mode it is a hard failure.
    Unsupported { name: Arc<str> },
}

/// Host-owned registry. Internally a small `RwLock<FxHashMap>` — the
/// registry is write-once at construction and read-mostly afterwards.
#[derive(Debug, Default)]
pub struct IntrinsicRegistry {
    entries: RwLock<FxHashMap<Arc<str>, IntrinsicImpl>>,
}

impl IntrinsicRegistry {
    /// Build an empty registry. Use [`Self::with_defaults`] for the
    /// canonical in-tree set of intrinsics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Canonical in-tree registry. Populated with the intrinsics Verter
    /// currently supports — the audit scanner compares this against the
    /// active TypeScript SDK and fails on mismatches.
    pub fn with_defaults() -> Self {
        let reg = Self::new();
        reg.register("Uppercase", IntrinsicImpl::UppercaseString);
        reg.register("Lowercase", IntrinsicImpl::LowercaseString);
        reg.register("Capitalize", IntrinsicImpl::CapitalizeString);
        reg.register("Uncapitalize", IntrinsicImpl::UncapitalizeString);
        reg.register("NoInfer", IntrinsicImpl::NoInfer);
        reg.register(
            "BuiltinIteratorReturn",
            IntrinsicImpl::BuiltinIteratorReturn,
        );
        reg
    }

    pub fn register(&self, name: impl Into<Arc<str>>, impl_id: IntrinsicImpl) {
        self.entries.write().insert(name.into(), impl_id);
    }

    /// Lookup by resolved-declaration name. Only called after a
    /// declaration's body has resolved to `= intrinsic`; userland
    /// shadowing is handled upstream by normal declaration resolution and
    /// never reaches this table.
    #[must_use]
    pub fn lookup(&self, name: &str) -> IntrinsicLookup {
        let entries = self.entries.read();
        if let Some(impl_id) = entries.get(name).copied() {
            IntrinsicLookup::Found(impl_id)
        } else {
            IntrinsicLookup::Unsupported {
                name: Arc::from(name),
            }
        }
    }

    /// Audit helper: returns `true` iff the name has a registered
    /// implementation. Used by the repo / CI intrinsic audit scanner.
    pub fn audit_has_entry(&self, name: &str) -> bool {
        self.entries.read().contains_key(name)
    }

    /// Iterate registered names. Primarily used by the audit scanner to
    /// report which intrinsics are supported.
    pub fn registered_names(&self) -> Vec<Arc<str>> {
        self.entries.read().keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }
}

/// Extract every `type X<...> = intrinsic;` declaration name from a
/// `lib*.d.ts` source. Used by the repo audit (active-SDK + `typescript@latest`)
/// to enumerate intrinsics the resolver must implement.
///
/// This is an intentionally simple textual scan — TypeScript's `lib*.d.ts`
/// files declare intrinsics on one physical line with no conditional
/// gating, so lexical matching is sufficient and keeps the audit
/// dependency-free.
pub fn extract_intrinsics_from_lib_source(source: &str) -> Vec<Arc<str>> {
    let mut names = Vec::new();
    for raw in source.lines() {
        let line = raw.trim_start();
        // Match leading declaration variants:
        //   `type X<...> = intrinsic;`
        //   `declare type X<...> = intrinsic;`
        let rest = if let Some(after_declare) = line.strip_prefix("declare type") {
            after_declare
        } else if let Some(after_type) = line.strip_prefix("type") {
            after_type
        } else {
            continue;
        };
        // Must separate the `type` keyword from the name with whitespace.
        let Some(first) = rest.chars().next() else {
            continue;
        };
        if !first.is_whitespace() {
            continue;
        }
        let trimmed = rest.trim_start();
        // Must end with `= intrinsic;`.
        if !trimmed.contains("= intrinsic") {
            continue;
        }
        // Name is the prefix up to `<` or whitespace or `=`.
        let name_end = trimmed
            .find(|c: char| c == '<' || c.is_whitespace() || c == '=')
            .unwrap_or(trimmed.len());
        let name = &trimmed[..name_end];
        if !name.is_empty() {
            names.push(Arc::from(name));
        }
    }
    names
}

/// Compare a scanned intrinsic set against the registry; return the names
/// that the SDK declares but the registry does not implement. Empty result
/// ⇒ audit passes.
pub fn audit_unsupported(registry: &IntrinsicRegistry, scanned: &[Arc<str>]) -> Vec<Arc<str>> {
    scanned
        .iter()
        .filter(|name| !registry.audit_has_entry(name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_contains_known_intrinsics() {
        let reg = IntrinsicRegistry::with_defaults();
        assert!(reg.audit_has_entry("Uppercase"));
        assert!(reg.audit_has_entry("Lowercase"));
        assert!(reg.audit_has_entry("Capitalize"));
        assert!(reg.audit_has_entry("Uncapitalize"));
        assert!(reg.audit_has_entry("NoInfer"));
        assert!(reg.audit_has_entry("BuiltinIteratorReturn"));
    }

    #[test]
    fn lookup_returns_unsupported_for_unknown() {
        let reg = IntrinsicRegistry::with_defaults();
        match reg.lookup("TotallyNewIntrinsicInNextSdk") {
            IntrinsicLookup::Unsupported { name } => {
                assert_eq!(name.as_ref(), "TotallyNewIntrinsicInNextSdk");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn lookup_returns_found_for_registered() {
        let reg = IntrinsicRegistry::with_defaults();
        match reg.lookup("Uppercase") {
            IntrinsicLookup::Found(id) => {
                assert_eq!(id, IntrinsicImpl::UppercaseString);
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn scanner_extracts_type_x_equals_intrinsic() {
        let src = r#"
// Skip declarations without intrinsic body.
type Foo = { x: number };
type Uppercase<S extends string> = intrinsic;
  type Lowercase<S extends string> = intrinsic;
declare type Capitalize<S extends string> = intrinsic;
type NotIntrinsic<T> = T extends string ? T : never;
// Whitespace + weird spacing.
type   NoInfer   <   T  >   = intrinsic   ;
"#;
        let names = extract_intrinsics_from_lib_source(src);
        let names: Vec<_> = names.iter().map(|s| s.as_ref().to_string()).collect();
        assert!(names.contains(&"Uppercase".to_string()));
        assert!(names.contains(&"Lowercase".to_string()));
        assert!(names.contains(&"Capitalize".to_string()));
        assert!(names.contains(&"NoInfer".to_string()));
        assert!(!names.contains(&"Foo".to_string()));
        assert!(!names.contains(&"NotIntrinsic".to_string()));
    }

    #[test]
    fn audit_unsupported_reports_missing_registry_entries() {
        let reg = IntrinsicRegistry::with_defaults();
        let scanned = vec![Arc::from("Uppercase"), Arc::from("NewIntrinsicInLatestSdk")];
        let missing = audit_unsupported(&reg, &scanned);
        let missing: Vec<_> = missing.iter().map(|s| s.as_ref().to_string()).collect();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0], "NewIntrinsicInLatestSdk");
    }

    #[test]
    fn registry_round_trip_register_and_lookup() {
        let reg = IntrinsicRegistry::new();
        reg.register("Custom", IntrinsicImpl::NoInfer);
        match reg.lookup("Custom") {
            IntrinsicLookup::Found(id) => assert_eq!(id, IntrinsicImpl::NoInfer),
            other => panic!("expected Found, got {other:?}"),
        }
    }
}
