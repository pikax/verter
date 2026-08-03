//! Generated compatibility row for downstream carrier projections.
//!
//! None of these values is carrier identity or persisted-carrier adoption
//! input. Each nominal field remains owned and bumped by its downstream
//! surface; this module only assembles the closed generated row.

use serde::{Deserialize, Serialize};

macro_rules! nonzero_version {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
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

nonzero_version!(BlockContentArtifactSchemaVersion);
nonzero_version!(QualifiedSourceMapSchemaVersion);
nonzero_version!(CacheClusterSchemaVersion);
nonzero_version!(ComponentMetaSchemaVersion);
nonzero_version!(StructureProtocolVersion);
nonzero_version!(ProviderProtocolVersion);
nonzero_version!(NapiSchemaVersion);
nonzero_version!(WasmSchemaVersion);
nonzero_version!(NativeApiVersion);
nonzero_version!(UnpluginApiVersion);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidPublicHashV1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicHashV1(String);

impl PublicHashV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidPublicHashV1> {
        let value = value.into();
        let valid = value.len() == 71
            && value.starts_with("sha256:")
            && value.as_bytes()[7..]
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte));
        if valid {
            Ok(Self(value))
        } else {
            Err(InvalidPublicHashV1)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

const BLOCK_CONTENT_ARTIFACT_SCHEMA_VERSION: BlockContentArtifactSchemaVersion =
    BlockContentArtifactSchemaVersion(1);
const QUALIFIED_SOURCE_MAP_SCHEMA_VERSION: QualifiedSourceMapSchemaVersion =
    QualifiedSourceMapSchemaVersion(1);
const CACHE_CLUSTER_SCHEMA_VERSION: CacheClusterSchemaVersion = CacheClusterSchemaVersion(8);
const STRUCTURE_PROTOCOL_VERSION: StructureProtocolVersion = StructureProtocolVersion(1);
const PROVIDER_PROTOCOL_VERSION: ProviderProtocolVersion = ProviderProtocolVersion(12);
const NAPI_SCHEMA_VERSION: NapiSchemaVersion = NapiSchemaVersion(1);
const WASM_SCHEMA_VERSION: WasmSchemaVersion = WasmSchemaVersion(1);
const NATIVE_API_VERSION: NativeApiVersion = NativeApiVersion(1);
const UNPLUGIN_API_VERSION: UnpluginApiVersion = UnpluginApiVersion(1);
const GENERATED_BINDING_MANIFEST_HASH: &str =
    "sha256:d0884f3070be543a1bb7b16364ac4facfd229ddacb61f7d116a0ea2de575edce";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerCompatibilityManifest {
    pub block_content_artifact_schema_version: BlockContentArtifactSchemaVersion,
    pub qualified_source_map_schema_version: QualifiedSourceMapSchemaVersion,
    pub cache_cluster_schema_version: CacheClusterSchemaVersion,
    pub component_meta_schema_version: ComponentMetaSchemaVersion,
    pub structure_protocol_version: StructureProtocolVersion,
    pub provider_protocol_version: ProviderProtocolVersion,
    pub napi_schema_version: NapiSchemaVersion,
    pub wasm_schema_version: WasmSchemaVersion,
    pub native_api_version: NativeApiVersion,
    pub unplugin_api_version: UnpluginApiVersion,
    pub generated_binding_manifest_hash: PublicHashV1,
}

/// Generates the one closed downstream compatibility row from live pins.
pub fn current_consumer_compatibility_manifest() -> ConsumerCompatibilityManifest {
    ConsumerCompatibilityManifest {
        block_content_artifact_schema_version: BLOCK_CONTENT_ARTIFACT_SCHEMA_VERSION,
        qualified_source_map_schema_version: QUALIFIED_SOURCE_MAP_SCHEMA_VERSION,
        cache_cluster_schema_version: CACHE_CLUSTER_SCHEMA_VERSION,
        component_meta_schema_version: ComponentMetaSchemaVersion::new(
            crate::component_meta::COMPONENT_META_SCHEMA_VERSION,
        )
        .expect("component-meta schema version must be nonzero"),
        structure_protocol_version: STRUCTURE_PROTOCOL_VERSION,
        provider_protocol_version: PROVIDER_PROTOCOL_VERSION,
        napi_schema_version: NAPI_SCHEMA_VERSION,
        wasm_schema_version: WASM_SCHEMA_VERSION,
        native_api_version: NATIVE_API_VERSION,
        unplugin_api_version: UNPLUGIN_API_VERSION,
        generated_binding_manifest_hash: PublicHashV1::new(GENERATED_BINDING_MANIFEST_HASH)
            .expect("generated binding manifest hash is frozen-valid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_consumer_manifest_is_fresh() {
        let generated = serde_json::to_string_pretty(&current_consumer_compatibility_manifest())
            .expect("serialize generated manifest");
        assert_eq!(
            generated.trim(),
            include_str!("consumer_compatibility_manifest.json").trim()
        );
    }

    #[test]
    fn public_hash_grammar_and_version_domains_are_closed() {
        assert!(PublicHashV1::new(GENERATED_BINDING_MANIFEST_HASH).is_ok());
        assert!(PublicHashV1::new("sha256:ABC").is_err());
        assert!(CacheClusterSchemaVersion::new(0).is_none());
        let manifest = current_consumer_compatibility_manifest();
        assert_eq!(manifest.cache_cluster_schema_version.get(), 8);
        assert_eq!(manifest.component_meta_schema_version.get(), 7);
        assert_eq!(manifest.provider_protocol_version.get(), 12);
    }
}
