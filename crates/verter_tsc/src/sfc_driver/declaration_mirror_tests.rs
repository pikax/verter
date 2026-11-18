use super::*;

const API: &str = "import { t } from '@/types'; const x: number = 'a';\nimport { a } from './util';\nimport { b } from '../scripts/x';\nimport { ref } from 'vue';\nimport { d } from 'dep';\nimport Child from '@/Child.vue';\nexport const read = (c: InstanceType<typeof Child>) => c.count;\n";

const LEGACY: &str =
    "/// <reference path=\"./ambient.d.ts\" />\n/// <reference path=\"./util.ts\" />\nexport {};\n";

struct Project {
    dir: tempfile::TempDir,
    root: String,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = collapse_path(&slashed(dir.path()));
        Self { dir, root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        PathBuf::from(format!("{}/{relative}", self.root))
    }

    /// The fixture's resolver: `@/` names `src/`; `dep` is installed beside
    /// the authored sources only, so the mirror's directories miss it.
    fn resolve(&self, importer: &Path, specifier: &str) -> Option<String> {
        let staged = slashed(importer).contains("/mirror/");
        let target = match specifier {
            "@/types" => "src/types.ts",
            "./util" => "src/util.ts",
            "../scripts/x" => "scripts/x.ts",
            "vue" => "node_modules/vue/dist/vue.d.ts",
            "dep" if staged => return None,
            "dep" => "src/node_modules/dep/index.d.ts",
            // A carrier without a staged product.
            "@/Child.vue" => "src/Child.vue",
            _ => return None,
        };
        Some(format!("{}/{target}", self.root))
    }

    fn program(&self) -> DeclarationMirror {
        let mut program = MirrorBuilder::new(Path::new(&self.root)).unwrap();
        program.member(&self.path("src/api.ts"), API);
        program.member(&self.path("src/util.ts"), "export const a = 1;\n");
        program.member(&self.path("src/types.ts"), "export const t = 1;\n");
        // Read where it is: outside the project, no declaration of its own,
        // a declaration file.
        program.member(Path::new("/elsewhere/shared.ts"), "export {};\n");
        program.member(&self.path("src/data.json"), "{}");
        program.list(&self.path("src/env.d.ts"));
        // Staged; the files it references by path are named where they are
        // read.
        program.member(&self.path("src/legacy.ts"), LEGACY);
        program
            .bind(|importer, specifier| self.resolve(importer, specifier))
            .unwrap()
    }
}

fn api(program: &DeclarationMirror) -> &MirroredFile {
    program
        .files
        .iter()
        .find(|file| slashed(&file.authored).ends_with("/src/api.ts"))
        .unwrap()
}

#[test]
fn staged_members_keep_relative_imports_of_staged_files_and_bind_the_rest() {
    let project = Project::new();
    let program = project.program();
    assert_eq!(program.files.len(), 4);
    assert_eq!(
        program.listed,
        [
            PathBuf::from("/elsewhere/shared.ts"),
            project.path("src/data.json"),
            project.path("src/env.d.ts"),
        ]
    );
    let legacy = program
        .files
        .iter()
        .find(|file| slashed(&file.authored).ends_with("/src/legacy.ts"))
        .unwrap();
    // A declaration file is read where it is; a staged file in the mirror.
    assert_eq!(
        legacy.text,
        LEGACY.replace(
            "\"./ambient.d.ts\"",
            &format!("\"{}/src/ambient.d.ts\"", project.root)
        )
    );
    let api = api(&program);
    assert_eq!(api.module, Path::new("src/api.ts"));
    assert_eq!(api.declaration, Path::new("src/api.d.ts"));
    // An alias of a staged file reads the staged file.
    assert!(api.text.contains("from './types.ts'"), "{}", api.text);
    // A relative import of a staged file keeps its spelling.
    assert!(api.text.contains("from './util'"), "{}", api.text);
    // A relative import of anything else reads the authored file.
    assert!(
        api.text
            .contains(&format!("from '{}/src/../scripts/x'", project.root)),
        "{}",
        api.text
    );
    // A package the staged file reaches as the authored one does keeps its
    // spelling; one only the authored directory reaches is named by path.
    assert!(api.text.contains("from 'vue'"), "{}", api.text);
    assert!(
        api.text.contains(&format!(
            "from '{}/src/node_modules/dep/index.js'",
            project.root
        )),
        "{}",
        api.text
    );
    // A carrier with no staged product keeps its spelling.
    assert!(api.text.contains("from '@/Child.vue'"), "{}", api.text);
    assert_eq!(
        api.replaced
            .iter()
            .map(|(_, authored)| authored.as_str())
            .collect::<Vec<_>>(),
        ["@/types", "../scripts/x", "dep"]
    );
}

