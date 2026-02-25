//! Vue-specific lint rules (essential + recommended).

mod no_dupe_v_else_if;
mod no_duplicate_attributes;
mod no_template_key;
mod no_textarea_mustache;
mod no_unused_components;
mod no_unused_props;
mod no_use_v_if_with_v_for;
mod require_v_for_key;
mod valid_v_for;

pub use no_dupe_v_else_if::NoDupeVElseIf;
pub use no_duplicate_attributes::NoDuplicateAttributes;
pub use no_template_key::NoTemplateKey;
pub use no_textarea_mustache::NoTextareaMustache;
pub use no_unused_components::NoUnusedComponents;
pub use no_unused_props::NoUnusedProps;
pub use no_use_v_if_with_v_for::NoUseVIfWithVFor;
pub use require_v_for_key::RequireVForKey;
pub use valid_v_for::ValidVFor;
