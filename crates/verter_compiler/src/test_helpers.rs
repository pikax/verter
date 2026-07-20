use smallvec::SmallVec;
use std::sync::Arc;
use verter_macro_dto::{
    AuthoredMemberOrdinal, MacroAnchor, MacroFailure, MacroMemberReason, MacroRuntimeBundle,
    MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape, ModelRuntimeShape,
    OrderedRuntimeConstructors, PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor,
    RuntimeEmit, RuntimeProp, RuntimePropType, SynthesizedRowKind,
};

use crate::parser::types::{RootNodeTemplate, RootNodeTemplateContent};
use crate::types::NodeTag;

pub fn make_root() -> RootNodeTemplate {
    RootNodeTemplate {
        tag_open: NodeTag {
            start: 0,
            end: 0,
            name_end: 0,
        },
        tag_close: None,
        lang: None,
        attributes: Vec::new(),
        content: Some(RootNodeTemplateContent {
            start: 0,
            end: 0,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    }
}

pub fn make_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
    NodeTag {
        start,
        end,
        name_end,
    }
}

#[derive(Debug, Clone)]
pub struct RuntimePropSpec {
    name: String,
    optional: bool,
    constructors: Vec<RuntimeConstructor>,
    skip_check: bool,
    anchor: RuntimePropAnchor,
    degradation: Option<MacroFailure<MacroMemberReason>>,
}

#[derive(Debug, Clone, Copy)]
enum RuntimePropAnchor {
    AuthoredMember,
    MacroArgument,
}

pub fn runtime_prop(
    name: impl Into<String>,
    optional: bool,
    constructors: impl IntoIterator<Item = RuntimeConstructor>,
) -> RuntimePropSpec {
    RuntimePropSpec {
        name: name.into(),
        optional,
        constructors: constructors.into_iter().collect(),
        skip_check: false,
        anchor: RuntimePropAnchor::AuthoredMember,
        degradation: None,
    }
}

pub fn runtime_prop_at_macro_argument(
    name: impl Into<String>,
    optional: bool,
    constructors: impl IntoIterator<Item = RuntimeConstructor>,
) -> RuntimePropSpec {
    RuntimePropSpec {
        name: name.into(),
        optional,
        constructors: constructors.into_iter().collect(),
        skip_check: false,
        anchor: RuntimePropAnchor::MacroArgument,
        degradation: None,
    }
}

pub fn runtime_degraded_prop_at_macro_argument(
    name: impl Into<String>,
    optional: bool,
    reason: MacroMemberReason,
    diagnostic: Option<String>,
) -> RuntimePropSpec {
    RuntimePropSpec {
        name: name.into(),
        optional,
        constructors: Vec::new(),
        skip_check: false,
        anchor: RuntimePropAnchor::MacroArgument,
        degradation: Some(MacroFailure::new(reason, diagnostic)),
    }
}

pub fn runtime_props_entry(
    syntax_index: u32,
    macro_index: u32,
    defaults: PropsDefaultsAssociation,
    props: impl IntoIterator<Item = RuntimePropSpec>,
) -> MacroRuntimeEntry {
    let authored_macro_index = match defaults {
        PropsDefaultsAssociation::None => macro_index,
        PropsDefaultsAssociation::WithDefaults {
            payload_macro_index,
            ..
        } => payload_macro_index,
    };
    MacroRuntimeEntry {
        syntax_index,
        macro_index,
        outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
            defaults,
            props: props
                .into_iter()
                .enumerate()
                .map(|(ordinal, prop)| {
                    let anchor = match prop.anchor {
                        RuntimePropAnchor::AuthoredMember => MacroAnchor::Authored {
                            macro_index: authored_macro_index,
                            member_ordinal: AuthoredMemberOrdinal::new(ordinal as u32),
                        },
                        RuntimePropAnchor::MacroArgument => MacroAnchor::MacroArgument {
                            macro_index: authored_macro_index,
                        },
                    };
                    RuntimeProp {
                        name: prop.name,
                        optional: prop.optional,
                        type_shape: match prop.degradation {
                            Some(failure) => RuntimePropType::Degraded(failure),
                            None => RuntimePropType::Resolved {
                                constructors: OrderedRuntimeConstructors::from_ordered(
                                    prop.constructors,
                                ),
                                skip_check: prop.skip_check,
                            },
                        },
                        anchor,
                    }
                })
                .collect(),
        })),
    }
}

pub fn runtime_emits_entry(
    syntax_index: u32,
    macro_index: u32,
    names: impl IntoIterator<Item = impl Into<String>>,
) -> MacroRuntimeEntry {
    MacroRuntimeEntry {
        syntax_index,
        macro_index,
        outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(
            names
                .into_iter()
                .enumerate()
                .map(|(ordinal, name)| RuntimeEmit {
                    name: name.into(),
                    anchor: MacroAnchor::Authored {
                        macro_index,
                        member_ordinal: AuthoredMemberOrdinal::new(ordinal as u32),
                    },
                })
                .collect(),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn runtime_model_entry(
    syntax_index: u32,
    macro_index: u32,
    prop_name: impl Into<String>,
    modifiers_name: impl Into<String>,
    update_event_name: impl Into<String>,
    optional: bool,
    constructors: impl IntoIterator<Item = RuntimeConstructor>,
) -> MacroRuntimeEntry {
    let prop = RuntimeProp {
        name: prop_name.into(),
        optional,
        type_shape: RuntimePropType::Resolved {
            constructors: OrderedRuntimeConstructors::from_ordered(constructors),
            skip_check: false,
        },
        anchor: MacroAnchor::Synthesized {
            macro_index,
            row: SynthesizedRowKind::ModelProp,
        },
    };
    let modifiers_prop = RuntimeProp {
        name: modifiers_name.into(),
        optional: true,
        type_shape: RuntimePropType::Resolved {
            constructors: OrderedRuntimeConstructors::default(),
            skip_check: false,
        },
        anchor: MacroAnchor::Synthesized {
            macro_index,
            row: SynthesizedRowKind::ModelModifiersProp,
        },
    };
    let update_event = RuntimeEmit {
        name: update_event_name.into(),
        anchor: MacroAnchor::Synthesized {
            macro_index,
            row: SynthesizedRowKind::ModelUpdateEvent,
        },
    };

    MacroRuntimeEntry {
        syntax_index,
        macro_index,
        outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Model(ModelRuntimeShape {
            prop,
            update_event,
            modifiers_prop,
        })),
    }
}

pub fn runtime_bundle(
    entries: impl IntoIterator<Item = MacroRuntimeEntry>,
) -> Arc<MacroRuntimeBundle> {
    Arc::new(MacroRuntimeBundle {
        entries: entries.into_iter().collect(),
    })
}
