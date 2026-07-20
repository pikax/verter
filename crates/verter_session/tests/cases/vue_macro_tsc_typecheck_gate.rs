//! End-to-end Vue macro TSC validity under the TypeScript >= 7 engine.
//!
//! Parser-only assertions cannot prove that the generated Public, Testing, and
//! Declaration carriers are valid TypeScript. This gate drives a typed Vue SFC
//! through the public host API, writes all three outputs into one hermetic
//! project, and invokes the pinned TypeScript launcher for a real type check.
//!
//! The checker is optional on ordinary local machines. It is mandatory when
//! `CI` or `VERTER_REQUIRE_TYPECHECKER` is truthy; silently skipping there would
//! mask a broken carrier. The Vue package contract is checked in under
//! `tests/fixtures/vue_macro_tsc_typecheck_gate` and copied into the temporary
//! project, so the test never depends on an installed Vue package or network.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use verter_session::{
    CompileErrorPolicy, DependencyResolution, FileLanguage, HostConfig, PublicApiMode, TscResponse,
    UpsertRequest, VerterHost,
};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vue_macro_tsc_typecheck_gate")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate is <workspace>/crates/verter_session")
        .to_path_buf()
}

fn require_type_checker() -> bool {
    fn truthy(name: &str) -> bool {
        std::env::var_os(name).is_some_and(|value| {
            let value = value.to_string_lossy();
            let value = value.trim();
            !value.is_empty()
                && !value.eq_ignore_ascii_case("0")
                && !value.eq_ignore_ascii_case("false")
        })
    }

    truthy("CI") || truthy("VERTER_REQUIRE_TYPECHECKER")
}

fn typescript_package_major(tsc_js: &Path) -> Option<u32> {
    let package_json = tsc_js.parent()?.parent()?.join("package.json");
    let contents = std::fs::read_to_string(package_json).ok()?;
    let version_key = contents.find("\"version\"")?;
    let after_key = &contents[version_key..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let quote_start = after_colon.find('"')? + 1;
    let value = &after_colon[quote_start..];
    let quote_end = value.find('"')?;
    value[..quote_end].split('.').next()?.parse().ok()
}

fn assert_ts7_launcher(tsc_js: PathBuf) -> PathBuf {
    let major = typescript_package_major(&tsc_js);
    assert!(
        major.is_some_and(|major| major >= 7),
        "the Vue macro TSC gate found a non-TS>=7 launcher (major={major:?}) at {}; \
         install the pinned TypeScript package instead of checking with a legacy engine",
        tsc_js.display(),
    );
    tsc_js
}

/// Locate the pinned TypeScript >= 7 launcher using the same policy as the
/// established Svelte typecheck gate. The JavaScript launcher resolves the
/// platform-specific typescript-go binary and is invoked through `node`, never
/// through an OS-specific `.bin/tsc` wrapper.
fn locate_type_checker() -> Option<PathBuf> {
    let node_modules = workspace_root().join("node_modules");
    let hoisted = node_modules.join("typescript/lib/tsc.js");
    if hoisted.is_file() {
        return Some(assert_ts7_launcher(hoisted));
    }

    let pnpm_dir = node_modules.join(".pnpm");
    if let Ok(entries) = std::fs::read_dir(&pnpm_dir) {
        let mut candidates = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_str()?;
                if !name.starts_with("typescript@") || name.contains('+') {
                    return None;
                }
                let launcher = entry.path().join("node_modules/typescript/lib/tsc.js");
                launcher.is_file().then_some(launcher)
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            typescript_package_major(left)
                .cmp(&typescript_package_major(right))
                .then_with(|| left.cmp(right))
        });
        if let Some(launcher) = candidates.pop() {
            return Some(assert_ts7_launcher(launcher));
        }
    }

    assert!(
        !require_type_checker(),
        "the Vue macro TSC gate requires TypeScript >= 7 because CI or \
         VERTER_REQUIRE_TYPECHECKER is set, but `typescript/lib/tsc.js` was not found under {}; \
         run `pnpm install` or unset the local opt-in",
        node_modules.display(),
    );
    None
}

