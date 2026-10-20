//! Audit capture never changes an answer.
//!
//! The same component-meta, compile and named-type requests run cold on
//! fresh hosts with capture switched off and on, through both the plain and
//! the audited entry points, and every answer must be identical at the wire
//! boundary. Component meta carries per-host opaque tokens (artifact, block,
//! source-space and encoding-session tokens) that differ between any two
//! hosts, so its wire message is compared with those token values masked.
//! Its origin graph is the audit trace itself, so it is compared separately:
//! present exactly when the host captures, and outside the answer. The suite runs in the default build, where capture is
//! compiled out and every audited entry reports it unavailable, and in the
//! instrumentation lane, where the audited host records full captures.

use std::sync::Arc;

use verter_audit::AuditCaptureState;
use verter_session::component_meta_host::{ComponentMetaHost, ComponentMetaHostError};
use verter_session::{CompileTarget, HostConfig, VerterHost};

const TYPES: &str = "/src/types.ts";
const COMPONENT: &str = "/src/Comp.vue";

const TYPES_SOURCE: &str = "export interface Base { id: number; tags: string[] }\n\
export type Label<T> = T extends string ? 'text' : 'other';\n\
export type Probe = Pick<Base, 'id'> & { kind: Label<'x'>; rest: Omit<Base, 'id'> };\n";

const COMPONENT_SOURCE: &str = "<script setup lang=\"ts\">\n\
import type { Base, Label } from './types';\n\
const props = defineProps<Base & { label: Label<string>; size?: 'sm' | 'lg' }>();\n\
const emit = defineEmits<{ (e: 'select', id: number): void }>();\n\
</script>\n\
<template><button @click=\"emit('select', props.id)\">{{ props.label }}</button></template>\n";

/// Every answer one host gives, encoded at the wire boundary.
#[derive(Debug, PartialEq, Eq)]
struct Answers {
    component_meta: String,
    ide_tsx: String,
    ide_source_map: String,
    bundler_script: String,
    bundler_template: String,
    probe: Vec<u8>,
}

/// Which entry points a host is driven through.
#[derive(Clone, Copy, Debug)]
enum Entry {
    Plain,
    Audited,
}

fn host(audit_enabled: bool) -> (Arc<VerterHost>, ComponentMetaHost) {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled,
        footprint_capture: audit_enabled,
        audit_timing_capture: audit_enabled,
        ..HostConfig::default()
    }));
    let meta = ComponentMetaHost::new_shared_host(Arc::clone(&host));
    meta.upsert_base(TYPES, TYPES_SOURCE).expect("types upsert");
    meta.upsert_base(COMPONENT, COMPONENT_SOURCE)
        .expect("component upsert");
    (host, meta)
}

/// Whether a host built with `audit_enabled` records captures in this build.
fn captures(audit_enabled: bool) -> bool {
    audit_enabled
        && verter_session::capture::availability() == verter_audit::CaptureAvailability::Available
}

/// The component-meta wire message, rendered with every per-host token
/// value masked, after a first line naming whether it carried the audit
/// origin graph (which the rendering then omits).
fn encode(output: verter_session::meta_resolve::ComponentMetaOutput) -> Vec<u8> {
    let mut payload = verter_protocol::component_meta::component_meta_payload(
        &super::component_meta_output_to_ffi(output),
    );
    let traced = payload.origin_graph.take().is_some();
    format!("{traced}\n{}", mask_tokens(&format!("{payload:?}"))).into_bytes()
}

