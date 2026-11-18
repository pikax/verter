use verter_css_syntax::StyleStage;

use verter_identity::encoding::CanonicalDigest;

use super::stage_options::{self, ClassifiedOptions, OptionStage};
use super::{close_demand, DemandSet, FactFamilies};
use crate::compile_request::identity::{
    CompileStructureKey, EmitKey, ParseKey, SemanticAdmissionBasis, SemanticKey, StageEpoch,
    TargetPlanKey, TerminalKey,
};
use crate::compile_request::{CompileProduct, CompileRequest, ProductKind, RuntimeProductRequest};
use crate::framework_common::VueCompileAdmission;

/// Contract epochs of the stages this route keys. A revision that changes
/// one stage advances only that stage's epoch.
const SEMANTIC_EPOCH: StageEpoch = StageEpoch(0);
const STRUCTURE_EPOCH: StageEpoch = StageEpoch(0);
const TARGET_PLAN_EPOCH: StageEpoch = StageEpoch(0);
const EMIT_EPOCH: StageEpoch = StageEpoch(0);
const TERMINAL_EPOCH: StageEpoch = StageEpoch(0);

/// How much of the demanded evidence an admission covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    Complete,
    /// Facts exist but are approximate; cannot satisfy a complete admission.
    Approximate,
    Refused,
}

/// Evidence that the demanded source regions parsed. Which parse it was is
/// settled before this exists: the semantic authority admits only over the
/// artifact's own parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseAdmission {
    coverage: Coverage,
}

impl ParseAdmission {
    /// A parse that covered every demanded region without recovery.
    pub(crate) fn complete() -> Self {
        Self {
            coverage: Coverage::Complete,
        }
    }

    /// A parse that produced a tree only by recovering from errors. Its
    /// facts are approximate, so it never admits a complete compile.
    pub(crate) fn recovered() -> Self {
        Self {
            coverage: Coverage::Approximate,
        }
    }
}

/// Evidence of the fact families the semantic authority answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticAdmission {
    families: FactFamilies,
    coverage: Coverage,
}

impl SemanticAdmission {
    pub(crate) fn issued(families: FactFamilies, coverage: Coverage) -> Self {
        Self { families, coverage }
    }
}

/// The closed demand of one admitted compile and how completely its
/// evidence covers it. Held only inside [`VueCompileAdmission`].
#[derive(Debug)]
pub(crate) struct CompileAdmission {
    demand: DemandSet,
    coverage: Coverage,
    basis: SemanticAdmissionBasis,
}

/// An external input the compile cannot proceed without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDemand {
    /// Style block `style_index` must be supplied at `stage`.
    ExternalStyleStage { style_index: u32, stage: StyleStage },
}

/// Typed admission refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DemandRefusal {
    ParseIncomplete,
    /// The semantic admission covers fewer families than demanded, or is
    /// approximate or refused.
    SemanticIncomplete {
        missing: FactFamilies,
    },
}

/// The verdict of composing a demand with its admissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompileProgress {
    Admitted,
    /// Resume with the same demand once every listed input is supplied.
    NeedInputs(Vec<InputDemand>),
}

impl CompileAdmission {
    /// Admit `demand` at the coverage its evidence earns: complete only
    /// when [`Self::compose`] admits it.
    pub(crate) fn admit(
        demand: DemandSet,
        parse: &ParseAdmission,
        semantic: &SemanticAdmission,
        basis: SemanticAdmissionBasis,
    ) -> Self {
        let coverage = match Self::compose(&demand, parse, semantic, Vec::new()) {
            Ok(CompileProgress::Admitted) => Coverage::Complete,
            Ok(CompileProgress::NeedInputs(_)) | Err(_) => Coverage::Approximate,
        };
        Self {
            demand,
            coverage,
            basis,
        }
    }

    /// A request that cannot be compiled demands nothing.
    pub(crate) fn refused(basis: SemanticAdmissionBasis) -> Self {
        Self {
            demand: close_demand(&[]),
            coverage: Coverage::Refused,
            basis,
        }
    }