fn install_vue_contract(project_root: &Path) {
    let destination = project_root.join("node_modules/vue");
    std::fs::create_dir_all(&destination).expect("create hermetic Vue package directory");
    for file in ["package.json", "index.d.ts"] {
        std::fs::copy(fixture_dir().join("vue").join(file), destination.join(file))
            .unwrap_or_else(|error| panic!("copy hermetic Vue contract {file}: {error}"));
    }
}

fn typecheck_project(launcher: &Path, files: &[(&str, &str)]) -> (bool, String) {
    let project = tempfile::tempdir().expect("create TypeScript checker project");
    install_vue_contract(project.path());

    for (relative, contents) in files {
        let path = project.path().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create checker fixture parent");
        }
        std::fs::write(path, contents).expect("write checker fixture");
    }

    std::fs::write(
        project.path().join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "module": "esnext",
    "target": "esnext",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": false,
    "types": [],
    "lib": ["esnext"]
  },
  "include": ["**/*.ts", "**/*.tsx"]
}"#,
    )
    .expect("write checker tsconfig");

    let output = Command::new("node")
        .arg(launcher)
        .arg("--noEmit")
        .arg("-p")
        .arg(project.path().join("tsconfig.json"))
        .current_dir(project.path())
        .output()
        .expect("run TypeScript >= 7 checker");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    (output.status.success(), diagnostics)
}

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_owned()),
            input_id: canonical.to_owned(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert Vue macro checker fixture");
}

fn upsert_typescript(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_owned()),
            input_id: canonical.to_owned(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert TypeScript dependency for Vue macro checker fixture");
}

fn require_projection(
    host: &VerterHost,
    canonical: &str,
    mode: PublicApiMode,
    fixture: &str,
) -> TscResponse {
    let outer = host.get_public_api_with_mode(canonical, mode, None);
    let inner = match outer {
        Ok(inner) => inner,
        Err(error) => panic!(
            "{fixture} {mode:?}: expected outer Ok from public API projection, got \
             Err(code={}, detail={}, subject={}): {error:?}",
            error.code(),
            error.detail_code(),
            error.subject(),
        ),
    };
    match inner {
        Some(response) => response,
        None => panic!(
            "{fixture} {mode:?}: public API returned outer Ok but inner None for a loaded Vue carrier"
        ),
    }
}

fn skip_note(name: &str) {
    eprintln!(
        "SKIP {name}: TypeScript >= 7 is not installed; set \
         VERTER_REQUIRE_TYPECHECKER=1 on a machine with `pnpm install` to require the gate"
    );
}

const COMPONENT_SOURCE: &str = r#"<script setup lang="ts">
import { importedValue, ImportedBase } from './external'
interface Base<T> {}
class Payload<T extends string> extends ImportedBase implements Base<T> {
  readonly literal = 1
  value = 1
  // Keep the authored fixture valid: Verter preserves Testing-mode class bodies.
  constructor(public id?: number, protected name = "x") { super() }
  method(input = 1) { return input }
}
enum Kind { Zero, Two = Zero + 2, Text = "text" }
type Props = {
  payload: Payload<"x">
  kind: Kind
  kindCtor: typeof Kind
  imported: typeof importedValue
  label?: string
}
const props = defineProps<Props>()
const emit = defineEmits<{
  save: [payload: Payload<"x">]
  choose: [kind: Kind]
}>()
const model = defineModel<Payload<"x">>("modelValue")
</script>
<template><div /></template>"#;

const EXTERNAL_SOURCE: &str = r#"export const importedValue = { code: 'external' } as const
export class ImportedBase { base = 1 }
"#;

const CONSUMER: &str = r#"import PublicComponent from './macro-public'
import TestingComponent from './macro-testing'
import DeclarationComponent from './macro-declaration'

type IsAny<T> = 0 extends (1 & T) ? true : false
type IsExactly<Actual, Expected> = IsAny<Actual> extends true
  ? false
  : [Actual] extends [Expected]
    ? [Expected] extends [Actual]
      ? true
      : false
    : false
