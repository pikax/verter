//! Vue declarations through the real declaration stage: `verter-tsc
//! --declaration` over the projection driver publishes each carrier's public
//! contract as `Foo.vue.d.ts`, and a plain TypeScript project that imports
//! the published declarations checks clean against the exact types.
//!
//! Real engine coverage; prerequisites are required, so the gate cannot
//! silently skip: the frozen pnpm install provides `packages/example`'s
//! `vue` and the pinned engine.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oxc_sourcemap::SourceMap;
use verter_tsgo_api::toolchain::discovery::{resolve_blocking, ResolutionRequest};
use verter_tsgo_api::toolchain::validation::Capability;

const OPTIONS: &str = r#""strict":true,"target":"ES2022","module":"ESNext","moduleResolution":"bundler","skipLibCheck":true,"lib":["ES2022","DOM"],"types":[]"#;

/// A carrier whose public types reach a local type, an imported type and a
/// `typeof` dependency; its setup body and a private binding stay private.
const COUNTER: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "import type { Remote } from './types';\n",
    "const limit: 10 = 10;\n",
    "type Local = { remote: Remote; limit: typeof limit };\n",
    "const count: number = 1;\n",
    "const local: Local = { remote: { id: 'a' }, limit };\n",
    "const secret = count * 2;\n",
    "defineExpose({ count, local });\n",
    "</script>\n",
);