    /// The verdict over a demand, without running any analysis. The inputs
    /// are admissions and demands only: no source, program or allocator
    /// reaches this function, so it cannot parse or analyse anything.
    pub(crate) fn compose(
        demand: &DemandSet,
        parse: &ParseAdmission,
        semantic: &SemanticAdmission,
        needed_inputs: Vec<InputDemand>,
    ) -> Result<CompileProgress, DemandRefusal> {
        if parse.coverage != Coverage::Complete {
            return Err(DemandRefusal::ParseIncomplete);
        }
        let demanded = demand.fact_families();
        // Approximate or refused facts satisfy no demanded family.
        let missing = if semantic.coverage == Coverage::Complete {
            FactFamilies(demanded.0 & !semantic.families.0)
        } else {
            demanded
        };
        if !missing.is_empty() || semantic.coverage != Coverage::Complete {
            return Err(DemandRefusal::SemanticIncomplete { missing });
        }
        if !needed_inputs.is_empty() {
            return Ok(CompileProgress::NeedInputs(needed_inputs));
        }
        Ok(CompileProgress::Admitted)
    }

    pub(crate) fn demand(&self) -> &DemandSet {
        &self.demand
    }

    pub(crate) fn coverage(&self) -> Coverage {
        self.coverage
    }
}

/// The stage identities of one admitted compile.
#[derive(Debug, Clone)]
pub struct StageKeys {
    pub semantic: SemanticKey,
    pub structure: CompileStructureKey,
    pub target_plan: Vec<TargetPlanKey>,
    pub emit: Vec<EmitKey>,
    pub terminal: Vec<TerminalKey>,
}

impl StageKeys {
    /// Derive every stage key from the admitted request's classified
    /// options and the admission. One entry per requested target, in
    /// demand order.
    pub fn derive(admission: &VueCompileAdmission) -> Self {
        Self::from_parts(
            admission.request(),
            admission.parse_key(),
            admission.compile_admission(),
        )
    }

    fn from_parts(
        request: &CompileRequest,
        parse_key: &ParseKey,
        admission: &CompileAdmission,
    ) -> Self {
        let options = ClassifiedOptions::classify(request);
        let semantic = SemanticKey::compose(
            std::slice::from_ref(parse_key),
            stage_options::framework_profile(request),
            &options.bytes(OptionStage::Semantic, &[]),
            SEMANTIC_EPOCH,
            &admission.basis,
        );
        let structure = CompileStructureKey::compose(
            &semantic,
            CanonicalDigest::of_bytes(&admission.demand.canonical_bytes()),
            &options.bytes(OptionStage::Structure, &[]),
            STRUCTURE_EPOCH,
        );
        let targets = admission.demand.targets();
        let mut keys = Self {
            semantic,
            structure,
            target_plan: Vec::with_capacity(targets.len()),
            emit: Vec::with_capacity(targets.len()),
            terminal: Vec::with_capacity(targets.len()),
        };
        for &(product, target) in targets {
            let runtime = runtime_product(request, product);
            let product_options = stage_options::product_entries(&runtime, request.is_production());
            let target_plan = TargetPlanKey::compose(
                &keys.structure,
                stage_options::target_digest(product, target),
                &options.bytes(OptionStage::TargetPlan, &product_options),
                TARGET_PLAN_EPOCH,
            );
            let emit = EmitKey::compose(
                &target_plan,
                &options.bytes(OptionStage::Emit, &product_options),
                stage_options::map_mode(runtime.runtime_source_map),
                EMIT_EPOCH,
            );
            let terminal = TerminalKey::compose(
                &emit,
                &options.bytes(OptionStage::Terminal, &product_options),
                TERMINAL_EPOCH,
            );
            keys.target_plan.push(target_plan);
            keys.emit.push(emit);
            keys.terminal.push(terminal);
        }
        keys
    }
}

