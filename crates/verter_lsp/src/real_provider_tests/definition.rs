//! Go-to-definition tests ported from E2E suite.

use crate::test_harness::{real_provider_test, RealProviderTestSession};
use tower_lsp_server::ls_types::Position;

// ---------------------------------------------------------------------------
// Same-file definitions (script bindings used in template)
// ---------------------------------------------------------------------------

real_provider_test!(
    definition_same_file,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        if !session.require_or_skip_ready(&uri, "action.disabled", 7, "disabled").await {
            return;
        }

        let uri_path = RealProviderTestSession::uri_to_path(&uri);

        // A1: {{ title }} → same file
        let pos = session.find_position(&uri, "{{ title }}", 3);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "{{ title }} should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("App.vue"), "title def should be in App.vue, got: {def_path}");
        assert!(!def_path.ends_with(".tsx"), "title def should NOT be in .tsx, got: {def_path}");

        // A2: {{ count }} → same file
        let pos = session.find_position(&uri, "{{ count }}", 3);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "{{ count }} should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert_eq!(def_path, uri_path, "count def should be same file");
        assert!(!def_path.ends_with(".tsx"), "count def should NOT be in .tsx");

        // A3: {{ doubled }} → same file
        let pos = session.find_position(&uri, "{{ doubled }}", 3);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "{{ doubled }} should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert_eq!(def_path, uri_path, "doubled def should be same file");
        assert!(!def_path.ends_with(".tsx"), "doubled def should NOT be in .tsx");

        // A4: @click="increment" → same file
        let delta = r#"@click=""#.len();
        let pos = session.find_position(&uri, r#"@click="increment">+"#, delta);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "increment should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert_eq!(def_path, uri_path, "increment def should be same file");
        assert!(!def_path.ends_with(".tsx"), "increment def should NOT be in .tsx");

        // A5: {{ formatted }} → same file
        let pos = session.find_position(&uri, "{{ formatted }}", 3);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "{{ formatted }} should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert_eq!(def_path, uri_path, "formatted def should be same file");
        assert!(!def_path.ends_with(".tsx"), "formatted def should NOT be in .tsx");
    }
);

