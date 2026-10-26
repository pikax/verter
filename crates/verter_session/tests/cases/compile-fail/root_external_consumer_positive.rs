//! A pollable host root is production API an external driver consumes with
//! neither `test-support` nor any private store access: it begins a root,
//! polls it with its own waker, runs the dependency work the root hands it
//! and polls again, all on one thread, until the root answers. Gating the
//! roots, their polls or their work on a support feature fails this
//! contract.

use std::sync::Arc;
use std::task::{Poll, Wake, Waker};

use verter_session::{
    CompileRequestRoot, ComponentMetaOutputRoot, FileLanguage, HostConfig, RootWork,
    UpsertRequest, VerterHost,
};

struct Unpark(std::thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Run handed-out work here; park when there is none.
fn run_or_park(work: Option<RootWork<'_>>) {
    match work {
        Some(work) => {
            let _ = work.is_producer();
            if !work.run() {
                std::thread::park();
            }
        }
        None => std::thread::park(),
    }
}

fn meta(mut root: ComponentMetaOutputRoot<'_>, waker: &Waker) -> bool {
    loop {
        if let Poll::Ready(answer) = root.poll(waker) {
            return matches!(answer, Ok(Some(_)));
        }
        run_or_park(root.take_work());
    }
}

fn compile(mut root: CompileRequestRoot<'_>, waker: &Waker) -> bool {
    loop {
        if let Poll::Ready(answer) = root.poll(waker) {
            return answer.is_ok();
        }
        run_or_park(root.take_work());
    }
}

fn main() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/src/Owner.vue".to_owned()),
        input_id: "/src/Owner.vue".to_owned(),
        source: Arc::from(
            "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template><b>{{ label }}</b></template>",
        ),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .expect("the owner registers");
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    assert!(meta(host.begin_component_meta_output("/src/Owner.vue"), &waker));
    let request = verter_compiler::compile_request::CompileRequest::new(
        vec![verter_compiler::compile_request::CompileProduct::IdeCompanion(
            verter_compiler::compile_request::IdeProductRequest::default(),
        )],
        verter_compiler::compile_request::FrameworkCompileRequest::Vue(
            verter_compiler::compile_request::VueCompileRequest::default(),
        ),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("the demand constructs");
    assert!(compile(host.begin_compile_request("/src/Owner.vue", request), &waker));
}