/// Replace the value of every `…token` / `…tokens` field in a message's
/// `Debug` rendering with a fixed marker. The value is skipped as one
/// balanced expression, so lists and nested token messages mask whole.
fn mask_tokens(debug: &str) -> String {
    let bytes = debug.as_bytes();
    let mut out = String::with_capacity(debug.len());
    let mut i = 0;
    while i < bytes.len() {
        let key_end = ["token: ", "tokens: "]
            .iter()
            .find(|key| debug[i..].starts_with(*key))
            .map(|key| i + key.len());
        let Some(value_start) = key_end else {
            let ch = debug[i..].chars().next().expect("in bounds");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        };
        out.push_str(&debug[i..value_start]);
        out.push_str("<token>");
        let mut depth = 0usize;
        let mut j = value_start;
        while j < bytes.len() {
            match bytes[j] {
                b'"' => {
                    j += 1;
                    while bytes[j] != b'"' {
                        j += if bytes[j] == b'\\' { 2 } else { 1 };
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth == 0 => break,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => break,
                _ => {}
            }
            j += 1;
        }
        i = j;
    }
    out
}

/// What one cold host returned: its answers, the capture state of every
/// audited request it made, and whether its component meta carried the
/// audit origin graph.
struct Run {
    answers: Answers,
    states: Vec<AuditCaptureState>,
    traced: bool,
}

/// Drive one cold host through `entry`.
fn run(audit_enabled: bool, entry: Entry) -> Run {
    let (host, meta) = host(audit_enabled);
    let session = meta.open_session().expect("session opens");
    let mut states = Vec::new();

    let component_meta = match entry {
        Entry::Plain => session
            .get_component_meta_payload(COMPONENT, encode)
            .expect("component meta resolves")
            .expect("the component has meta"),
        Entry::Audited => {
            let (outcome, record) = session
                .get_component_meta_with_audit(COMPONENT)
                .into_parts();
            states.push(record.capture_state);
            match outcome {
                Ok(output) => {
                    assert!(
                        captures(audit_enabled),
                        "the audited component-meta lane answers only on a capturing host"
                    );
                    encode(output.expect("the component has meta"))
                }
                // The audited component-meta lane refuses on a host that
                // cannot capture; the plain lane still answers there.
                Err(ComponentMetaHostError::AuditNotEnabled) => {
                    assert!(
                        !captures(audit_enabled),
                        "a capturing host must not refuse the audited lane"
                    );
                    session
                        .get_component_meta_payload(COMPONENT, encode)
                        .expect("component meta resolves")
                        .expect("the component has meta")
                }
                Err(error) => panic!("component meta resolves: {error}"),
            }
        }
    };

    // Compile has no plain host entry: the audited one is the only lane,
    // and it runs whether or not capture is on.
    let compile = |target| {
        let (outcome, record) = host.compile_with_audit(COMPONENT, target).into_parts();
        let Ok(result) = outcome;
        (result, record.capture_state)
    };
    let (ide, ide_state) = compile(CompileTarget::IDE);
    let (bundler, bundler_state) = compile(CompileTarget::BUNDLER);
    states.extend([ide_state, bundler_state]);

    let probe_node = match entry {
        Entry::Plain => host.resolve_named_symbol(TYPES, "Probe", None),
        Entry::Audited => {
            let (outcome, record) = host
                .resolve_named_symbol_with_audit(TYPES, "Probe", None)
                .into_parts();
            states.push(record.capture_state);
            outcome.expect("the named demand completes")
        }
    }
    .expect("Probe resolves");
    let probe = host
        .project_node_to_type_expr_json_bytes(&probe_node)
        .expect("Probe projects");

    let component_meta = String::from_utf8(component_meta).expect("a UTF-8 rendering");
    let (traced, component_meta) = component_meta
        .split_once('\n')
        .expect("the trace line leads the rendering");
    let answers = Answers {
        component_meta: component_meta.to_owned(),
        ide_tsx: ide.tsx.as_ref().expect("an IDE tsx block").code.clone(),
        ide_source_map: ide.tsx.expect("an IDE tsx block").source_map,
        bundler_script: bundler.script.expect("a bundler script").code,
        bundler_template: bundler
            .template
            .map(|template| template.code)
            .unwrap_or_default(),
        probe,
    };
    Run {
        answers,
        states,
        traced: traced == "true",
    }
}

#[test]
fn capture_on_and_off_give_identical_answers_through_every_entry() {
    let Run {
        answers: reference,
        states: off_states,
        traced: reference_traced,
    } = run(false, Entry::Plain);
    assert!(!reference_traced, "a host with capture off traces nothing");
    assert!(
        reference.component_meta.contains("<token>") && !reference.probe.is_empty(),
        "the fixture resolves real answers: {reference:?}"
    );
    assert!(
        off_states.iter().all(|state| matches!(
            state,
            AuditCaptureState::AuditDisabled | AuditCaptureState::Unavailable
        )),
        "a host with capture off records nothing: {off_states:?}"
    );

    for (audit_enabled, entry) in [
        (false, Entry::Audited),
        (true, Entry::Plain),
        (true, Entry::Audited),
    ] {
        let Run {
            answers,
            states,
            traced,
        } = run(audit_enabled, entry);
        assert_eq!(
            answers, reference,
            "capture must not change an answer (audit_enabled={audit_enabled}, entry={entry:?})"
        );
        assert_eq!(
            traced,
            captures(audit_enabled),
            "the origin graph rides the payload exactly when the host captures \
             (audit_enabled={audit_enabled}, entry={entry:?})"
        );
        if audit_enabled && matches!(entry, Entry::Audited) {
            // The comparison is only meaningful if the audited host really
            // captured: every audited request stores a record when capture
            // is compiled in, and reports it unavailable when it is not.
            let expected = if captures(true) {
                AuditCaptureState::ActiveStored
            } else {
                AuditCaptureState::Unavailable
            };
            assert!(
                states.iter().all(|state| *state == expected),
                "every audited request on a capturing host is {expected:?}: {states:?}"
            );
        }
    }
}