type Expect<Condition extends true> = Condition
type PublicInstance = InstanceType<typeof PublicComponent>
type TestingInstance = InstanceType<typeof TestingComponent>
type DeclarationInstance = InstanceType<typeof DeclarationComponent>

declare const publicInstance: PublicInstance
declare const testingInstance: TestingInstance
declare const declarationInstance: DeclarationInstance

const publicPayload = publicInstance.$props.payload
const publicPayloadNotAny: IsAny<typeof publicPayload> = false
type PublicValueExact = Expect<IsExactly<typeof publicPayload.value, number>>
type PublicLiteralExact = Expect<IsExactly<typeof publicPayload.literal, 1>>
type PublicMethodParameterExact = Expect<
  IsExactly<Parameters<typeof publicPayload.method>, [input?: number]>
>
type PublicMethodReturnExact = Expect<
  IsExactly<ReturnType<typeof publicPayload.method>, number>
>
type PublicIdExact = Expect<IsExactly<typeof publicPayload.id, number | undefined>>
type PublicImportedCodeExact = Expect<
  IsExactly<typeof publicInstance.$props.imported.code, 'external'>
>
type PublicKindCtorKeysExact = Expect<
  IsExactly<keyof typeof publicInstance.$props.kindCtor, 'Zero' | 'Two' | 'Text'>
>
publicPayload.value = 2
const publicLiteral: 1 = publicPayload.literal
const publicMethodResult: number = publicPayload.method(2)
const publicId: number | undefined = publicPayload.id
const publicBase: number = publicPayload.base
const publicImportedCode: 'external' = publicInstance.$props.imported.code
// Kind is heterogeneous because Text is a string member; its primitive domain is string | number.
const publicKindAsPrimitive: string | number = publicInstance.$props.kind
const publicKindFromCtor: typeof publicInstance.$props.kind =
  publicInstance.$props.kindCtor.Two
const publicKindNotAny: IsAny<typeof publicInstance.$props.kind> = false
publicInstance.$props.onSave?.(publicPayload)
publicInstance.$emit('save', publicPayload)
publicInstance.$emit('choose', publicInstance.$props.kind)
publicInstance.$emit('update:modelValue', publicPayload)
// @ts-expect-error readonly class fields stay readonly
publicPayload.literal = 2
// @ts-expect-error protected parameter properties stay protected
publicPayload.name
// @ts-expect-error enum props do not accept arbitrary strings
const publicBadKind: typeof publicInstance.$props.kind = 'wrong'
// @ts-expect-error save retains its exact payload
publicInstance.$emit('save', 'wrong')
// @ts-expect-error undeclared events are rejected
publicInstance.$emit('missing')
// @ts-expect-error Public mode excludes setup bindings
publicInstance.props
// @ts-expect-error Public mode excludes the setup emit binding
publicInstance.emit
// @ts-expect-error Public mode excludes the setup model binding
publicInstance.model

const testingPayload = testingInstance.$props.payload
const testingPayloadNotAny: IsAny<typeof testingPayload> = false
type TestingValueExact = Expect<IsExactly<typeof testingPayload.value, number>>
type TestingLiteralExact = Expect<IsExactly<typeof testingPayload.literal, 1>>
type TestingMethodParameterExact = Expect<
  IsExactly<Parameters<typeof testingPayload.method>, [input?: number]>
>
type TestingMethodReturnExact = Expect<
  IsExactly<ReturnType<typeof testingPayload.method>, number>
>
type TestingIdExact = Expect<IsExactly<typeof testingPayload.id, number | undefined>>
type TestingImportedCodeExact = Expect<
  IsExactly<typeof testingInstance.$props.imported.code, 'external'>
>
type TestingKindCtorKeysExact = Expect<
  IsExactly<keyof typeof testingInstance.$props.kindCtor, 'Zero' | 'Two' | 'Text'>
>
testingPayload.value = 2
const testingLiteral: 1 = testingPayload.literal
const testingMethodResult: number = testingPayload.method(2)
const testingId: number | undefined = testingPayload.id
const testingBase: number = testingPayload.base
const testingImportedCode: 'external' = testingInstance.$props.imported.code
const testingKindAsPrimitive: string | number = testingInstance.$props.kind
const testingKindFromCtor: typeof testingInstance.$props.kind =
  testingInstance.$props.kindCtor.Two
