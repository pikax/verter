//! Exact persisted-carrier compatibility cohort.
//!
//! This frozen type shape contains exactly eight carrier-owned version words;
//! exact equality compares the complete row. Downstream consumer compatibility
//! is a separate protocol row and cannot participate in carrier identity or
//! adoption: adding it here changes the structurally pinned size, while the
//! compiler and language identity owners have no Cargo dependency on the
//! downstream protocol crate. Carrier lanes and artifact IDs must remain built
//! solely from these carrier-owned versions when those types are introduced.

use verter_compiler::framework_common::vue_bridge::VUE_CARRIER_ARTIFACT_VERSION;
use verter_compiler::svelte::carrier::SVELTE_CARRIER_ARTIFACT_VERSION;
use verter_language::carrier_grammar::{
    CarrierGrammarFingerprintSchemaVersion, CARRIER_GRAMMAR_FINGERPRINT_SCHEMA_VERSION,
};
use verter_language::carrier_versions::{
    CarrierParserVersion, CarrierSourceMapSchemaVersion, CarrierSourceSpaceSchemaVersion,
    FrameworkParseArtifactSchemaVersion, CARRIER_SOURCE_MAP_SCHEMA_VERSION,
    CARRIER_SOURCE_SPACE_SCHEMA_VERSION, FRAMEWORK_PARSE_ARTIFACT_SCHEMA_VERSION,
};

