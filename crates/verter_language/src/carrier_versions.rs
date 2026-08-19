//! Nominal versions owned by the neutral registered-carrier artifact.

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

nonzero_version!(CarrierSourceSpaceSchemaVersion);
nonzero_version!(CarrierSourceMapSchemaVersion);

/// First frozen registered/derived source-space serialization schema.
pub const CARRIER_SOURCE_SPACE_SCHEMA_VERSION: CarrierSourceSpaceSchemaVersion =
    CarrierSourceSpaceSchemaVersion(1);
/// First frozen qualified carrier-map serialization schema.
pub const CARRIER_SOURCE_MAP_SCHEMA_VERSION: CarrierSourceMapSchemaVersion =
    CarrierSourceMapSchemaVersion(1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carrier_versions_are_nonzero_nominal_values() {
        assert_eq!(CARRIER_SOURCE_SPACE_SCHEMA_VERSION.get(), 1);
        assert_eq!(CARRIER_SOURCE_MAP_SCHEMA_VERSION.get(), 1);
    }
}
