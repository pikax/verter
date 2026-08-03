//! Semantic compatibility surface for the workspace-owned fact schema.

pub use verter_workspace::fact_registry::*;

impl From<crate::analysis::MacroKind> for MacroKind {
    fn from(value: crate::analysis::MacroKind) -> Self {
        use crate::analysis::MacroKind as TemplateMacroKind;
        match value {
            TemplateMacroKind::DefineProps => Self::DefineProps,
            TemplateMacroKind::DefineEmits => Self::DefineEmits,
            TemplateMacroKind::DefineModel => Self::DefineModel,
            TemplateMacroKind::DefineSlots => Self::DefineSlots,
            TemplateMacroKind::DefineExpose => Self::DefineExpose,
            TemplateMacroKind::DefineOptions => Self::DefineOptions,
            TemplateMacroKind::WithDefaults => Self::WithDefaults,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MacroKind;
    use crate::analysis::MacroKind as TemplateMacroKind;

    #[test]
    fn macro_kind_round_trips_with_template_kind() {
        let pairs = [
            (TemplateMacroKind::DefineProps, MacroKind::DefineProps),
            (TemplateMacroKind::DefineEmits, MacroKind::DefineEmits),
            (TemplateMacroKind::DefineModel, MacroKind::DefineModel),
            (TemplateMacroKind::DefineSlots, MacroKind::DefineSlots),
            (TemplateMacroKind::DefineExpose, MacroKind::DefineExpose),
            (TemplateMacroKind::DefineOptions, MacroKind::DefineOptions),
            (TemplateMacroKind::WithDefaults, MacroKind::WithDefaults),
        ];
        for (template, fact) in pairs {
            assert_eq!(MacroKind::from(template), fact);
        }
    }
}