const testingKindNotAny: IsAny<typeof testingInstance.$props.kind> = false
testingInstance.$props.onSave?.(testingPayload)
testingInstance.$emit('save', testingPayload)
testingInstance.$emit('choose', testingInstance.$props.kind)
testingInstance.$emit('update:modelValue', testingPayload)
const testingBindingValue: number = testingInstance.props.payload.value
testingInstance.emit('save', testingInstance.props.payload)
const testingModelValue: number | undefined = testingInstance.model?.value
// @ts-expect-error readonly class fields stay readonly in Testing mode
testingPayload.literal = 2
// @ts-expect-error protected parameter properties stay protected in Testing mode
testingPayload.name
// @ts-expect-error Testing $emit retains its exact payload
testingInstance.$emit('save', 'wrong')
// @ts-expect-error undeclared Testing events are rejected
testingInstance.$emit('missing')

const declarationPayload = declarationInstance.$props.payload
const declarationPayloadNotAny: IsAny<typeof declarationPayload> = false
type DeclarationValueExact = Expect<IsExactly<typeof declarationPayload.value, number>>
type DeclarationLiteralExact = Expect<IsExactly<typeof declarationPayload.literal, 1>>
type DeclarationMethodParameterExact = Expect<
  IsExactly<Parameters<typeof declarationPayload.method>, [input?: number]>
>
type DeclarationMethodReturnExact = Expect<
  IsExactly<ReturnType<typeof declarationPayload.method>, number>
>
type DeclarationIdExact = Expect<
  IsExactly<typeof declarationPayload.id, number | undefined>
>
type DeclarationImportedCodeExact = Expect<
  IsExactly<typeof declarationInstance.$props.imported.code, 'external'>
>
type DeclarationKindCtorKeysExact = Expect<
  IsExactly<keyof typeof declarationInstance.$props.kindCtor, 'Zero' | 'Two' | 'Text'>
>
declarationPayload.value = 2
const declarationLiteral: 1 = declarationPayload.literal
const declarationMethodResult: number = declarationPayload.method(2)
const declarationId: number | undefined = declarationPayload.id
const declarationBase: number = declarationPayload.base
const declarationImportedCode: 'external' = declarationInstance.$props.imported.code
const declarationKindAsPrimitive: string | number = declarationInstance.$props.kind
const declarationKindFromCtor: typeof declarationInstance.$props.kind =
  declarationInstance.$props.kindCtor.Two
const declarationKindNotAny: IsAny<typeof declarationInstance.$props.kind> = false
declarationInstance.$props.onSave?.(declarationPayload)
declarationInstance.$emit('save', declarationPayload)
declarationInstance.$emit('choose', declarationInstance.$props.kind)
declarationInstance.$emit('update:modelValue', declarationPayload)
// @ts-expect-error readonly class fields stay readonly in Declaration mode
declarationPayload.literal = 2
// @ts-expect-error protected parameter properties stay protected in Declaration mode
declarationPayload.name
// @ts-expect-error Declaration $emit retains its exact payload
declarationInstance.$emit('save', 'wrong')
// @ts-expect-error undeclared Declaration events are rejected
declarationInstance.$emit('missing')
// @ts-expect-error Declaration mode excludes setup bindings
declarationInstance.props
// @ts-expect-error Declaration mode excludes the setup emit binding
declarationInstance.emit
// @ts-expect-error Declaration mode excludes the setup model binding
declarationInstance.model

void publicPayloadNotAny
void publicLiteral
void publicMethodResult
void publicId
void publicBase
void publicImportedCode
void publicKindAsPrimitive
void publicKindFromCtor
void publicKindNotAny
void publicBadKind
void testingPayloadNotAny
void testingLiteral
void testingMethodResult
void testingId
void testingBase
void testingImportedCode
void testingKindAsPrimitive
void testingKindFromCtor
void testingKindNotAny
void testingBindingValue
void testingModelValue
void declarationPayloadNotAny
void declarationLiteral
void declarationMethodResult
void declarationId
void declarationBase
void declarationImportedCode
void declarationKindAsPrimitive
void declarationKindFromCtor
void declarationKindNotAny