real_provider_test!(
    definition_svelte_bind_value_lands_on_authored_state_binding,
    fixture = "svelte-parity",
    async fn run(session) {
        let uri = session.open_fixture_file("src/features/BindValue.svelte").await;
        let use_position = session.find_nth_position(&uri, "name", 1, 1);
        let declaration = session.find_nth_position(&uri, "name", 0, 0);

        let mut definitions = Vec::new();
        for _ in 0..16 {
            definitions = session.definition_locations(&uri, use_position).await;
            if definitions.iter().any(|location| {
                location.uri == uri && location.range.start == declaration
            }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert!(
            definitions.iter().any(|location| {
                location.uri == uri && location.range.start == declaration
            }),
            "bind:value must navigate from the authored bound expression to the authored state binding; expected={declaration:?}, got={definitions:?}"
        );
        assert!(
            definitions.iter().all(|location| {
                !location.uri.as_str().ends_with(".tsx")
            }),
            "same-file Svelte binding navigation must never leak the IDE carrier: {definitions:?}"
        );
    }
);

real_provider_test!(
    definition_svelte_component_shorthand_lands_on_authored_local,
    fixture = "svelte-parity",
    async fn run(session) {
        let uri = session
            .open_fixture_file("src/ide/IdeSurfaceParent.svelte")
            .await;
        let use_position = session.find_position(&uri, "{label}", 1);
        let declaration = session.find_nth_position(&uri, "label", 0, 0);

        let mut definitions = Vec::new();
        for _ in 0..16 {
            definitions = session.definition_locations(&uri, use_position).await;
            if definitions.iter().any(|location| {
                location.uri == uri && location.range.start == declaration
            }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert!(
            definitions.iter().any(|location| {
                location.uri == uri && location.range.start == declaration
            }),
            "Svelte component shorthand must navigate to its authored local binding; expected={declaration:?}, got={definitions:?}"
        );
    }
);

// ---------------------------------------------------------------------------
// Member-access definitions (`receiver.member` — the property token itself)
// ---------------------------------------------------------------------------

real_provider_test!(
    definition_member_access_lands_on_authored_property,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        if !session.require_or_skip_ready(&uri, "action.disabled", 7, "disabled").await {
            return;
        }

        let uri_path = RealProviderTestSession::uri_to_path(&uri);

        // M1: template `{{ props.title }}` — cursor on the MEMBER `title`.
        // The declaration is the authored `title` in `defineProps<{ title: string }>()`.
        // NOTE: `title` is a prop NAME, so the native `prop_fields` word match
        // (`features/definition.rs`) can also serve this position and the merge
        // prefers a same-file native result — this case pins the user-visible
        // outcome, not the reverse map. The provider-ONLY relocated-member
        // discrimination lives in M7/M8 below (`defineModel` nested member).
        let decl_pos = session.find_position(
            &uri,
            "defineProps<{ title: string }>",
            "defineProps<{ ".len(),
        );
        let pos = session.find_position(&uri, "{{ props.title }}", "{{ props.".len());
        let locs = session.definition_locations(&uri, pos).await;
        assert!(
            !locs.is_empty(),
            "member `title` in {{{{ props.title }}}} must resolve a definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == uri_path
                    && l.range.start == decl_pos
            }),
            "member `title` must land on the authored prop declaration at {decl_pos:?}; got: {locs:?}"
        );
        assert!(
            locs.iter().all(|l| !RealProviderTestSession::uri_to_path(&l.uri).ends_with(".tsx")),
            "member definition must never leak the IDE carrier: {locs:?}"
        );

        // M2: template `{{ user.name }}` — cursor on the MEMBER `name`.
        // The declaration is `name: string;` inside `interface User` (its first
        // byte-occurrence in the fixture).
        let user_decl_pos = session.find_position(&uri, "name: string;", 0);
        let pos = session.find_position(&uri, "{{ user.name }}", "{{ user.".len());
        let locs = session.definition_locations(&uri, pos).await;
        assert!(
            !locs.is_empty(),
            "member `name` in {{{{ user.name }}}} must resolve a definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == uri_path
                    && l.range.start == user_decl_pos
            }),
            "member `name` must land on the authored interface member at {user_decl_pos:?}; got: {locs:?}"
        );

        // M3: script `count.value * 2` — cursor on the MEMBER `value` (declared in
        // vue's own d.ts). The definition must land in a REAL `.d.ts` at the real
        // declaration coordinates — never the generated carrier and never a
        // line-0 collapse of some unrelated real file.
        let pos = session.find_position(&uri, "count.value * 2", "count.".len());
        let locs = session.definition_locations(&uri, pos).await;
        assert!(
            !locs.is_empty(),
            "member `value` in `count.value` must resolve a definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                let path = RealProviderTestSession::uri_to_path(&l.uri);
                // `.value` is declared deep inside vue's reactivity declarations —
                // never on line 0 of any real `.d.ts` — so a line-0 hit is a
                // collapsed/wrong-file range, whatever its character offset.
                path.ends_with(".d.ts") && l.range.start.line > 0
            }),
            "member `value` must land in a real .d.ts at its real (non-line-0) declaration \
             coordinates; got: {locs:?}"
        );
        assert!(
            locs.iter().all(|l| !RealProviderTestSession::uri_to_path(&l.uri).ends_with(".tsx")),
            "member definition must never leak the IDE carrier: {locs:?}"
        );

        // M4: template `{{ action.label }}` — member on a v-for iteration variable;
        // declaration is `label: string;` inside `interface Action` (its first
        // byte-occurrence in the fixture).
        let action_decl_pos = session.find_position(&uri, "label: string;", 0);
        let pos = session.find_position(&uri, "{{ action.label }}", "{{ action.".len());
        let locs = session.definition_locations(&uri, pos).await;
        assert!(
            !locs.is_empty(),
            "member `label` in {{{{ action.label }}}} must resolve a definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == uri_path
                    && l.range.start == action_decl_pos
            }),
            "member `label` must land on the authored interface member at {action_decl_pos:?}; got: {locs:?}"
        );

        // M5: member inside an ATTRIBUTE expression — `:disabled="action.disabled"`,
        // cursor on the MEMBER `disabled`. Declaration is `disabled: boolean;` in
        // `interface Action`. Attribute expressions lower through a different
        // template codegen shape than interpolations, so this position is NOT
        // subsumed by M4.
        let disabled_decl_pos = session.find_position(&uri, "disabled: boolean;", 0);
        let pos = session.find_position(
            &uri,
            r#":disabled="action.disabled""#,
            r#":disabled="action."#.len(),
        );
        let locs = session.definition_locations(&uri, pos).await;
        assert!(
            !locs.is_empty(),
            "member `disabled` in :disabled=\"action.disabled\" must resolve a definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == uri_path
                    && l.range.start == disabled_decl_pos
            }),
            "attribute-expression member `disabled` must land on the authored interface member at {disabled_decl_pos:?}; got: {locs:?}"
        );

        // M6: member inside an EVENT-HANDLER expression — `@click="action.handler"`,
        // cursor on the MEMBER `handler`. Declaration is `handler: () => void;` in
        // `interface Action`.
        let handler_decl_pos = session.find_position(&uri, "handler: () => void;", 0);
        let pos = session.find_position(
            &uri,
            r#"@click="action.handler""#,
            r#"@click="action."#.len(),
        );
        let locs = session.definition_locations(&uri, pos).await;
        assert!(
            !locs.is_empty(),
            "member `handler` in @click=\"action.handler\" must resolve a definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == uri_path
                    && l.range.start == handler_decl_pos
            }),
            "event-expression member `handler` must land on the authored interface member at {handler_decl_pos:?}; got: {locs:?}"
        );

        // M7/M8: the PROVIDER-ONLY relocated member. `size` in
        // `defineModel<{ size: number }>()` is not a prop field and not a
        // binding — no native leg can serve it — and IDE codegen RELOCATES the
        // inline `{ size: number }` text into the Prettify-wrapped macro type
        // alias, so the reverse map over MOVED authored text is the ONLY route
        // to the mapped result. A broken relocated-text reverse map fails
        // exactly here (M1 can be served natively; this cannot).
        let model_uri = session.open_fixture_file("src/ModelBox.vue").await;
        let model_uri_path = RealProviderTestSession::uri_to_path(&model_uri);
        let size_decl_pos =
            session.find_position(&model_uri, "{ size: number }", "{ ".len());

        // M7: script `box.value.size` — cursor on the MEMBER `size`. First pin
        // the provider-only premise: the RAW provider answer resolves into the
        // GENERATED carrier IDE surface, so the merged authored result below
        // can only come from the reverse map.
        let pos = session.find_position(&model_uri, "box.value.size", "box.value.".len());
        let raw = session.raw_provider_definitions(&model_uri, pos).await;
        assert!(
            raw.iter().any(|d| d.path.ends_with(".vue.tsx")),
            "the raw provider answer for the defineModel member must target the generated \
             IDE surface (the relocated macro type) — provider-only premise; got: {raw:?}"
        );
        let locs = session.definition_locations(&model_uri, pos).await;
        assert!(
            !locs.is_empty(),
            "provider-only member `size` in `box.value.size` must resolve a definition \
             (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == model_uri_path
                    && l.range.start == size_decl_pos
            }),
            "relocated defineModel member `size` must reverse-map to the authored \
             declaration at {size_decl_pos:?}; got: {locs:?}"
        );
        assert!(
            locs.iter().all(|l| {
                !RealProviderTestSession::uri_to_path(&l.uri).ends_with(".tsx")
            }),
            "relocated member definition must never leak the IDE carrier: {locs:?}"
        );

        // M8: template `{{ box.size }}` — the same provider-only member from
        // the template (auto-unwrapped model ref).
        let pos = session.find_position(&model_uri, "{{ box.size }}", "{{ box.".len());
        let locs = session.definition_locations(&model_uri, pos).await;
        assert!(
            !locs.is_empty(),
            "provider-only template member `size` in {{{{ box.size }}}} must resolve a \
             definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == model_uri_path
                    && l.range.start == size_decl_pos
            }),
            "template defineModel member `size` must reverse-map to the authored \
             declaration at {size_decl_pos:?}; got: {locs:?}"
        );
    }
);

