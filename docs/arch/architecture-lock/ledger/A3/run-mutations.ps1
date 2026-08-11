$ErrorActionPreference = 'Continue'
$env:CARGO_TARGET_DIR = '<EVIDENCE>\A3\mutation-worktree\target'
Set-Location (Join-Path $PSScriptRoot 'mutation-worktree')

function Count-Text([string]$Haystack, [string]$Needle) {
    if ($Needle.Length -eq 0) { return 0 }
    return ([regex]::Matches($Haystack, [regex]::Escape($Needle))).Count
}

function Invoke-Mutation {
    param(
        [string]$Id,
        [string]$File,
        [string]$Needle,
        [string]$Replacement,
        [string]$Test
    )
    if ($env:MUTATION_PHASE -eq 'guards' -and $Id.StartsWith('T')) { return }
    if ($env:MUTATION_ONLY -and $Id -ne $env:MUTATION_ONLY) { return }
    $head = (git rev-parse HEAD).Trim()
    if ($head -ne '81d3f85044782851ad454d03e28e12edb6cc650b') {
        throw "$Id wrong baseline HEAD $head"
    }
    $statusBefore = (git status --porcelain -- $File | Out-String).Trim()
    if ($statusBefore) { throw "$Id target dirty before plant: $statusBefore" }
    $path = Join-Path (Get-Location) $File
    $bytes = [System.IO.File]::ReadAllBytes($path)
    $beforeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash
    $text = [System.Text.Encoding]::UTF8.GetString($bytes)
    $marker = "MUTATION_EVIDENCE_$Id"
    if ((Count-Text $text $Needle) -ne 1) { throw "$Id needle is not unique" }
    if ((Count-Text $text $marker) -ne 0) { throw "$Id marker already present" }
    $planted = $text.Replace($Needle, $Replacement)
    [System.IO.File]::WriteAllText($path, $planted, [System.Text.UTF8Encoding]::new($false))
    $after = [System.IO.File]::ReadAllText($path)
    $afterHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash
    if ((Count-Text $after $Needle) -ne 0) { throw "$Id original survived plant" }
    if ((Count-Text $after $marker) -ne 1) { throw "$Id marker not present exactly once" }
    if ($afterHash -eq $beforeHash) { throw "$Id plant did not change the blob" }
    try {
        $output = (& cargo nextest run -p verter_session $Test --no-fail-fast 2>&1 | Out-String)
        $exitCode = $LASTEXITCODE
        if ($exitCode -eq 0 -or $output -notmatch 'FAIL') {
            throw "$Id did not produce the named test failure (exit=$exitCode): $output"
        }
    }
    finally {
        [System.IO.File]::WriteAllBytes($path, $bytes)
    }
    $restoredHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash
    $statusAfter = (git status --porcelain -- $File | Out-String).Trim()
    if ($restoredHash -ne $beforeHash) { throw "$Id restoration hash mismatch" }
    if ($statusAfter) { throw "$Id target dirty after restore: $statusAfter" }
    Write-Output "$Id|$File|$Test|RED|plant=$afterHash|restored=$restoredHash|status=EMPTY"
}

$testFile = 'crates/verter_session/src/u6_flow_gap_retraction_tests.rs'

Invoke-Mutation 'T1' $testFile @'
            "g1",
            "function makeProps(x: string) { if (typeof x === \"number\") return \"dead\" as const; return \"live\" as const }",
            FlowGap::GuardNarrowing,
'@ @'
            "g1", /* MUTATION_EVIDENCE_T1 */
            "function makeProps() { return \"live\" as const }",
            FlowGap::GuardNarrowing,
'@ 'flow_gap_known_gap_results_are_typed_partial_and_never_warm'

Invoke-Mutation 'T2' $testFile @'
                Some(IIFE_EFFECT_REFUSAL),
                "{position:?}: {trace:#?}"
'@ @'
                None, /* MUTATION_EVIDENCE_T2 */
                "{position:?}: {trace:#?}"
'@ 'flow_gap_invoked_closure_effect_is_position_independent_no_value'