type _ExactMemberWitnesses = [
  PublicValueExact,
  PublicLiteralExact,
  PublicMethodParameterExact,
  PublicMethodReturnExact,
  PublicIdExact,
  PublicImportedCodeExact,
  PublicKindCtorKeysExact,
  TestingValueExact,
  TestingLiteralExact,
  TestingMethodParameterExact,
  TestingMethodReturnExact,
  TestingIdExact,
  TestingImportedCodeExact,
  TestingKindCtorKeysExact,
  DeclarationValueExact,
  DeclarationLiteralExact,
  DeclarationMethodParameterExact,
  DeclarationMethodReturnExact,
  DeclarationIdExact,
  DeclarationImportedCodeExact,
  DeclarationKindCtorKeysExact,
]
"#;

const OWNER_SCOPE_COMPONENT_SOURCE: &str = r#"<script lang="ts">
import { companionSharedValue as SharedValue, companionFallbackValue } from './owner-companion'
import * as OwnerNS from './owner-companion'
import * as CompanionNS from './owner-companion'
interface SharedDecl { companionMarker: string }
interface CompanionDecl { companionOnly: boolean }
class SharedClass { companionClass = "wrong" }
class CompanionClass { companionClass = 1 }
enum SharedEnum { Companion = 9 }
enum CompanionEnum { Companion = 3 }
</script>
<script setup lang="ts">
import { setupSharedValue as SharedValue, SetupBase } from './owner-setup'
import * as OwnerNS from './owner-setup'
interface SharedDecl { setupMarker: number }
class SharedClass extends SetupBase { setupClass = 1 }
enum SharedEnum { Setup = 1 }
type Props = {
  sharedDecl: SharedDecl
  sharedClass: SharedClass
  sharedEnum: SharedEnum
  sharedEnumCtor: typeof SharedEnum
  sharedValue: typeof SharedValue
  qualifiedSetup: OwnerNS.SetupQualified
  companionDecl: CompanionDecl
  companionClass: CompanionClass
  companionEnum: CompanionEnum
  companionEnumCtor: typeof CompanionEnum
  companionValue: typeof companionFallbackValue
  qualifiedCompanion: CompanionNS.CompanionQualified
}
defineProps<Props>()
</script>
<template><div /></template>"#;

const OWNER_SETUP_EXTERNAL: &str = r#"export const setupSharedValue = { owner: 'setup' } as const
export interface SetupQualified { setupQualified: 1 }
export class SetupBase { setupBase = 1 }
"#;

const OWNER_COMPANION_EXTERNAL: &str = r#"export const companionSharedValue = { owner: 'companion' } as const
export const companionFallbackValue = { fallback: 'companion' } as const
export interface CompanionQualified { companionQualified: 1 }
"#;

const OWNER_SCOPE_CONSUMER: &str = r#"import PublicComponent from './owner-public'
import TestingComponent from './owner-testing'
import DeclarationComponent from './owner-declaration'

type IsAny<T> = 0 extends (1 & T) ? true : false
type IsExactly<Actual, Expected> = IsAny<Actual> extends true
  ? false
  : [Actual] extends [Expected]
    ? [Expected] extends [Actual]
      ? true
      : false
    : false
type Expect<Condition extends true> = Condition
type At<Value, Key extends PropertyKey> = Key extends keyof Value ? Value[Key] : never
type OwnerFacts<Props> = [
  IsExactly<keyof At<Props, 'sharedDecl'>, 'setupMarker'>,
  IsExactly<keyof At<Props, 'sharedClass'>, 'setupBase' | 'setupClass'>,
  IsExactly<keyof At<Props, 'sharedEnumCtor'>, 'Setup'>,
  IsExactly<At<At<Props, 'sharedValue'>, 'owner'>, 'setup'>,
  IsExactly<At<At<Props, 'qualifiedSetup'>, 'setupQualified'>, 1>,
  IsExactly<keyof At<Props, 'companionDecl'>, 'companionOnly'>,
  IsExactly<keyof At<Props, 'companionClass'>, 'companionClass'>,
  IsExactly<keyof At<Props, 'companionEnumCtor'>, 'Companion'>,
  IsExactly<At<At<Props, 'companionValue'>, 'fallback'>, 'companion'>,
  IsExactly<At<At<Props, 'qualifiedCompanion'>, 'companionQualified'>, 1>,
]
type ExpectedOwnerFacts = [true, true, true, true, true, true, true, true, true, true]