// The SAME member contract on a Svelte carrier: this path is carrier-agnostic,
// so `{dailyValue.label}` markup members and script members must land on the
// authored interface member exactly as the Vue cases above do.
real_provider_test!(
    definition_svelte_member_access_lands_on_authored_property,
    fixture = "svelte-parity",
    async fn run(session) {
        let uri = session.open_fixture_file("src/DailyBinding.svelte").await;

        if !session
            .require_or_skip_ready(&uri, "dailyValue.count", "dailyValue.".len(), "count")
            .await
        {
            return;
        }

        let uri_path = RealProviderTestSession::uri_to_path(&uri);

        // S1: markup `{dailyValue.label}` — cursor on the MEMBER `label`; the
        // declaration is `label: string;` in `interface DailyValue`.
        let label_decl_pos = session.find_position(&uri, "label: string;", 0);
        let pos = session.find_position(&uri, "{dailyValue.label}", "{dailyValue.".len());
        let locs = session.definition_locations(&uri, pos).await;
        assert!(
            !locs.is_empty(),
            "Svelte markup member `label` must resolve a definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == uri_path
                    && l.range.start == label_decl_pos
            }),
            "Svelte markup member `label` must land on the authored interface member at {label_decl_pos:?}; got: {locs:?}"
        );
        assert!(
            locs.iter().all(|l| !RealProviderTestSession::uri_to_path(&l.uri).ends_with(".tsx")),
            "Svelte member definition must never leak the IDE carrier: {locs:?}"
        );

        // S2: script member — `dailyValue.count` inside `renderDaily`; the
        // declaration is `count: number;` in the same interface.
        let count_decl_pos = session.find_position(&uri, "count: number;", 0);
        let pos = session.find_position(&uri, "dailyValue.count}`", "dailyValue.".len());
        let locs = session.definition_locations(&uri, pos).await;
        assert!(
            !locs.is_empty(),
            "Svelte script member `count` must resolve a definition (got empty)"
        );
        assert!(
            locs.iter().any(|l| {
                RealProviderTestSession::uri_to_path(&l.uri) == uri_path
                    && l.range.start == count_decl_pos
            }),
            "Svelte script member `count` must land on the authored interface member at {count_decl_pos:?}; got: {locs:?}"
        );
    }
);

