//! Go-to-definition tests ported from E2E suite.

use crate::test_harness::{real_provider_test, RealProviderTestSession};

// ---------------------------------------------------------------------------
// Same-file definitions (script bindings used in template)
// ---------------------------------------------------------------------------

real_provider_test!(
    definition_same_file,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
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

        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
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

        // B1b: <WrappedButton → WrappedButton.vue
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

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
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

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
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
