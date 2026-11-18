/// Output geometry a product is compiled for. Framework-neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TargetKind {
    /// Client output that builds a retained tree and patches it.
    ClientTree,
    /// Client output that performs fine-grained operations.
    ClientFineGrained,
    /// Server string output.
    Server,
}

impl TargetKind {
    /// Stable spelling used in canonical digests.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            TargetKind::ClientTree => "client-tree",
            TargetKind::ClientFineGrained => "client-fine-grained",
            TargetKind::Server => "server",
        }
    }
}

/// Number of [`Capability`] variants.
pub const CAPABILITY_COUNT: usize = 22;

/// One unit of compiler work a request can require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Capability {
    ParseCarrier,
    ParseTemplate,
    ParseScript,
    ParseStyle,
    ParseExpressions,
    FactsBindings,
    FactsImports,
    FactsMacros,
    FactsTemplate,
    FactsReactivity,
    FactsDependencies,
    StructureTopology,
    StructureCompile,
    StyleScopePlan,
    StyleRewrite,
    PlanClientTree,
    PlanClientFineGrained,
    PlanServer,
    EmitRender,
    EmitModule,
    MapRuntime,
    PublishArtifacts,
}

impl Capability {
    pub const ALL: [Capability; CAPABILITY_COUNT] = [
        Capability::ParseCarrier,
        Capability::ParseTemplate,
        Capability::ParseScript,
        Capability::ParseStyle,
        Capability::ParseExpressions,
        Capability::FactsBindings,
        Capability::FactsImports,
        Capability::FactsMacros,
        Capability::FactsTemplate,
        Capability::FactsReactivity,
        Capability::FactsDependencies,
        Capability::StructureTopology,
        Capability::StructureCompile,
        Capability::StyleScopePlan,
        Capability::StyleRewrite,
        Capability::PlanClientTree,
        Capability::PlanClientFineGrained,
        Capability::PlanServer,
        Capability::EmitRender,
        Capability::EmitModule,
        Capability::MapRuntime,
        Capability::PublishArtifacts,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Capability::ParseCarrier => "parse-carrier",
            Capability::ParseTemplate => "parse-template",
            Capability::ParseScript => "parse-script",
            Capability::ParseStyle => "parse-style",
            Capability::ParseExpressions => "parse-expressions",
            Capability::FactsBindings => "facts-bindings",
            Capability::FactsImports => "facts-imports",
            Capability::FactsMacros => "facts-macros",
            Capability::FactsTemplate => "facts-template",
            Capability::FactsReactivity => "facts-reactivity",
            Capability::FactsDependencies => "facts-dependencies",
            Capability::StructureTopology => "structure-topology",
            Capability::StructureCompile => "structure-compile",
            Capability::StyleScopePlan => "style-scope-plan",
            Capability::StyleRewrite => "style-rewrite",
            Capability::PlanClientTree => "plan-client-tree",
            Capability::PlanClientFineGrained => "plan-client-fine-grained",
            Capability::PlanServer => "plan-server",
            Capability::EmitRender => "emit-render",
            Capability::EmitModule => "emit-module",
            Capability::MapRuntime => "map-runtime",
            Capability::PublishArtifacts => "publish-artifacts",
        }
    }
    /// Direct prerequisites: the fixed reason edges out of this capability.
    ///
    /// This is the superset. An edge that holds only when the source has a
    /// template, a script or styles, and the planning edge that depends on
    /// the requested target, are listed here and filtered per seed by
    /// [`super::close_demand`].
    pub const fn requires(self) -> &'static [Capability] {
        use Capability::*;
        match self {
            PublishArtifacts => &[EmitModule],
            EmitModule => &[
                EmitRender,
                FactsBindings,
                FactsMacros,
                ParseScript,
                StyleScopePlan,
            ],
            EmitRender => &[PlanClientTree, PlanClientFineGrained, PlanServer],
            PlanClientTree => &[
                StructureCompile,
                FactsTemplate,
                FactsBindings,
                FactsReactivity,
            ],
            PlanClientFineGrained => &[
                StructureCompile,
                FactsTemplate,
                FactsBindings,
                FactsReactivity,
                FactsDependencies,
            ],
            PlanServer => &[
                StructureCompile,
                FactsTemplate,
                FactsBindings,
                StyleScopePlan,
            ],
            StructureCompile => &[StructureTopology, FactsTemplate],
            StructureTopology => &[ParseTemplate],
            FactsTemplate => &[ParseExpressions, ParseTemplate],
            FactsReactivity => &[FactsBindings, FactsImports],
            FactsBindings | FactsImports | FactsMacros | FactsDependencies => &[ParseScript],
            StyleRewrite => &[StyleScopePlan],
            StyleScopePlan => &[ParseStyle],
            MapRuntime => &[],
            ParseTemplate | ParseScript | ParseStyle | ParseExpressions => &[ParseCarrier],
            ParseCarrier => &[],
        }
    }
    /// The fact family this capability asks the semantic authority for.
    pub(crate) const fn fact_family(self) -> Option<FactFamily> {
        match self {
            Capability::FactsBindings => Some(FactFamily::Bindings),
            Capability::FactsImports => Some(FactFamily::Imports),
            Capability::FactsMacros => Some(FactFamily::Macros),
            Capability::FactsTemplate => Some(FactFamily::Template),
            Capability::FactsReactivity => Some(FactFamily::Reactivity),
            Capability::FactsDependencies => Some(FactFamily::Dependencies),
            _ => None,
        }
    }
    /// The planning capability of one target.
    pub(crate) const fn planning(target: TargetKind) -> Capability {
        match target {
            TargetKind::ClientTree => Capability::PlanClientTree,
            TargetKind::ClientFineGrained => Capability::PlanClientFineGrained,
            TargetKind::Server => Capability::PlanServer,
        }
    }
    /// This capability's bit in a capability set.
    pub(crate) const fn bit(self) -> u32 {
        1 << self as u8
    }
}

