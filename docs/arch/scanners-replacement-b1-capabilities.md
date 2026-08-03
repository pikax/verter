# Scanner replacement B1 capabilities

Block B1 introduces the neutral carrier syntax schema without changing the
live parse-artifact storage or routing path.

`verter_language::parse_artifact::carrier_inventory` owns the immutable
`CarrierBlockInventory`: validated source spaces, a normalized-name table, one
source-ordered block collection, and an arena that exclusively owns markup
geometry. Raw names and values remain source slices; attributes preserve source
order and duplicates; entity decoding is selected by a closed lazy recipe.
`CarrierStructureHash` is a value-side digest of inventory semantics and is
deliberately insensitive to byte-offset motion. It must not be used as a query,
lane, revision, or artifact identity.

`verter_compiler::framework_common::registered_carrier_projection` contains the
single internal projector. It accepts only a `CarrierCompiler` and a sealed
`AcceptedRegisteredCarrierSource`. The returned bundle is private,
non-serializable, and has read-only inventory/hash accessors. B1 has no
production mint, acceptance, or projector caller; B2 owns the first production
publication path.

`verter_session::carrier_artifact_cohort` owns the exact eight-field persisted
carrier compatibility cohort and its sole assembler. Every field is nominal
and adoption compares the complete row by exact equality. The current carrier
parser stamps are Vue 6 and Svelte 2; the session current-parser stamp remains
5. `verter_protocol::consumer_compatibility_manifest` separately generates the
closed eleven-field downstream compatibility row. Its cache-cluster value is 8
and none of its fields may enter carrier identity, lane selection, cohort
assembly, or adoption.