/// The runtime options of one demanded product. Demand is seeded only from
/// runtime products, so a product the request lacks keys as the defaults.
fn runtime_product(request: &CompileRequest, product: ProductKind) -> RuntimeProductRequest {
    request
        .products()
        .iter()
        .find_map(|requested| match requested {
            CompileProduct::RuntimeClient(runtime) | CompileProduct::RuntimeServer(runtime)
                if requested.kind() == product =>
            {
                Some(runtime.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
    use verter_identity::profile::OutputProfileId;

    use super::*;
    use crate::compile_request::demand::{close_demand, DemandSeed, FactFamily, TargetKind};
    use crate::compile_request::{
        FrameworkCompileRequest, RuntimeStyleProcessing, VueCompileRequest,
    };

    struct ParseFixture(&'static str);

    impl CanonicalEncode for ParseFixture {
        const DOMAIN_TAG: &'static str = "demand-admission-test.parse.v1";

        fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
            encoder.field_str(1, self.0);
        }
    }

    fn parse_key(label: &'static str) -> ParseKey {
        ParseKey::from_canonical(&ParseFixture(label))
    }

    fn basis() -> SemanticAdmissionBasis {
        SemanticAdmissionBasis::from_parts(&[CanonicalDigest::of_bytes(b"region")], &[], &[])
    }

    fn styled_client_demand() -> DemandSet {
        close_demand(&[DemandSeed {
            product: ProductKind::RuntimeClient,
            target: TargetKind::ClientTree,
            runtime_source_map: false,
            style_processing: RuntimeStyleProcessing::Complete,
            has_template: true,
            has_script: true,
            style_count: 1,
        }])
    }

    fn complete_semantic(demand: &DemandSet) -> SemanticAdmission {
        SemanticAdmission::issued(demand.fact_families(), Coverage::Complete)
    }

    fn vue_request(
        edit: impl FnOnce(&mut VueCompileRequest, &mut RuntimeProductRequest, &mut bool),
    ) -> CompileRequest {
        let mut vue = VueCompileRequest::default();
        let mut runtime = RuntimeProductRequest::default();
        let mut is_production = false;
        edit(&mut vue, &mut runtime, &mut is_production);
        CompileRequest::new(
            vec![CompileProduct::RuntimeClient(runtime)],
            FrameworkCompileRequest::Vue(vue),
            None,
            Some("App.vue".to_owned()),
            Some("abc123".to_owned()),
            is_production,
            false,
        )
        .expect("a valid client request")
    }

    fn admitted(demand: DemandSet) -> CompileAdmission {
        let semantic = complete_semantic(&demand);
        let parse = ParseAdmission::complete();
        let admission = CompileAdmission::admit(demand, &parse, &semantic, basis());
        assert_eq!(admission.coverage(), Coverage::Complete);
        admission
    }

    /// Close the request's demand and admit it, as the compile entry does.
    fn keys_for(request: &CompileRequest) -> StageKeys {
        let seeds: Vec<DemandSeed> = request
            .products()
            .iter()
            .filter_map(|product| match product {
                CompileProduct::RuntimeClient(runtime) => Some(DemandSeed {
                    product: ProductKind::RuntimeClient,
                    target: TargetKind::ClientTree,
                    runtime_source_map: runtime.runtime_source_map,
                    style_processing: runtime.style_processing,
                    has_template: true,
                    has_script: true,
                    style_count: 1,
                }),
                _ => None,
            })
            .collect();
        let admission = admitted(close_demand(&seeds));
        StageKeys::from_parts(request, &parse_key("a"), &admission)
    }

    /// Which stage keys differ between two derivations, earliest first.
    fn rekeyed(a: &StageKeys, b: &StageKeys) -> [bool; 5] {
        [
            a.semantic != b.semantic,
            a.structure != b.structure,
            a.target_plan != b.target_plan,
            a.emit != b.emit,
            a.terminal != b.terminal,
        ]
    }

    #[test]
    fn an_option_changes_only_its_own_stage_and_later() {
        let base = keys_for(&vue_request(|_, _, _| {}));
        // [semantic, structure, target plan, emit, terminal]
        let rows: [(&str, StageKeys, [bool; 5]); 6] = [
            (
                "comments",
                keys_for(&vue_request(|vue, _, _| vue.comments = Some(true))),
                [false, true, true, true, true],
            ),
            (
                "runtime_source_map",
                keys_for(&vue_request(|_, runtime, _| {
                    runtime.runtime_source_map = true
                })),
                [false, false, false, true, true],
            ),
            (
                "hoist_static",
                keys_for(&vue_request(|vue, _, _| vue.hoist_static = Some(true))),
                [false, false, true, true, true],
            ),
            (
                "is_production",
                keys_for(&vue_request(|_, _, production| *production = true)),
                [true, true, true, true, true],
            ),
            (
                "runtime_module_name",
                keys_for(&vue_request(|vue, _, _| {
                    vue.runtime_module_name = Some("@vue/runtime-dom".to_owned())
                })),
                [false, false, false, true, true],
            ),
            (
                "output_profile",
                keys_for(&vue_request(|_, runtime, _| {
                    runtime.output_profile =
                        Some(OutputProfileId::from_canonical(&ParseFixture("minified")))
                })),
                [false, false, false, false, true],
            ),
        ];
        for (option, keys, expected) in rows {
            assert_eq!(rekeyed(&base, &keys), expected, "flipping {option}");
        }
    }

    #[test]
    fn equal_requests_give_equal_keys() {
        fn edit(vue: &mut VueCompileRequest, runtime: &mut RuntimeProductRequest, _: &mut bool) {
            vue.comments = Some(false);
            vue.is_custom_element = vec!["my-".to_owned()];
            runtime.runtime_source_map = true;
        }
        let a = keys_for(&vue_request(edit));
        let b = keys_for(&vue_request(edit));
        assert_eq!(rekeyed(&a, &b), [false; 5]);
        assert_eq!(a.target_plan.len(), 1);
        assert_eq!(a.emit.len(), 1);
        assert_eq!(a.terminal.len(), 1);
    }

    #[test]
    fn each_target_gets_its_own_plan_keys() {
        let request = CompileRequest::new(
            vec![
                CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
            ],
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            Some("App.vue".to_owned()),
            None,
            false,
            false,
        )
        .expect("a valid client and server request");
        let seed = |product, target| DemandSeed {
            product,
            target,
            runtime_source_map: false,
            style_processing: RuntimeStyleProcessing::Complete,
            has_template: true,
            has_script: true,
            style_count: 0,
        };
        let admission = admitted(close_demand(&[
            seed(ProductKind::RuntimeServer, TargetKind::Server),
            seed(ProductKind::RuntimeClient, TargetKind::ClientTree),
        ]));
        let keys = StageKeys::from_parts(&request, &parse_key("a"), &admission);
        assert_eq!(keys.target_plan.len(), 2);
        assert_ne!(keys.target_plan[0], keys.target_plan[1]);
        assert_ne!(keys.terminal[0], keys.terminal[1]);
    }

    #[test]
    fn a_recovered_parse_is_not_complete_coverage() {
        let demand = styled_client_demand();
        let semantic = complete_semantic(&demand);
        let parse = ParseAdmission::recovered();
        let verdict = CompileAdmission::compose(&demand, &parse, &semantic, Vec::new());
        assert_eq!(verdict, Err(DemandRefusal::ParseIncomplete));

        let recovered = CompileAdmission::admit(demand, &parse, &semantic, basis());
        assert_eq!(recovered.coverage(), Coverage::Approximate);
        assert_eq!(recovered.demand(), &styled_client_demand());
    }

    #[test]
    fn approximate_facts_do_not_admit() {
        let demand = styled_client_demand();
        let parse = ParseAdmission::complete();
        for coverage in [Coverage::Approximate, Coverage::Refused] {
            let semantic = SemanticAdmission::issued(demand.fact_families(), coverage);
            let verdict = CompileAdmission::compose(&demand, &parse, &semantic, Vec::new());
            let missing = demand.fact_families();
            assert_eq!(verdict, Err(DemandRefusal::SemanticIncomplete { missing }));

            let admission = CompileAdmission::admit(demand.clone(), &parse, &semantic, basis());
            assert_eq!(admission.coverage(), Coverage::Approximate);
        }
    }

    #[test]
    fn a_missing_family_is_named() {
        let demand = styled_client_demand();
        assert!(demand.fact_families().contains(FactFamily::Reactivity));
        let reactivity = FactFamilies::empty().with(FactFamily::Reactivity);
        let without_reactivity = FactFamilies(demand.fact_families().0 & !reactivity.0);
        let semantic = SemanticAdmission::issued(without_reactivity, Coverage::Complete);
        let parse = ParseAdmission::complete();
        let verdict = CompileAdmission::compose(&demand, &parse, &semantic, Vec::new());
        let missing = reactivity;
        assert_eq!(verdict, Err(DemandRefusal::SemanticIncomplete { missing }));
    }

    #[test]
    fn extra_families_still_admit() {
        let demand = styled_client_demand();
        let semantic = SemanticAdmission::issued(FactFamilies(0b11_1111), Coverage::Complete);
        let parse = ParseAdmission::complete();
        let verdict = CompileAdmission::compose(&demand, &parse, &semantic, Vec::new());
        assert_eq!(verdict, Ok(CompileProgress::Admitted));
    }

    #[test]
    fn missing_style_stage_asks_for_input_and_keeps_the_demand() {
        let demand = styled_client_demand();
        let semantic = complete_semantic(&demand);
        let parse = ParseAdmission::complete();
        let needed = vec![InputDemand::ExternalStyleStage {
            style_index: 0,
            stage: StyleStage::Preprocessed,
        }];
        let first = CompileAdmission::compose(&demand, &parse, &semantic, needed.clone());
        assert_eq!(first, Ok(CompileProgress::NeedInputs(needed)));

        let resumed = CompileAdmission::compose(&demand, &parse, &semantic, Vec::new());
        assert_eq!(resumed, Ok(CompileProgress::Admitted));
        assert_eq!(admitted(demand).demand(), &styled_client_demand());
    }
}
