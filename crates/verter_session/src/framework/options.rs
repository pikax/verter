#![deny(missing_docs)]
//! Typed framework construction options.
//!
//! Framework configuration travels through THIS typed representation —
//! never through loosely typed host state, generic accessors, or a global
//! lookup. A [`FrameworkOptions`] value is fixed at construction: the
//! native host, the language server, the NAPI binding, and the WASM
//! binding each parse their carrier-specific configuration into this one
//! type through the ONE validator ([`FrameworkOptions::admitting_names`]),
//! so every carrier accepts the same names, applies the same defaults,
//! and rejects the same invalid combinations with the same diagnostics.
//!
//! The options are an ADMISSION SET over the composed framework
//! capability catalog, not a second framework enumeration: `All` (the
//! default) defers to whatever the catalog composes, and `Only` names a
//! subset that the catalog must contain. The catalog stays the sole
//! authority for which frameworks exist.
//!
//! Constructor-time per the host's configuration discipline: a request
//! cannot mutate a constructed host's framework admission. The LSP
//! therefore takes its admission from the `--frameworks` process flag —
//! the only channel that exists before its host is constructed — and
//! reads no `initializationOptions` key for it.

use std::collections::BTreeSet;

use verter_language::FrameworkAdapterId;

/// The admitted framework set of one construction.
///
/// Constructed only through [`Self::default`] (admit every composed
/// vertical) or the validating [`Self::admitting`] /
/// [`Self::admitting_names`]; the inner set is private so a
/// partially-validated admission cannot exist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameworkOptions {
    /// `None` admits every vertical the composed catalog names (the
    /// historical behavior); `Some(set)` admits exactly the named,
    /// catalog-validated subset.
    admitted: Option<BTreeSet<FrameworkAdapterId>>,
}

/// Why a framework admission was rejected.
///
/// Every variant is actionable: it names the offending input and the
/// supported set, so a carrier can surface the message verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameworkOptionsError {
    /// A requested framework name is not a composed vertical.
    UnknownFramework {
        /// The name the carrier received.
        requested: String,
        /// The names the composed capability catalog supports, sorted.
        supported: Vec<String>,
    },
    /// The same framework was named more than once.
    DuplicateFramework {
        /// The duplicated name.
        requested: String,
    },
    /// No framework was named at all.
    EmptyAdmission {
        /// The names the composed capability catalog supports, sorted.
        supported: Vec<String>,
    },
}

impl std::fmt::Display for FrameworkOptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownFramework {
                requested,
                supported,
            } => write!(
                f,
                "framework admission requested '{requested}', which the composed capability \
                 catalog does not name; the supported frameworks are: {}",
                supported.join(", ")
            ),
            Self::DuplicateFramework { requested } => write!(
                f,
                "framework admission named '{requested}' more than once — admission is a set; \
                 name each framework once"
            ),
            Self::EmptyAdmission { supported } => write!(
                f,
                "framework admission cannot be empty — admit at least one of: {}",
                supported.join(", ")
            ),
        }
    }
}

impl std::error::Error for FrameworkOptionsError {}