struct Fixture {
    dir: tempfile::TempDir,
    engine: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let example = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("packages/example");
        assert!(
            example.join("node_modules/vue").is_dir(),
            "run the frozen pnpm install before the engine tests"
        );
        let request = ResolutionRequest::for_environment(Capability::Api, Some(example.clone()));
        let engine = resolve_blocking(&request)
            .expect("the pinned API engine is required")
            .path;
        let dir = tempfile::Builder::new()
            .prefix("vue-declarations-")
            .tempdir_in(example)
            .unwrap();
        Self { dir, engine }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.path(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name))
            .unwrap_or_else(|_| panic!("{name} is published: {:?}", self.files("")))
    }

    fn files(&self, under: &str) -> Vec<String> {
        let root = self.path(under);
        let mut files = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(
                        path.strip_prefix(&root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        files.sort();
        files
    }

    /// A producer project at `name/`, publishing to `name/dist`.
    fn producer(&self, name: &str, options: &str) {
        self.write(
            &format!("{name}/tsconfig.json"),
            &format!(
                r#"{{"compilerOptions":{{{OPTIONS},"outDir":"dist"{options}}},"include":["src/**/*"]}}"#
            ),
        );
    }

    /// `verter-tsc --declaration` over the projection driver.
    fn declare(&self, name: &str) -> Output {
        Command::new(verter_test_support::cargo_test_binary_path!("verter-tsc"))
            .arg("--projection-route")
            .arg("--declaration")
            .arg("-p")
            .arg(self.path(&format!("{name}/tsconfig.json")))
            .arg("--tsgo-bin")
            .arg(&self.engine)
            .output()
            .expect("run verter-tsc")
    }

    /// The engine alone over a plain TypeScript project.
    fn check(&self, name: &str) -> Output {
        Command::new(&self.engine)
            .arg("-p")
            .arg(self.path(&format!("{name}/tsconfig.json")))
            .output()
            .expect("run the engine")
    }
}

fn text(output: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn published_vue_declarations_carry_their_dependencies_and_a_consumer_checks_clean() {
    let f = Fixture::new();
    f.producer("producer", "");
    f.write(
        "producer/src/types.ts",
        "export interface Remote { id: string }\n",
    );
    f.write("producer/src/Counter.vue", COUNTER);
    let declared = f.declare("producer");
    assert_eq!(declared.status.code(), Some(0), "{}", text(&declared));

    assert_eq!(
        f.files("producer/dist"),
        ["src/Counter.vue.d.ts", "src/types.d.ts"]
    );
    let counter = f.read("producer/dist/src/Counter.vue.d.ts");
    // Local, imported and `typeof` dependencies travel with the declaration.
    assert!(counter.contains("type Local"), "{counter}");
    assert!(counter.contains("./types"), "{counter}");
    assert!(counter.contains("const limit: 10"), "{counter}");
    for private in ["secret", "count * 2", "defineExpose", ".tmp"] {
        assert!(!counter.contains(private), "{private}:\n{counter}");
    }

    // A plain TypeScript consumer, with no Verter and no `*.vue` module,
    // reads the published declaration by the carrier's own name.
    f.write(
        "consumer/tsconfig.json",
        &format!(r#"{{"compilerOptions":{{{OPTIONS},"noEmit":true}},"files":["use.ts"]}}"#),
    );
    f.write(
        "consumer/use.ts",
        concat!(
            "import Counter from '../producer/dist/src/Counter.vue';\n",
            "declare const counter: InstanceType<typeof Counter>;\n",
            "export const id: string = counter.local.remote.id;\n",
            "export const count: number = counter.count;\n",
            "export const limit: 10 = counter.local.limit;\n",
            "// @ts-expect-error the exposed id is a string\n",
            "export const wrong: number = counter.local.remote.id;\n",
            "// @ts-expect-error a binding the component does not expose\n",
            "counter.secret;\n",
        ),
    );
    let checked = f.check("consumer");
    assert!(checked.status.success(), "{}", text(&checked));
}

#[test]
fn a_checking_path_in_the_public_types_publishes_nothing() {
    let f = Fixture::new();
    f.producer("producer", "");
    f.write("producer/src/Other.vue", COUNTER);
    f.write(
        "producer/src/types.ts",
        "export interface Remote { id: string }\n",
    );
    f.write(
        "producer/src/Private.vue",
        concat!(
            "<script setup lang=\"ts\">\n",
            "type Checked = typeof import('./Other.vue.ts');\n",
            "defineProps<{ checked: Checked }>();\n",
            "</script>\n",
        ),
    );
    let declared = f.declare("producer");
    let report = text(&declared);
    assert_ne!(declared.status.code(), Some(0), "{report}");
    assert!(
        report.contains("the public types name the checking path './Other.vue.ts'"),
        "{report}"
    );
    assert!(f.files("producer/dist").is_empty(), "{report}");
}

#[test]
fn published_declaration_maps_lead_to_the_authored_carrier() {
    let f = Fixture::new();
    f.producer("producer", r#","declaration":true,"declarationMap":true"#);
    f.write(
        "producer/src/types.ts",
        "export interface Remote { id: string }\n",
    );
    f.write("producer/src/Counter.vue", COUNTER);
    let declared = f.declare("producer");
    assert_eq!(declared.status.code(), Some(0), "{}", text(&declared));

    let counter = f.read("producer/dist/src/Counter.vue.d.ts");
    assert!(
        counter
            .trim_end()
            .ends_with("//# sourceMappingURL=Counter.vue.d.ts.map"),
        "{counter}"
    );
    let raw = f.read("producer/dist/src/Counter.vue.d.ts.map");
    let map = SourceMap::from_json_string(&raw).unwrap();
    assert_eq!(
        map.get_sources().collect::<Vec<_>>(),
        ["../../src/Counter.vue"]
    );
    // The published `Local` declaration maps to the authored one.
    let (line, column) = position(&counter, "type Local");
    let token = map
        .get_tokens()
        .filter(|token| token.get_dst_line() == line && token.get_dst_col() <= column)
        .last()
        .unwrap_or_else(|| panic!("no mapping for `type Local`:\n{raw}"));
    assert_eq!(
        token.get_src_line(),
        position(COUNTER, "type Local").0,
        "{raw}"
    );
    // The member's map names the member, from where it is published.
    let types = SourceMap::from_json_string(&f.read("producer/dist/src/types.d.ts.map"))
        .unwrap()
        .get_sources()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(types, ["../../src/types.ts"]);
}

#[test]
fn one_unpublishable_carrier_keeps_the_previous_output() {
    let f = Fixture::new();
    f.producer("producer", "");
    f.write("producer/dist/keep.d.ts", "export {};\n");
    f.write(
        "producer/src/types.ts",
        "export interface Remote { id: string }\n",
    );
    f.write("producer/src/Counter.vue", COUNTER);
    // The public surface names a binding the declaration cannot carry.
    f.write(
        "producer/src/Status.vue",
        concat!(
            "<script setup lang=\"ts\">\n",
            "const status: number = 1;\n",
            "defineExpose({ read: () => status });\n",
            "</script>\n",
        ),
    );
    let declared = f.declare("producer");
    let report = text(&declared);
    assert_ne!(declared.status.code(), Some(0), "{report}");
    assert!(
        report.contains("the public types name 'status'"),
        "{report}"
    );
    assert_eq!(f.files("producer/dist"), ["keep.d.ts"], "{report}");
}

/// The zero-based line and column of `needle` in `text`, for ASCII text.
fn position(text: &str, needle: &str) -> (u32, u32) {
    let offset = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle}:\n{text}"));
    let line = text[..offset].matches('\n').count() as u32;
    let column = (offset - text[..offset].rfind('\n').map_or(0, |start| start + 1)) as u32;
    (line, column)
}