type PublicInstance = InstanceType<typeof PublicComponent>
type TestingInstance = InstanceType<typeof TestingComponent>
type DeclarationInstance = InstanceType<typeof DeclarationComponent>
type PublicOwnerFactsExact = Expect<
  IsExactly<OwnerFacts<PublicInstance['$props']>, ExpectedOwnerFacts>
>
type TestingOwnerFactsExact = Expect<
  IsExactly<OwnerFacts<TestingInstance['$props']>, ExpectedOwnerFacts>
>
type DeclarationOwnerFactsExact = Expect<
  IsExactly<OwnerFacts<DeclarationInstance['$props']>, ExpectedOwnerFacts>
>

declare const publicInstance: PublicInstance
declare const testingInstance: TestingInstance
declare const declarationInstance: DeclarationInstance
const publicSharedEnum: typeof publicInstance.$props.sharedEnum =
  publicInstance.$props.sharedEnumCtor.Setup
const publicCompanionEnum: typeof publicInstance.$props.companionEnum =
  publicInstance.$props.companionEnumCtor.Companion
const testingSharedEnum: typeof testingInstance.$props.sharedEnum =
  testingInstance.$props.sharedEnumCtor.Setup
const testingCompanionEnum: typeof testingInstance.$props.companionEnum =
  testingInstance.$props.companionEnumCtor.Companion
const declarationSharedEnum: typeof declarationInstance.$props.sharedEnum =
  declarationInstance.$props.sharedEnumCtor.Setup
const declarationCompanionEnum: typeof declarationInstance.$props.companionEnum =
  declarationInstance.$props.companionEnumCtor.Companion

void publicSharedEnum
void publicCompanionEnum
void testingSharedEnum
void testingCompanionEnum
void declarationSharedEnum
void declarationCompanionEnum
type _OwnerFactsWitnesses = [
  PublicOwnerFactsExact,
  TestingOwnerFactsExact,
  DeclarationOwnerFactsExact,
]
"#;

const OWNER_VALUE_FAILURE_SOURCE: &str = r#"<script lang="ts">
const seed = 'companion'
interface Props { companionMarker: string }
</script>
<script setup lang="ts">
const seed = 1
type Props = { seed: typeof seed }
defineProps<Props>()
</script>"#;

#[test]
fn checker_controls_accept_valid_and_reject_invalid_typescript() {
    let Some(checker) = locate_type_checker() else {
        skip_note("Vue macro TS>=7 checker controls");
        return;
    };

    let (valid, valid_diagnostics) = typecheck_project(
        &checker,
        &[(
            "control-good.ts",
            "const value: string = 'ready'; void value;\n",
        )],
    );
    assert!(
        valid,
        "known-good checker control failed:\n{valid_diagnostics}"
    );

    let (invalid, invalid_diagnostics) = typecheck_project(
        &checker,
        &[("control-bad.ts", "const value: string = 1;\n")],
    );
    assert!(
        !invalid && invalid_diagnostics.contains("TS2322"),
        "the deliberately-invalid checker control must fail with TS2322:\n{invalid_diagnostics}",
    );
}