Invoke-Mutation 'T3' $testFile @'
    for function in ["explicit", "implicit", "rest", "callAny", "asserted"] {
'@ @'
    for function in ["explicit", "implicit", "rest", "callAny", "asserted", "missing" /* MUTATION_EVIDENCE_T3 */] {
'@ 'flow_gap_authored_any_remains_complete_and_warm'

Invoke-Mutation 'T4' $testFile @'
    for depth in 0..65 {
'@ @'
    for depth in 0..1 { /* MUTATION_EVIDENCE_T4 */
'@ 'flow_gap_default_parameter_budget_failure_is_no_value_and_cold'

Invoke-Mutation 'T5' $testFile @'
    assert_partial(&root, FlowGap::UnmodeledExpression);
'@ @'
    assert_partial(&root, FlowGap::GuardNarrowing); /* MUTATION_EVIDENCE_T5 */
'@ 'flow_gap_partial_propagates_through_consumer_and_scc_gates'

Invoke-Mutation 'T6' 'crates/verter_session/src/flow_slice_content.rs' @'
        if completeness == ExpressionInferenceCompleteness::Unmodeled
            && !self.optional_member_read_is_semantic_any(expr)
'@ @'
        if completeness == ExpressionInferenceCompleteness::Unmodeled /* MUTATION_EVIDENCE_T6 */
            && true
'@ 'flow_gap_false_refusal_controls_remain_complete_and_warm'

Invoke-Mutation 'T7' 'crates/verter_session/src/u6_flow_expect_tests.rs' @'
        assert_eq!(error, IIFE_EFFECT_REFUSAL);
'@ @'
        assert_ne!(error, IIFE_EFFECT_REFUSAL); /* MUTATION_EVIDENCE_T7 */
'@ 'uniform_iife_effect_refusal_covers_every_position'

Invoke-Mutation 'T8' 'crates/verter_session/src/u6_flow_shape_corpus_tests.rs' @'
        "d8a8f42a671509c0a02b379c89644b21c3a8e7ce69d381354571b93ef980b9d0",
'@ @'
        "08a8f42a671509c0a02b379c89644b21c3a8e7ce69d381354571b93ef980b9d0", /* MUTATION_EVIDENCE_T8 */
'@ 'flow_gap_retraction_preserves_clean_checker_matches'

Invoke-Mutation 'T9' 'crates/verter_session/src/u6_flow_shape_corpus_tests.rs' @'
            matches!(row.verdict, Verdict::MatchesChecker)
                && matches!(
'@ @'
            matches!(row.verdict, Verdict::MatchesChecker)
                && row.id != "X05_catch_return_fallthrough" /* MUTATION_EVIDENCE_T9 */
                && matches!(
'@ 'flow_gap_retraction_preserves_clean_checker_matches'

Invoke-Mutation 'G1' 'crates/verter_session/src/project_semantic_dispatch/flow_return.rs' @'
    fn record_degradation(&mut self, degradation: crate::semantic_query::FlowReturnDegradation) {
        self.degradation.get_or_insert(degradation);
    }
'@ @'
    fn record_degradation(&mut self, degradation: crate::semantic_query::FlowReturnDegradation) {
        if degradation == crate::semantic_query::FlowReturnDegradation::FlowGap(
            crate::semantic_query::FlowGap::GuardNarrowing,
        ) { return; } /* MUTATION_EVIDENCE_G1 */
        self.degradation.get_or_insert(degradation);
    }
'@ 'flow_gap_known_gap_results_are_typed_partial_and_never_warm'

Invoke-Mutation 'G2' 'crates/verter_session/src/project_semantic_dispatch/flow_return.rs' @'
                    crate::semantic_query::FlowGap::NominalRelation,
'@ @'
                    crate::semantic_query::FlowGap::GuardNarrowing, /* MUTATION_EVIDENCE_G2 */
'@ 'flow_gap_known_gap_results_are_typed_partial_and_never_warm'

Invoke-Mutation 'G3' 'crates/verter_session/src/flow_slice_content.rs' @'
                gap = Some(crate::semantic_query::FlowGap::ClosureCapture);
'@ @'
                gap = None; /* MUTATION_EVIDENCE_G3 */
'@ 'flow_gap_known_gap_results_are_typed_partial_and_never_warm'

Invoke-Mutation 'G4' 'crates/verter_semantic/src/analysis/type_eval_build.rs' @'
        _ => {
            budget.used_unmodeled_fallback = true;
            Ok(TypeExpr::Primitive(PrimitiveName::Any))
        }
'@ @'
        _ => {
            budget.used_unmodeled_fallback = false; /* MUTATION_EVIDENCE_G4 */
            Ok(TypeExpr::Primitive(PrimitiveName::Any))
        }
'@ 'flow_gap_known_gap_results_are_typed_partial_and_never_warm'

Invoke-Mutation 'G5' 'crates/verter_session/src/flow_slice_content.rs' @'
            if self.span_contains_unsafe_invoked_closure(statement.span()) {
'@ @'
            if false && self.span_contains_unsafe_invoked_closure(statement.span()) { /* MUTATION_EVIDENCE_G5 */
'@ 'flow_gap_invoked_closure_effect_is_position_independent_no_value'

Invoke-Mutation 'G6' 'crates/verter_session/src/project_semantic_dispatch/flow_return.rs' @'
                if degraded {
                    // Degraded SUCCESS: a usable value, ReturnOnly by the
'@ @'
                if false && degraded { /* MUTATION_EVIDENCE_G6 */
                    // Degraded SUCCESS: a usable value, ReturnOnly by the
'@ 'flow_gap_known_gap_results_are_typed_partial_and_never_warm'

Write-Output 'ALL_MUTATIONS_RESTORED'