/// A semantic fact family a framework authority can be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum FactFamily {
    Bindings,
    Imports,
    Macros,
    Template,
    Reactivity,
    Dependencies,
}

/// A set of fact families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FactFamilies(pub u8);

impl FactFamilies {
    pub const fn empty() -> Self {
        Self(0)
    }
    pub const fn with(self, family: FactFamily) -> Self {
        Self(self.0 | (1 << family as u8))
    }
    pub const fn contains(self, family: FactFamily) -> bool {
        self.0 & (1 << family as u8) != 0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_names_are_unique_and_follow_declaration_order() {
        for (index, capability) in Capability::ALL.into_iter().enumerate() {
            assert_eq!(capability as usize, index);
        }
        let mut names: Vec<&str> = Capability::ALL.iter().map(|c| c.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), CAPABILITY_COUNT);
    }

    #[test]
    fn every_parse_capability_rests_on_the_carrier() {
        for capability in [
            Capability::ParseTemplate,
            Capability::ParseScript,
            Capability::ParseStyle,
            Capability::ParseExpressions,
        ] {
            assert_eq!(capability.requires(), &[Capability::ParseCarrier]);
        }
        assert!(Capability::ParseCarrier.requires().is_empty());
        assert!(Capability::MapRuntime.requires().is_empty());
    }

    #[test]
    fn server_planning_never_requires_client_work() {
        let server = Capability::PlanServer.requires();
        assert!(!server.contains(&Capability::FactsDependencies));
        assert!(!server.contains(&Capability::FactsReactivity));
        assert!(!server.contains(&Capability::PlanClientTree));
        assert!(!server.contains(&Capability::PlanClientFineGrained));
    }

    #[test]
    fn exactly_the_facts_capabilities_name_a_family() {
        let families: Vec<FactFamily> = Capability::ALL
            .iter()
            .filter_map(|c| c.fact_family())
            .collect();
        assert_eq!(
            families,
            [
                FactFamily::Bindings,
                FactFamily::Imports,
                FactFamily::Macros,
                FactFamily::Template,
                FactFamily::Reactivity,
                FactFamily::Dependencies,
            ]
        );
    }
}