macro_rules! nonzero_version {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(u32);

        impl $name {
            pub const fn new(value: u32) -> Option<Self> {
                if value == 0 {
                    None
                } else {
                    Some(Self(value))
                }
            }

            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

nonzero_version!(SessionCurrentParserVersion);
nonzero_version!(CarrierCacheSerializationVersion);

pub const SESSION_CURRENT_PARSER_VERSION: SessionCurrentParserVersion =
    match SessionCurrentParserVersion::new(crate::file_artifact_store::CURRENT_PARSER_VERSION) {
        Some(version) => version,
        None => panic!("session parser version must be nonzero"),
    };
pub const CARRIER_CACHE_SERIALIZATION_VERSION: CarrierCacheSerializationVersion =
    match CarrierCacheSerializationVersion::new(1) {
        Some(version) => version,
        None => panic!("carrier cache serialization version must be nonzero"),
    };

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PersistedCarrierArtifactCohort {
    vue_parser_version: CarrierParserVersion,
    svelte_parser_version: CarrierParserVersion,
    grammar_fingerprint_schema_version: CarrierGrammarFingerprintSchemaVersion,
    framework_artifact_schema_version: FrameworkParseArtifactSchemaVersion,
    carrier_source_space_schema_version: CarrierSourceSpaceSchemaVersion,
    carrier_source_map_schema_version: CarrierSourceMapSchemaVersion,
    session_current_parser_version: SessionCurrentParserVersion,
    carrier_cache_serialization_version: CarrierCacheSerializationVersion,
}

impl PersistedCarrierArtifactCohort {
    pub const fn vue_parser_version(self) -> CarrierParserVersion {
        self.vue_parser_version
    }
    pub const fn svelte_parser_version(self) -> CarrierParserVersion {
        self.svelte_parser_version
    }
    pub const fn grammar_fingerprint_schema_version(
        self,
    ) -> CarrierGrammarFingerprintSchemaVersion {
        self.grammar_fingerprint_schema_version
    }
    pub const fn framework_artifact_schema_version(self) -> FrameworkParseArtifactSchemaVersion {
        self.framework_artifact_schema_version
    }
    pub const fn carrier_source_space_schema_version(self) -> CarrierSourceSpaceSchemaVersion {
        self.carrier_source_space_schema_version
    }
    pub const fn carrier_source_map_schema_version(self) -> CarrierSourceMapSchemaVersion {
        self.carrier_source_map_schema_version
    }
    pub const fn session_current_parser_version(self) -> SessionCurrentParserVersion {
        self.session_current_parser_version
    }
    pub const fn carrier_cache_serialization_version(self) -> CarrierCacheSerializationVersion {
        self.carrier_cache_serialization_version
    }
}

/// The sole assembly point for the exact persisted-carrier compatibility row.
pub const fn current_persisted_carrier_artifact_cohort() -> PersistedCarrierArtifactCohort {
    PersistedCarrierArtifactCohort {
        vue_parser_version: VUE_CARRIER_ARTIFACT_VERSION,
        svelte_parser_version: SVELTE_CARRIER_ARTIFACT_VERSION,
        grammar_fingerprint_schema_version: CARRIER_GRAMMAR_FINGERPRINT_SCHEMA_VERSION,
        framework_artifact_schema_version: FRAMEWORK_PARSE_ARTIFACT_SCHEMA_VERSION,
        carrier_source_space_schema_version: CARRIER_SOURCE_SPACE_SCHEMA_VERSION,
        carrier_source_map_schema_version: CARRIER_SOURCE_MAP_SCHEMA_VERSION,
        session_current_parser_version: SESSION_CURRENT_PARSER_VERSION,
        carrier_cache_serialization_version: CARRIER_CACHE_SERIALIZATION_VERSION,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistedCarrierArtifactCohortMismatch;

pub fn validate_persisted_carrier_artifact_cohort(
    candidate: PersistedCarrierArtifactCohort,
) -> Result<(), PersistedCarrierArtifactCohortMismatch> {
    if candidate == current_persisted_carrier_artifact_cohort() {
        Ok(())
    } else {
        Err(PersistedCarrierArtifactCohortMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn current_cohort_reads_live_carrier_owned_versions() {
        let current = current_persisted_carrier_artifact_cohort();
        assert_eq!(current.vue_parser_version().get(), 6);
        assert_eq!(current.svelte_parser_version().get(), 2);
        assert_eq!(current.session_current_parser_version().get(), 5);
        assert_eq!(current.grammar_fingerprint_schema_version().get(), 1);
        assert_eq!(validate_persisted_carrier_artifact_cohort(current), Ok(()));
    }

    #[test]
    fn every_cohort_field_uses_exact_equality() {
        let current = current_persisted_carrier_artifact_cohort();
        let mutations = [
            PersistedCarrierArtifactCohort {
                vue_parser_version: CarrierParserVersion::new(99).unwrap(),
                ..current
            },
            PersistedCarrierArtifactCohort {
                svelte_parser_version: CarrierParserVersion::new(99).unwrap(),
                ..current
            },
            PersistedCarrierArtifactCohort {
                grammar_fingerprint_schema_version: CarrierGrammarFingerprintSchemaVersion::new(99)
                    .unwrap(),
                ..current
            },
            PersistedCarrierArtifactCohort {
                framework_artifact_schema_version: FrameworkParseArtifactSchemaVersion::new(99)
                    .unwrap(),
                ..current
            },
            PersistedCarrierArtifactCohort {
                carrier_source_space_schema_version: CarrierSourceSpaceSchemaVersion::new(99)
                    .unwrap(),
                ..current
            },
            PersistedCarrierArtifactCohort {
                carrier_source_map_schema_version: CarrierSourceMapSchemaVersion::new(99).unwrap(),
                ..current
            },
            PersistedCarrierArtifactCohort {
                session_current_parser_version: SessionCurrentParserVersion::new(99).unwrap(),
                ..current
            },
            PersistedCarrierArtifactCohort {
                carrier_cache_serialization_version: CarrierCacheSerializationVersion::new(99)
                    .unwrap(),
                ..current
            },
        ];
        for mutation in mutations {
            assert_eq!(
                validate_persisted_carrier_artifact_cohort(mutation),
                Err(PersistedCarrierArtifactCohortMismatch)
            );
        }
    }

    #[test]
    fn persisted_carrier_cohort_has_frozen_eight_word_shape() {
        assert_eq!(
            std::mem::size_of::<PersistedCarrierArtifactCohort>(),
            8 * std::mem::size_of::<u32>()
        );
        assert_eq!(
            std::mem::align_of::<PersistedCarrierArtifactCohort>(),
            std::mem::align_of::<u32>()
        );
    }

    #[test]
    fn carrier_identity_owner_crates_cannot_depend_on_consumer_protocol() {
        let output = Command::new(env!("CARGO"))
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("run cargo metadata");
        assert!(
            output.status.success(),
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("parse cargo metadata");
        let packages = metadata["packages"]
            .as_array()
            .expect("cargo metadata packages");
        for owner in ["verter_language", "verter_compiler"] {
            let package = packages
                .iter()
                .find(|package| package["name"] == owner)
                .unwrap_or_else(|| panic!("cargo metadata omitted {owner}"));
            let dependencies = package["dependencies"]
                .as_array()
                .expect("package dependencies");
            assert!(
                dependencies
                    .iter()
                    .all(|dependency| dependency["name"] != "verter_protocol"),
                "{owner} must not depend on downstream verter_protocol"
            );
        }
    }

    #[test]
    fn consumer_manifest_cache_pin_is_fresh_but_excluded_from_carrier_cohort() {
        let manifest =
            verter_protocol::consumer_compatibility_manifest::current_consumer_compatibility_manifest();
        assert_eq!(
            manifest.cache_cluster_schema_version.get(),
            crate::cache_schema::CACHE_CLUSTER_SCHEMA_VERSION
        );
    }
}