#[test]
fn published_declarations_spell_specifiers_as_the_authored_file_does() {
    let project = Project::new();
    let program = project.program();
    let root = &project.root;
    let emitted = format!(
        concat!(
            "import {{ t }} from './types.ts';\n",
            "import {{ b }} from '{root}/src/../scripts/x';\n",
            "export declare const c: import(\"./Child.vue\").default;\n",
            "export declare const l: typeof import(\"../../../src/legacy\");\n",
            "export declare const m: 'import(\"/project/src/model\")';\n",
        ),
        root = root
    );
    let (published, map) = program.published(
        &emitted,
        None,
        Some(api(&program)),
        Path::new("src"),
        Path::new("/out/src"),
    );
    assert_eq!(map, None);
    assert_eq!(
        published,
        concat!(
            "import { t } from '@/types';\n",
            "import { b } from '../scripts/x';\n",
            // Inside the mirror, the engine's spelling reads the same beside
            // the published file.
            "export declare const c: import(\"./Child.vue\").default;\n",
            // Outside it, the file is named from the authored directory.
            "export declare const l: typeof import(\"./legacy\");\n",
            // A literal type is not a specifier.
            "export declare const m: 'import(\"/project/src/model\")';\n",
        )
    );
}

#[test]
fn staged_positions_lead_to_the_authored_text_across_rebound_specifiers() {
    let project = Project::new();
    let program = project.program();
    let api = api(&program);
    let positions = Positions::new(api);
    let staged_column = api.text.find("x: number").unwrap() as u32;
    let authored_column = API.find("x: number").unwrap() as u32;
    assert_ne!(staged_column, authored_column);
    assert_eq!(
        positions.staged(0, staged_column),
        Some((0, authored_column))
    );
    // A later line is unchanged.
    assert_eq!(positions.staged(1, 4), Some((1, 4)));
}

#[test]
fn published_declaration_maps_follow_the_restored_specifiers_to_the_authored_file() {
    let project = Project::new();
    let program = project.program();
    let api = api(&program);
    // The engine's declaration: `x` is on the line whose specifier
    // publishing restores, after it.
    let emitted = "import { t } from './types.ts'; export declare const x: number;\n";
    let staged_x = api.text.find("x: number").unwrap() as u32;
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    builder.set_file("api.d.ts");
    let source = builder.add_source_and_content("../../.tmp1/mirror/src/api.ts", "");
    builder.add_token(
        0,
        emitted.find("x: number").unwrap() as u32,
        0,
        staged_x,
        Some(source),
        None,
    );
    let engine_map = builder.into_sourcemap().to_json_string();

    let (published, map) = program.published(
        emitted,
        Some(&engine_map),
        Some(api),
        Path::new("src"),
        Path::new("/out/types/src"),
    );
    assert_eq!(
        published,
        "import { t } from '@/types'; export declare const x: number;\n"
    );
    let map = SourceMap::from_json_string(map.as_deref().unwrap()).unwrap();
    assert_eq!(
        map.get_sources().collect::<Vec<_>>(),
        [compute_relative_path(
            "/out/types/src",
            &format!("{}/src/api.ts", project.root)
        )]
    );
    let table = map.generate_lookup_table();
    let published_x = published.find("x: number").unwrap() as u32;
    let token = map.lookup_token(&table, 0, published_x).unwrap();
    // The destination moved with the restored specifier; the source is the
    // authored `x`.
    assert_eq!(token.get_dst_col(), published_x);
    assert_eq!(
        (token.get_src_line(), token.get_src_col()),
        (0, API.find("x: number").unwrap() as u32)
    );
}

#[test]
fn maps_of_files_read_where_they_are_name_them_from_the_published_directory() {
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source = builder.add_source_and_content("../../../project/src/legacy.ts", "");
    builder.add_token(0, 0, 0, 0, Some(source), None);
    let map = builder.into_sourcemap().to_json_string();
    let rebased = rebase_sources(
        &map,
        Path::new("/out/stage/src"),
        Path::new("/out/types/src"),
    );
    assert_eq!(
        SourceMap::from_json_string(&rebased)
            .unwrap()
            .get_sources()
            .collect::<Vec<_>>(),
        ["../../../project/src/legacy.ts"]
    );
}

#[test]
fn the_engine_names_a_staged_source_declaration_by_its_extension() {
    for (module, declaration) in [
        ("src/Foo.vue.ts", Some("src/Foo.vue.d.ts")),
        ("src/a.ts", Some("src/a.d.ts")),
        ("src/a.tsx", Some("src/a.d.ts")),
        ("src/a.mts", Some("src/a.d.mts")),
        ("src/a.cjs", Some("src/a.d.cts")),
        ("src/a.json", None),
    ] {
        assert_eq!(
            emitted_declaration(Path::new(module)),
            declaration.map(PathBuf::from),
            "{module}"
        );
    }
}