#[test]
fn host_owner_scopes_select_setup_shadow_and_companion_fallback_with_ts7() {
    let Some(checker) = locate_type_checker() else {
        skip_note("Vue macro owner-scoped carrier matrix");
        return;
    };

    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });
    const CANONICAL: &str = "/src/OwnerMatrix.vue";
    const SETUP_EXTERNAL: &str = "/src/owner-setup.ts";
    const COMPANION_EXTERNAL: &str = "/src/owner-companion.ts";
    upsert_typescript(&host, SETUP_EXTERNAL, OWNER_SETUP_EXTERNAL);
    upsert_typescript(&host, COMPANION_EXTERNAL, OWNER_COMPANION_EXTERNAL);
    upsert_vue(&host, CANONICAL, OWNER_SCOPE_COMPONENT_SOURCE);
    host.set_import_dependencies(
        CANONICAL,
        vec![
            DependencyResolution {
                specifier: "./owner-setup".to_owned(),
                resolved_canonical_id: Some(SETUP_EXTERNAL.to_owned()),
                possible_canonical_ids: Vec::new(),
            },
            DependencyResolution {
                specifier: "./owner-companion".to_owned(),
                resolved_canonical_id: Some(COMPANION_EXTERNAL.to_owned()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let public = require_projection(
        &host,
        CANONICAL,
        PublicApiMode::Public,
        "owner-scope matrix",
    );
    let testing = require_projection(
        &host,
        CANONICAL,
        PublicApiMode::Testing,
        "owner-scope matrix",
    );
    let declaration = require_projection(
        &host,
        CANONICAL,
        PublicApiMode::Declaration,
        "owner-scope matrix",
    );

    for (mode, code) in [
        ("Public", public.code.as_ref()),
        ("Testing", testing.code.as_ref()),
        ("Declaration", declaration.code.as_ref()),
    ] {
        assert!(
            code.contains("setupMarker: number"),
            "{mode} must select setup-local declaration ordinal 0:\n{code}",
        );
        assert!(
            !code.contains("companionMarker: string"),
            "{mode} must not cross-join companion declaration ordinal 0:\n{code}",
        );
        assert!(
            !code
                .lines()
                .any(|line| line.contains("companionSharedValue")),
            "{mode} must not retain the shadowed companion import:\n{code}",
        );
        assert!(
            !code.lines().any(|line| {
                line.contains("* as OwnerNS") && line.contains("'./owner-companion'")
            }),
            "{mode} must select the setup-local namespace identity:\n{code}",
        );
        for required in [
            "companionOnly: boolean",
            "companionClass",
            "enum CompanionEnum",
            "companionFallbackValue",
            "* as CompanionNS",
        ] {
            assert!(
                code.contains(required),
                "{mode} must retain companion fallback `{required}`:\n{code}",
            );
        }
    }

    let (success, diagnostics) = typecheck_project(
        &checker,
        &[
            ("owner-public.ts", public.code.as_ref()),
            ("owner-testing.ts", testing.code.as_ref()),
            ("owner-declaration.d.ts", declaration.code.as_ref()),
            ("owner-setup.ts", OWNER_SETUP_EXTERNAL),
            ("owner-companion.ts", OWNER_COMPANION_EXTERNAL),
            ("owner-consumer.ts", OWNER_SCOPE_CONSUMER),
        ],
    );
    assert!(
        success,
        "owner-scoped Vue macro carriers failed TypeScript >= 7:\n{diagnostics}\n\
         --- Public ---\n{}\n--- Testing ---\n{}\n--- Declaration ---\n{}",
        public.code, testing.code, declaration.code,
    );
}

#[test]
fn public_result_reports_owner_body_value_dependency_as_structured_failure() {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });
    const CANONICAL: &str = "/src/OwnerValueFailure.vue";
    upsert_vue(&host, CANONICAL, OWNER_VALUE_FAILURE_SOURCE);

    let testing = require_projection(
        &host,
        CANONICAL,
        PublicApiMode::Testing,
        "owner-only value dependency",
    );
    assert!(testing.code.contains("type Props = { seed: typeof seed }"));
    assert!(!testing.code.contains("companionMarker: string"));

    for mode in [PublicApiMode::Public, PublicApiMode::Declaration] {
        let outer = host.get_public_api_with_mode(CANONICAL, mode, None);
        let error = match outer {
            Err(error) => error,
            Ok(Some(response)) => panic!(
                "owner-only value dependency {mode:?}: expected structured outer Err, got Some:\n{}",
                response.code,
            ),
            Ok(None) => panic!(
                "owner-only value dependency {mode:?}: expected structured outer Err, got inner None"
            ),
        };
        assert_eq!(error.code(), "tsc-generation", "mode={mode:?}");
        assert_eq!(
            error.subject(),
            verter_session::PublicApiProjectionSubject::Macro { syntax_index: 0 },
            "mode={mode:?}"
        );
        assert_eq!(
            error.declaration_shape_reason(),
            Some(verter_compiler::tsc::TscDeclarationShapeReason::OwnerValueDependencyUnavailable),
            "mode={mode:?}, error={error:?}",
        );
    }
}

/// @ai-generated - Proves the authoritative TypeInfo Vue-macro handoff produces
/// checker-valid Public, Testing, and Declaration carriers through the public host API.
///
/// Mutation recipes:
/// - pass `MacroTscInput::NotRequired` at the host/compiler join: the typed fixture
///   returns no projection and this test fails before checker invocation;
/// - widen an inferred class member/default parameter to `any`, or retain literal
///   `1` for mutable/defaulted positions: the anti-any witnesses or calls fail;
/// - route Declaration through the Public renderer: executable declarations in
///   `macro-declaration.d.ts` fail under the real checker.
#[test]
fn host_vue_macro_outputs_typecheck_in_all_modes_with_ts7() {
    let Some(checker) = locate_type_checker() else {
        skip_note("Vue macro Public/Testing/Declaration carriers");
        return;
    };

    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });
    const CANONICAL: &str = "/src/MacroMatrix.vue";
    const EXTERNAL_CANONICAL: &str = "/src/external.ts";
    upsert_typescript(&host, EXTERNAL_CANONICAL, EXTERNAL_SOURCE);
    upsert_vue(&host, CANONICAL, COMPONENT_SOURCE);
    host.set_import_dependencies(
        CANONICAL,
        vec![DependencyResolution {
            specifier: "./external".to_owned(),
            resolved_canonical_id: Some(EXTERNAL_CANONICAL.to_owned()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let public = require_projection(
        &host,
        CANONICAL,
        PublicApiMode::Public,
        "macro class/enum matrix",
    );
    let testing = require_projection(
        &host,
        CANONICAL,
        PublicApiMode::Testing,
        "macro class/enum matrix",
    );
    let declaration = require_projection(
        &host,
        CANONICAL,
        PublicApiMode::Declaration,
        "macro class/enum matrix",
    );

    assert!(public.code.contains("declare class Payload"));
    assert!(public.code.contains("declare enum Kind"));
    assert!(public.code.contains("const __comp = defineComponent"));
    assert!(testing.code.contains("class Payload<T extends string>"));
    assert!(testing.code.contains("enum Kind"));
    assert!(testing.code.contains("const props = defineProps<Props>()"));
    assert!(declaration.code.contains("declare class Payload"));
    assert!(declaration.code.contains("declare enum Kind"));
    assert!(!declaration.code.contains("const __comp"));
    assert!(!declaration.code.contains("defineComponent("));
    for (mode, code) in [
        ("Public", public.code.as_ref()),
        ("Declaration", declaration.code.as_ref()),
    ] {
        for binding in ["importedValue", "ImportedBase"] {
            assert!(
                code.lines().any(|line| {
                    line.starts_with("import ")
                        && !line.starts_with("import type ")
                        && line.contains(binding)
                        && line.contains("'./external'")
                }),
                "{mode} must retain value-capable import `{binding}` for `typeof`/heritage:\n{code}",
            );
        }
    }

    let (success, diagnostics) = typecheck_project(
        &checker,
        &[
            ("macro-public.ts", public.code.as_ref()),
            ("macro-testing.ts", testing.code.as_ref()),
            ("macro-declaration.d.ts", declaration.code.as_ref()),
            ("external.ts", EXTERNAL_SOURCE),
            ("consumer.ts", CONSUMER),
        ],
    );
    assert!(
        success,
        "host-generated Vue macro carriers failed TypeScript >= 7:\n{diagnostics}\n\
         --- Public ---\n{}\n--- Testing ---\n{}\n--- Declaration ---\n{}",
        public.code, testing.code, declaration.code,
    );
}