impl FrameworkOptions {
    /// Validate an explicit admission from framework names (the shared
    /// carrier entry point).
    ///
    /// The names are exact (case-sensitive) adapter ids as the composed
    /// capability catalog spells them. An unknown name, a duplicate, or
    /// an empty list is rejected with a diagnostic that names the
    /// supported set.
    ///
    /// # Errors
    ///
    /// [`FrameworkOptionsError`] naming the rejected input and the
    /// supported names.
    pub fn admitting_names<I, S>(names: I) -> Result<Self, FrameworkOptionsError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut admitted = BTreeSet::new();
        let supported = Self::supported_names();
        for name in names {
            let name = name.as_ref();
            if !supported.iter().any(|supported| supported == name) {
                return Err(FrameworkOptionsError::UnknownFramework {
                    requested: name.to_string(),
                    supported,
                });
            }
            let id = FrameworkAdapterId::new(name);
            if !admitted.insert(id) {
                return Err(FrameworkOptionsError::DuplicateFramework {
                    requested: name.to_string(),
                });
            }
        }
        if admitted.is_empty() {
            return Err(FrameworkOptionsError::EmptyAdmission {
                supported: Self::supported_names(),
            });
        }
        Ok(Self {
            admitted: Some(admitted),
        })
    }

    /// Validate an explicit admission from adapter ids.
    ///
    /// # Errors
    ///
    /// [`FrameworkOptionsError`] naming the rejected id and the
    /// supported set.
    pub fn admitting<I>(ids: I) -> Result<Self, FrameworkOptionsError>
    where
        I: IntoIterator<Item = FrameworkAdapterId>,
    {
        let names = ids.into_iter().map(|id| id.to_string()).collect::<Vec<_>>();
        Self::admitting_names(names)
    }

    /// Whether `adapter_id` is admitted by these options.
    #[must_use]
    pub fn admits(&self, adapter_id: &FrameworkAdapterId) -> bool {
        match &self.admitted {
            None => true,
            Some(set) => set.contains(adapter_id),
        }
    }

    /// The admitted adapter ids in deterministic (sorted) order, or
    /// `None` when these options admit every composed vertical.
    #[must_use]
    pub fn admitted_ids(&self) -> Option<impl Iterator<Item = &FrameworkAdapterId>> {
        self.admitted.as_ref().map(|set| set.iter())
    }

    /// The sorted names the composed capability catalog supports — the
    /// diagnostic half of every rejection above.
    fn supported_names() -> Vec<String> {
        let mut names: Vec<String> = super::registry::FrameworkCapabilityCatalog::built_in()
            .expect("every built-in frontend capability row publishes a carrier grammar fact")
            .adapter_ids()
            .map(ToString::to_string)
            .collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default admits every composed vertical — the historical
    /// built-in behavior, now spelled as a typed option.
    #[test]
    fn default_admits_every_composed_vertical() {
        let options = FrameworkOptions::default();
        let catalog = super::super::registry::FrameworkCapabilityCatalog::built_in()
            .expect("composed catalog");
        for row in catalog.rows() {
            assert!(
                options.admits(row.adapter_id()),
                "the default admission must admit composed vertical '{}'",
                row.adapter_id()
            );
        }
    }

    /// A valid explicit admission names a subset; order of the names is
    /// irrelevant (the admission is a set), and the sorted id iteration
    /// is deterministic.
    #[test]
    fn admission_is_an_order_insensitive_set_with_sorted_ids() {
        let vue_first = FrameworkOptions::admitting_names(["vue", "svelte"])
            .expect("both names are composed verticals");
        let svelte_first = FrameworkOptions::admitting_names(["svelte", "vue"])
            .expect("both names are composed verticals");
        assert_eq!(vue_first, svelte_first, "admission must be a set");
        let ids: Vec<String> = vue_first
            .admitted_ids()
            .expect("an explicit admission carries its set")
            .map(ToString::to_string)
            .collect();
        assert_eq!(ids, vec!["svelte".to_string(), "vue".to_string()]);
        assert!(vue_first.admits(&FrameworkAdapterId::vue()));
        assert!(vue_first.admits(&FrameworkAdapterId::svelte()));
    }

    /// An unknown framework name is rejected with a diagnostic that
    /// names the request AND the supported set — the message a carrier
    /// surfaces verbatim.
    #[test]
    fn unknown_name_is_rejected_naming_the_supported_set() {
        let error = FrameworkOptions::admitting_names(["react"])
            .expect_err("react is not a composed vertical");
        assert_eq!(
            error,
            FrameworkOptionsError::UnknownFramework {
                requested: "react".to_string(),
                supported: vec!["svelte".to_string(), "vue".to_string()],
            }
        );
        let message = error.to_string();
        assert!(message.contains("'react'"), "names the request: {message}");
        assert!(
            message.contains("svelte, vue"),
            "names the supported set: {message}"
        );
    }

    /// A duplicate name is an aliasing admission, rejected as such.
    #[test]
    fn duplicate_name_is_rejected() {
        let error = FrameworkOptions::admitting_names(["vue", "vue"])
            .expect_err("a duplicate name cannot admit");
        assert_eq!(
            error,
            FrameworkOptionsError::DuplicateFramework {
                requested: "vue".to_string()
            }
        );
    }

    /// An empty admission is rejected: a construction that admits no
    /// framework vertical has no framework configuration to carry, and a
    /// silent empty set would strip every carrier row without a
    /// decision.
    #[test]
    fn empty_admission_is_rejected() {
        let error = FrameworkOptions::admitting_names(Vec::<String>::new())
            .expect_err("an empty admission cannot compose");
        assert_eq!(
            error,
            FrameworkOptionsError::EmptyAdmission {
                supported: vec!["svelte".to_string(), "vue".to_string()],
            }
        );
        assert!(
            error.to_string().contains("at least one of: svelte, vue"),
            "the diagnostic is actionable: {}",
            error
        );
    }

    /// The adapter-id entry validates identically to the name entry —
    /// one admission semantics, two spellings.
    #[test]
    fn admitting_ids_matches_admitting_names() {
        let by_id = FrameworkOptions::admitting([FrameworkAdapterId::vue()])
            .expect("the Vue vertical is composed");
        let by_name =
            FrameworkOptions::admitting_names(["vue"]).expect("the Vue vertical is composed");
        assert_eq!(by_id, by_name);
        let error = FrameworkOptions::admitting([FrameworkAdapterId::new("solid")])
            .expect_err("solid is not a composed vertical");
        assert!(matches!(
            error,
            FrameworkOptionsError::UnknownFramework { .. }
        ));
    }
}