// ---------------------------------------------------------------------------
// Component and import definitions
// ---------------------------------------------------------------------------

real_provider_test!(
    definition_component_and_imports,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _mycomp = session.open_fixture_file("src/MyComp.vue").await;
        let _wrapped = session.open_fixture_file("src/WrappedButton.vue").await;
        let _on_event = session.open_fixture_file("src/OnEventPropComp.vue").await;

        if !session.require_or_skip_ready(&uri, "action.disabled", 7, "disabled").await {
            return;
        }

        // B1: <MyComp → MyComp.vue
        let pos = session.find_position(&uri, "<MyComp", 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "<MyComp should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("MyComp.vue"), "<MyComp should go to MyComp.vue, got: {def_path}");
        assert!(!def_path.ends_with(".tsx"), "<MyComp should NOT go to .tsx, got: {def_path}");
        // Should not be same file
        let uri_path = RealProviderTestSession::uri_to_path(&uri);
        assert_ne!(def_path, uri_path, "<MyComp should NOT go to same file");

        // <WrappedButton → WrappedButton.vue
        let pos = session.find_position(&uri, "<WrappedButton", 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "<WrappedButton should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("WrappedButton.vue"), "<WrappedButton should go to WrappedButton.vue, got: {def_path}");
        assert!(!def_path.contains(".vue.tsx"), "should NOT be .vue.tsx, got: {def_path}");
        assert!(!def_path.contains(".vue.ts"), "should NOT be .vue.ts, got: {def_path}");

        // C1: import { formatCount } → utils.ts
        let pos = session.find_position(&uri, "import { formatCount }", 9);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "formatCount import should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("utils.ts"), "formatCount import should go to utils.ts, got: {def_path}");

        // C2: formatCount(count.value) → utils.ts
        let pos = session.find_position(&uri, "formatCount(count.value)", 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "formatCount usage should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("utils.ts"), "formatCount usage should go to utils.ts, got: {def_path}");

        // E1: foo="literal" → MyComp.vue (defineProps)
        let pos = session.find_position(&uri, r#"foo="literal""#, 0);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "foo prop should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("MyComp.vue"), "foo prop should go to MyComp.vue, got: {def_path}");

        // E2: :bar="count" → MyComp.vue (defineProps)
        let pos = session.find_position(&uri, r#":bar="count""#, 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), ":bar prop should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("MyComp.vue"), ":bar prop should go to MyComp.vue, got: {def_path}");

        // E2b: variant="danger" → WrappedButton.vue
        let pos = session.find_position(&uri, r#"variant="danger""#, 0);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "variant prop should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("WrappedButton.vue"), "variant prop should go to WrappedButton.vue, got: {def_path}");

        // G2: @custom handler expression → same file (function)
        let delta = r#"@custom=""#.len();
        let pos = session.find_position(&uri, r#"@custom="handleCustom($event)""#, delta);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "@custom handler should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        let uri_path = RealProviderTestSession::uri_to_path(&uri);
        assert_eq!(def_path, uri_path, "@custom handler should go to same file");

        // G3: @custom event name → MyComp.vue (defineEmits)
        let pos = session.find_position(&uri, r#"@custom="handleCustom($event)""#, 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "@custom event should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("MyComp.vue"), "@custom event should go to MyComp.vue, got: {def_path}");

        // G4: @alert="handleCustom" → OnEventPropComp.vue
        let pos = session.find_position(&uri, r#"@alert="handleCustom""#, 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "@alert should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("OnEventPropComp.vue"), "@alert should go to OnEventPropComp.vue, got: {def_path}");
    }
);

// ---------------------------------------------------------------------------
// Barrel exports fixture
// ---------------------------------------------------------------------------

real_provider_test!(
    definition_barrel_exports,
    fixture = "barrel-exports",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _overlay = session.open_fixture_file("src/components/Overlay.vue").await;
        let _button = session.open_fixture_file("src/components/Button.vue").await;

        if !session.require_or_skip_ready(&uri, "{{ count }}", 3, "count").await {
            return;
        }

        // <Overlay → Overlay.vue (NOT index.ts)
        let pos = session.find_position(&uri, "<Overlay", 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "<Overlay should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("Overlay.vue"), "<Overlay should go to Overlay.vue, got: {def_path}");
        assert!(!def_path.contains("index.ts"), "<Overlay should NOT go to index.ts, got: {def_path}");
        assert!(!def_path.ends_with(".tsx"), "<Overlay should NOT be .tsx, got: {def_path}");

        // <Button → Button.vue (NOT index.ts)
        let pos = session.find_position(&uri, "<Button", 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "<Button should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("Button.vue"), "<Button should go to Button.vue, got: {def_path}");
        assert!(!def_path.contains("index.ts"), "<Button should NOT go to index.ts, got: {def_path}");

        // Import binding: { Overlay, Button } — Overlay
        let pos = session.find_position(&uri, "{ Overlay, Button }", 2);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "Overlay import should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("Overlay.vue"), "Overlay import should go to Overlay.vue, got: {def_path}");
        assert!(!def_path.contains("index.ts"), "Overlay import should NOT go to index.ts, got: {def_path}");

        // Import binding: { Overlay, Button } — Button
        let pos = session.find_position(&uri, "{ Overlay, Button }", 12);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "Button import should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("Button.vue"), "Button import should go to Button.vue, got: {def_path}");
        assert!(!def_path.contains("index.ts"), "Button import should NOT go to index.ts, got: {def_path}");

        // Prop on barrel component: label="Open" → Button.vue
        let pos = session.find_position(&uri, r#"label="Open""#, 0);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "barrel prop should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("Button.vue"), "barrel prop should go to Button.vue, got: {def_path}");
        let uri_path = RealProviderTestSession::uri_to_path(&uri);
        assert_ne!(def_path, uri_path, "barrel prop should NOT go to same file");
    }
);

// ---------------------------------------------------------------------------
// Path aliases fixture
// ---------------------------------------------------------------------------

real_provider_test!(
    definition_path_aliases,
    fixture = "path-aliases",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _mycomp = session.open_fixture_file("src/components/MyComp.vue").await;

        if !session.require_or_skip_ready(&uri, "{{ count }}", 3, "count").await {
            return;
        }

        // <MyComp → MyComp.vue via @/ alias
        let pos = session.find_position(&uri, "<MyComp", 1);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "<MyComp via alias should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("MyComp.vue"), "<MyComp should go to MyComp.vue, got: {def_path}");
        assert!(!def_path.ends_with(".tsx"), "<MyComp should NOT go to .tsx, got: {def_path}");

        // Import binding: import MyComp → MyComp.vue via @/ alias
        let pos = session.find_position(&uri, "import MyComp", 7);
        let locs = session.definition_locations(&uri, pos).await;
        assert!(!locs.is_empty(), "MyComp import should have definitions");
        let def_path = RealProviderTestSession::uri_to_path(&locs[0].uri);
        assert!(def_path.contains("MyComp.vue"), "MyComp import should go to MyComp.vue, got: {def_path}");
    }
);

// ---------------------------------------------------------------------------
// Svelte public-prop definitions
// ---------------------------------------------------------------------------

real_provider_test!(
    definition_svelte_component_prop_lands_on_authored_props_type_member,
    fixture = "svelte-parity",
    async fn run(session) {
        let parent = session
            .open_fixture_file("src/components/PropParent.svelte")
            .await;
        // Keep the imported child closed: this is the production first-open
        // shape. did_open/background publication must make its public surface
        // queryable without a test-only readiness join or an editor open.
        let child = session.workspace_uri("src/components/PropChild.svelte");
        let parent_path = RealProviderTestSession::uri_to_path(&parent);
        let resolved_child = session
            .server()
            .test_documents()
            .host()
            .resolve_import_transient(&parent_path, "./PropChild.svelte");
        assert_eq!(
            resolved_child.as_deref(),
            Some(RealProviderTestSession::uri_to_path(&child).as_str()),
            "the published workspace must resolve the direct Svelte carrier import"
        );

        // An explicit component-prop NAME navigates to the child's authored
        // `$props` member. The shorthand `{contractProp}` token is a value
        // expression and is asserted separately below against its parent-local
        // binding.
        let use_position = session.find_position(&parent, "optionalFlag={true}", 1);
        let declaration = Position::new(6, 4);
        let mut raw_definitions = Vec::new();
        for _ in 0..48 {
            raw_definitions = session
                .raw_provider_definitions(&parent, use_position)
                .await;
            if raw_definitions.iter().any(|definition| {
                definition.path.contains("PropChild.svelte")
            }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert!(
            raw_definitions.iter().any(|definition| {
                definition.path.contains("PropChild.svelte")
            }),
            "the real provider must resolve the generated component-prop use to the closed child carrier before LSP remapping; got={raw_definitions:?}"
        );
        let mut definitions = Vec::new();
        for _ in 0..16 {
            definitions = session.definition_locations(&parent, use_position).await;
            if definitions.iter().any(|location| {
                location.uri == child && location.range.start == declaration
            }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert!(
            definitions.iter().any(|location| {
                location.uri == child && location.range.start == declaration
            }),
            "Svelte component prop definition must map the provider's public projection back to the authored `$props` type member; expected={declaration:?}, child_state={:?}, raw={raw_definitions:?}, got={definitions:?}",
            session.server().test_provider_sync_state_for_canonical(
                &RealProviderTestSession::uri_to_path(&child)
            )
        );
        assert!(
            definitions
                .iter()
                .all(|location| !location.uri.as_str().ends_with(".tsx")),
            "public prop definition must never leak the IDE carrier: {definitions:?}"
        );

        let shorthand_position = session.find_position(&parent, "{contractProp}", 2);
        let shorthand_declaration = Position::new(2, 6);
        let shorthand_definitions = session
            .definition_locations(&parent, shorthand_position)
            .await;
        assert!(
            shorthand_definitions.iter().any(|location| {
                location.uri == parent && location.range.start == shorthand_declaration
            }),
            "a Svelte component shorthand value must navigate to its authored local binding; \
             expected={shorthand_declaration:?}, got={shorthand_definitions:?}"
        );
        assert!(
            shorthand_definitions
                .iter()
                .all(|location| !location.uri.as_str().ends_with(".tsx")),
            "shorthand definition must never leak the IDE carrier: {shorthand_definitions:?}"
        );
    }
);
