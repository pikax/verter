//! CSS-analysis phase timing surfaces through
//! `CompilePayload::css_analysis_ms` when the SFC has a `<style>` block
//! and the producer is exercising `compile.css_analysis` instrumentation.
//!
//! Discrimination contract:
//! - Without the producer emit: the
//!   `record_phase_timing("compile.css_analysis", ...)` call site does
//!   not run; the per-request `compile_css_analysis_us` accumulator
//!   stays at 0; `payload.css_analysis_ms` is `None`.
//! - With the producer emit: the producer emits at the boundary; payload
//!   is `Some(>= 0)` for SFCs that have at least one `<style>` block.

use std::sync::Arc;

use verter_compiler::compile::CompileTarget;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SFC_WITH_STYLE: &str = "<script setup lang=\"ts\">\n\
                              const greeting = 'hello';\n\
                              </script>\n\
                              <template><div class=\"box\">{{ greeting }}</div></template>\n\
                              <style scoped>.box { color: red; padding: 4px; }</style>\n";

const SFC_WITHOUT_STYLE: &str = "<script setup lang=\"ts\">\n\
                                 const greeting = 'hi';\n\
                                 </script>\n\
                                 <template><div>{{ greeting }}</div></template>\n";

fn host_with(canonical: &str, source: &'static str) -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.into()),
        input_id: canonical.into(),
        source: Arc::from(source),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });
    host
}

#[test]
fn compile_with_audit_populates_css_analysis_ms_when_style_block_present() {
    let host = host_with("/withstyle.vue", SFC_WITH_STYLE);
    let (result, record) = host
        .compile_with_audit("/withstyle.vue", CompileTarget::BUNDLER)
        .into_parts();
    let result = match result {
        Ok(r) => r,
        Err(e) => match e {},
    };

    assert!(
        !result.styles.is_empty(),
        "fixture has a <style> block; codegen should emit a styles entry",
    );

    let payload = record
        .compile_payload()
        .cloned()
        .expect("Compile kind ⇒ CompilePayload");

    // Discriminator: the producer emits one `compile.css_analysis`
    // record per compile when at least one <style> block is present.
    // A regression that drops the emit (or the wiring on the
    // RequestContext side) would leave this at None.
    assert!(
        payload.css_analysis_ms.is_some(),
        "SFC with <style> block must surface css_analysis_ms = Some; \
         a missing producer emit leaves it as None. payload = {payload:?}",
    );
    let css_ms = payload.css_analysis_ms.unwrap();
    assert!(
        css_ms >= 0.0,
        "css_analysis_ms must be non-negative; got {css_ms}",
    );
    assert_eq!(
        payload.num_style_blocks, 1,
        "fixture has exactly one <style> block",
    );
}

#[test]
fn compile_with_audit_leaves_css_analysis_ms_none_when_no_style_block() {
    let host = host_with("/nostyle.vue", SFC_WITHOUT_STYLE);
    let record = host
        .compile_with_audit("/nostyle.vue", CompileTarget::BUNDLER)
        .audit()
        .clone();

    let payload = record.compile_payload().cloned().expect("CompilePayload");

    // Negative discriminator: an SFC with no <style> blocks must
    // leave css_analysis_ms at None — protects against a producer
    // that always emits regardless of input shape.
    assert!(
        payload.css_analysis_ms.is_none(),
        "SFC without <style> blocks must leave css_analysis_ms = None; \
         a producer that emits unconditionally would surface Some(0.0) here. \
         payload = {payload:?}",
    );
    assert_eq!(payload.num_style_blocks, 0);
}
