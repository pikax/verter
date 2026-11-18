//! The declaration program of one project, staged in a mirror of the project
//! directory.
//!
//! Each carrier's declaration product is staged at the carrier's TypeScript
//! checking module (`src/Foo.vue` at `src/Foo.vue.ts`), and each TypeScript
//! member of the project is copied to the same relative path, all under one
//! temporary mirror directory. The engine therefore resolves a carrier import
//! to the carrier's product wherever it is written, and emits each carrier's
//! declaration under its published name (`src/Foo.vue.d.ts`).
//!
//! Specifiers are bound through the project's own workspace resolution
//! before staging:
//!
//! - a relative specifier that names a staged file keeps its spelling, since
//!   the mirror keeps the project's layout;
//! - a relative specifier that names anything else is made absolute, so the
//!   program reads the authored file;
//! - a non-relative specifier (a `paths` alias) that names a staged file is
//!   spelled relative to the importer, so it reads the staged file rather
//!   than the `*.vue` wildcard module or a second copy of a member;
//! - a non-relative specifier that names anything else keeps its spelling
//!   when the staged file reaches the same target, and names the authored
//!   target by its path otherwise (a package installed beside the authored
//!   file, which the mirror's directories do not see).
//!
//! A `/// <reference path>` names a file, not a module: it keeps its
//! spelling when that file is staged and is made absolute otherwise.
//!
//! Members the mirror does not copy are read where they are: declaration
//! files, files outside the project directory, and sources whose specifiers
//! cannot be read. Their own carrier imports still resolve to the wildcard
//! module.
//!
//! Both rewrites, binding and publishing, run through `CodeTransform`
//! (`rewrite_module_specifiers`). Publishing moves the mirror's declarations
//! to the root of the output directory and gives every rewritten specifier
//! its authored spelling back; the engine's declaration map is carried over
//! that rewrite, composed with the binding's map and, for a carrier, led
//! through the product's map to the authored file. Diagnostics take the same
//! path.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oxc_sourcemap::{SourceMap, Token};
use tempfile::TempDir;
use verter_compiler::framework_common::virtual_file_naming::checking_module_identity_for_source;
use verter_compiler::framework_common::vue_projection_backend::DeclarationProjectionProduct;
use verter_compiler::tsc::{
    collect_module_inventory, collect_module_specifier_spans, rewrite_module_specifiers,
    ModuleSpecifierSpan,
};
use verter_session_query::resolution::path_utils::{
    collapse_path, is_absolute_specifier, is_relative_specifier,
};
use verter_span::path::InjectedPathKey;
use verter_workspace::native_fs::NativeFs;

use crate::api_check::TypecheckError;
use crate::checker::{
    absolutize_relative_specifier, compute_relative_path, declaration_stage_dirs,
    diagnostic_file_key, is_temp_tsconfig_error, run_declaration_engine, staged_file_key,
    write_declaration_shim, EmitOptions,
};
use crate::position::{ColumnEncoding, LineTable};
use crate::reporter::{Diagnostic, TscDiagnostic, VerterCode};

#[cfg(test)]
#[path = "declaration_mirror_tests.rs"]
mod tests;

/// Where a staged file's positions lead.
#[derive(Debug, Clone)]
enum Origin {
    /// A member: the staged text is its own, with specifiers rebound.
    Member,
    /// A carrier: the staged text is its declaration product, whose map
    /// leads to the carrier source.
    Carrier {
        source: Arc<str>,
        product: Box<DeclarationProjectionProduct>,
    },
}

/// A file the mirror stages, before its specifiers are bound.
struct Pending {
    authored: PathBuf,
    /// The staged file and the declaration the engine emits for it, both
    /// relative to the project directory.
    module: PathBuf,
    declaration: PathBuf,
    input: String,
    spans: Vec<ModuleSpecifierSpan>,
    /// `/// <reference path>` values.
    references: Vec<ModuleSpecifierSpan>,
    origin: Origin,
}

/// A staged file with its specifiers bound.
#[derive(Debug, Clone)]
pub(crate) struct MirroredFile {
    authored: PathBuf,
    module: PathBuf,
    declaration: PathBuf,
    /// The text the staged text was bound from.
    input: String,
    /// The text the engine reads.
    text: String,
    /// Each rebound specifier as `(staged, authored)` values.
    replaced: Vec<(String, String)>,
    /// The staged text's map back to `input`.
    map: SourceMap<'static>,
    origin: Origin,
}

/// The members of one project's declaration program, being collected. The
/// mirror directory exists from the start, so binding can ask how a staged
/// file resolves.
pub(crate) struct MirrorBuilder {
    root: PathBuf,
    temp_dir: TempDir,
    mirror: PathBuf,
    pending: Vec<Pending>,
    listed: Vec<PathBuf>,
}

/// One project's declaration program, ready to emit.
pub(crate) struct DeclarationMirror {
    root: PathBuf,
    temp_dir: TempDir,
    mirror: PathBuf,
    files: Vec<MirroredFile>,
    listed: Vec<PathBuf>,
}

/// The declaration the engine emits for a staged TypeScript or JavaScript
/// source, by its extension: `a.ts` and `a.tsx` emit `a.d.ts`, `a.mts`
/// emits `a.d.mts`, `a.cts` emits `a.d.cts`.
fn emitted_declaration(module: &Path) -> Option<PathBuf> {
    let extension = match module.extension()?.to_str()? {
        "ts" | "tsx" | "js" | "jsx" => "d.ts",
        "mts" | "mjs" => "d.mts",
        "cts" | "cjs" => "d.cts",
        _ => return None,
    };
    Some(module.with_extension(extension))
}

/// The spelling a module file is imported by, by path: a TypeScript or
/// declaration file by the JavaScript file it stands for (`index.d.ts` as
/// `index.js`), which the engine's resolution maps back to it; a
/// declaration file named directly is not importable as a value.
fn importable_path(target: &str) -> String {
    for (typed, javascript) in [
        (".d.ts", ".js"),
        (".d.mts", ".mjs"),
        (".d.cts", ".cjs"),
        (".tsx", ".js"),
        (".ts", ".js"),
        (".mts", ".mjs"),
        (".cts", ".cjs"),
    ] {
        if let Some(stem) = target.strip_suffix(typed) {
            return format!("{stem}{javascript}");
        }
    }
    target.to_string()
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// True when `path` is `directory` or lies under it.
fn is_within(path: &str, directory: &str) -> bool {
    super::declaration_output::is_within(Path::new(path), Path::new(directory))
}

impl MirrorBuilder {
    /// A program for the project at `root`, with its mirror directory made
    /// inside the project directory, so packages resolve from staged files
    /// as from the project's own.
    pub(crate) fn new(root: &Path) -> Result<Self, TypecheckError> {
        let (temp_dir, mirror) = declaration_stage_dirs(root, "mirror")?;
        Ok(Self {
            root: root.to_path_buf(),
            temp_dir,
            mirror,
            pending: Vec::new(),
            listed: Vec::new(),
        })
    }

    fn relative(&self, path: &Path) -> Option<PathBuf> {
        path.strip_prefix(&self.root).ok().map(Path::to_path_buf)
    }

    /// Stage `carrier`'s declaration product at its TypeScript checking
    /// module. `Err` says why it cannot be staged.
    pub(crate) fn carrier(
        &mut self,
        carrier: &Path,
        source: Arc<str>,
        product: &DeclarationProjectionProduct,
    ) -> Result<(), String> {
        let module = checking_module_identity_for_source(&slashed(carrier), "ts")
            .and_then(|module| self.relative(Path::new(&module)))
            .ok_or("the carrier has no TypeScript checking module in the project")?;
        let declaration = self
            .relative(Path::new(&product.module.declaration_file))
            .ok_or("the declaration is not named inside the project")?;
        // The engine names the declaration it emits for the staged module;
        // the product must name the same file.
        if emitted_declaration(&module).as_deref() != Some(declaration.as_path()) {
            return Err(format!(
                "the declaration is named '{}', but the engine emits '{}'",
                slashed(&declaration),
                emitted_declaration(&module)
                    .as_deref()
                    .map(slashed)
                    .unwrap_or_default()
            ));
        }
        // A product the inventory cannot parse is staged as written.
        let spans = collect_module_specifier_spans(&product.declaration_source)
            .map_err(|_| "its module specifiers could not be read")?
            .unwrap_or_default();
        self.pending.push(Pending {
            authored: carrier.to_path_buf(),
            module,
            declaration,
            input: product.declaration_source.clone(),
            spans,
            references: Vec::new(),
            origin: Origin::Carrier {
                source,
                product: Box::new(product.clone()),
            },
        });
        Ok(())
    }

    /// Copy a TypeScript or JavaScript member into the mirror. A member the
    /// mirror cannot stage (outside the project directory, no declaration
    /// of its own, unreadable specifiers) is read where it is.
    pub(crate) fn member(&mut self, path: &Path, text: &str) {
        let staged = self.relative(path).and_then(|module| {
            let declaration = emitted_declaration(&module)?;
            let inventory = collect_module_inventory(text).ok().flatten()?;
            Some((module, declaration, inventory))
        });
        match staged {
            Some((module, declaration, inventory)) => self.pending.push(Pending {
                authored: path.to_path_buf(),
                module,
                declaration,
                input: text.to_owned(),
                spans: inventory.specifiers,
                references: inventory.reference_paths,
                origin: Origin::Member,
            }),
            None => self.listed.push(path.to_path_buf()),
        }
    }

    /// A member the program reads where it is.
    pub(crate) fn list(&mut self, path: &Path) {
        self.listed.push(path.to_path_buf());
    }

    /// Bind every staged file's specifiers. `resolve(importer, specifier)`
    /// answers the file a specifier of `importer` names, as the project's
    /// workspace resolution spells it; it is asked for authored importers
    /// and for their staged paths.
    pub(crate) fn bind(
        self,
        resolve: impl Fn(&Path, &str) -> Option<String>,
    ) -> Result<DeclarationMirror, String> {
        let staged: HashMap<InjectedPathKey, &Path> = self
            .pending
            .iter()
            .map(|file| {
                (
                    InjectedPathKey::new(&slashed(&file.authored)),
                    file.authored.as_path(),
                )
            })
            .collect();
        let mut files = Vec::with_capacity(self.pending.len());
        for file in &self.pending {
            let directory = file.authored.parent().unwrap_or(Path::new("."));
            let staged_path = self.mirror.join(&file.module);
            // A `path` reference names a file, relative to the importer: it
            // keeps its spelling when that file is staged, and is made
            // absolute otherwise.
            let references: HashMap<&str, Option<String>> = file
                .references
                .iter()
                .map(|reference| {
                    let target =
                        collapse_path(&format!("{}/{}", slashed(directory), reference.text));
                    let spelling =
                        (!staged.contains_key(&InjectedPathKey::new(&target))).then_some(target);
                    (reference.text.as_str(), spelling)
                })
                .collect();
            let mut spans: Vec<ModuleSpecifierSpan> =
                file.spans.iter().chain(&file.references).cloned().collect();
            spans.sort_by_key(|span| span.start);
            let rewritten = rewrite_module_specifiers(
                &file.input,
                &spans,
                |specifier| {
                    if let Some(spelling) = references.get(specifier) {
                        return spelling.clone();
                    }
                    let target = resolve(&file.authored, specifier);
                    let staged_target = target
                        .as_deref()
                        .and_then(|target| staged.get(&InjectedPathKey::new(target)).copied());
                    if is_relative_specifier(specifier) {
                        return match staged_target {
                            Some(_) => None,
                            None => absolutize_relative_specifier(specifier, directory),
                        };
                    }
                    if let Some(staged_target) = staged_target {
                        return Some(compute_relative_path(
                            &collapse_path(&slashed(directory)),
                            &collapse_path(&slashed(staged_target)),
                        ));
                    }
                    // The staged file must reach what the authored one does;
                    // otherwise it imports the authored target by path.
                    let target = target?;
                    let from_mirror = resolve(&staged_path, specifier);
                    (from_mirror.as_deref().map(InjectedPathKey::new)
                        != Some(InjectedPathKey::new(&target)))
                    .then(|| importable_path(&collapse_path(&target)))
                },
                None,
            )
            .map_err(|_| {
                format!(
                    "the specifiers of '{}' could not be staged",
                    slashed(&file.authored)
                )
            })?;
            files.push(MirroredFile {
                authored: file.authored.clone(),
                module: file.module.clone(),
                declaration: file.declaration.clone(),
                input: file.input.clone(),
                text: rewritten.text,
                replaced: rewritten.replaced,
                map: rewritten.map,
                origin: file.origin.clone(),
            });
        }
        Ok(DeclarationMirror {
            root: self.root,
            temp_dir: self.temp_dir,
            mirror: self.mirror,
            files,
            listed: self.listed,
        })
    }
}

impl MirroredFile {
    fn authored_text(&self) -> &str {
        match &self.origin {
            Origin::Member => &self.input,
            Origin::Carrier { source, .. } => source,
        }
    }
}

/// Line tables over a staged file's input and authored texts, leading
/// zero-based UTF-16 positions in the input to the authored file.
struct Positions<'a> {
    file: &'a MirroredFile,
    /// The binding map's lookup table.
    table: Vec<&'a [Token]>,
    input_starts: Vec<usize>,
    authored_starts: Vec<usize>,
}

impl<'a> Positions<'a> {
    fn new(file: &'a MirroredFile) -> Self {
        Self {
            file,
            table: file.map.generate_lookup_table(),
            input_starts: LineTable::line_starts(&file.input),
            authored_starts: LineTable::line_starts(file.authored_text()),
        }
    }

    /// The authored zero-based `(line, column)` of an input one. A member's
    /// input is the authored file; a carrier's leads through its product's
    /// map, and a position the product left unmapped leads nowhere.
    fn authored(&self, line: u32, column: u32) -> Option<(u32, u32)> {
        let Origin::Carrier { product, .. } = &self.file.origin else {
            return Some((line, column));
        };
        let input = LineTable::from_parts(&self.file.input, &self.input_starts);
        let byte_column = input
            .convert_column(
                line + 1,
                column + 1,
                ColumnEncoding::Utf16,
                ColumnEncoding::Utf8,
            )
            .ok()?;
        let offset =
            u32::try_from(self.input_starts.get(line as usize)? + byte_column as usize - 1).ok()?;
        let authored_offset = product
            .map
            .mappings
            .iter()
            .find(|mapping| mapping.generated_start <= offset && offset < mapping.generated_end)
            .map(|mapping| mapping.source_start + offset - mapping.generated_start)?;
        let authored = LineTable::from_parts(self.file.authored_text(), &self.authored_starts);
        let (line, byte_column) = authored
            .line_col_of_offset(u64::from(authored_offset), ColumnEncoding::Utf8)
            .ok()?;
        let column = authored
            .convert_column(
                line,
                byte_column,
                ColumnEncoding::Utf8,
                ColumnEncoding::Utf16,
            )
            .ok()?;
        Some((line - 1, column - 1))
    }

    /// The authored zero-based position of a staged one, through the
    /// binding's map.
    fn staged(&self, line: u32, column: u32) -> Option<(u32, u32)> {
        let token = self.file.map.lookup_token(&self.table, line, column)?;
        token.get_source_id()?;
        self.authored(
            token.get_src_line(),
            token.get_src_col() + column - token.get_dst_col(),
        )
    }
}

impl DeclarationMirror {
    /// Emit the program's declarations under `opts.declaration_dir`, which
    /// is published at `published_dir`. Returns the diagnostics, each at its
    /// authored file, and the declaration files written. Fail-closed as the
    /// engine run is ([`crate::checker::DeclarationEngineRun`]).
    pub(crate) fn emit(
        &self,
        tsconfig_path: &Path,
        opts: &EmitOptions,
        published_dir: &Path,
        tsgo_bin: Option<&Path>,
    ) -> Result<(Vec<Diagnostic>, Vec<PathBuf>), TypecheckError> {
        let fs = NativeFs::new();
        let mut files = vec![write_declaration_shim(self.temp_dir.path())?];
        let mut staged = HashMap::new();
        for (index, file) in self.files.iter().enumerate() {
            let path = self.mirror.join(&file.module);
            fs.write_file(&path.to_string_lossy(), &file.text)
                .map_err(|error| {
                    TypecheckError::new(format!(
                        "verter-tsc: failed to stage {} for declaration emit: {error}",
                        file.authored.display()
                    ))
                })?;
            staged.insert(staged_file_key(&path), index);
            files.push(path);
        }
        files.extend(self.listed.iter().cloned());
        let mut run = run_declaration_engine(
            self.temp_dir.path(),
            &files,
            &self.root,
            tsconfig_path,
            opts,
            tsgo_bin,
        )?;
        let mut diagnostics = self.remap(std::mem::take(&mut run.raw), &staged);
        run.refuse_silent_failure(&diagnostics)?;
        let mut emitted = Vec::new();
        if let Some(output) = &opts.declaration_dir {
            diagnostics.extend(self.publish(&fs, output, published_dir, &mut emitted));
        }
        Ok((diagnostics, emitted))
    }

    /// Diagnostics at their authored files. One in a staged carrier the
    /// product's map does not reach is reported at the carrier's start.
    fn remap(&self, raw: Vec<TscDiagnostic>, staged: &HashMap<String, usize>) -> Vec<Diagnostic> {
        let mut positions: HashMap<usize, Positions<'_>> = HashMap::new();
        raw.into_iter()
            .filter(|diagnostic| !is_temp_tsconfig_error(diagnostic))
            .map(|diagnostic| {
                let reported = PathBuf::from(&diagnostic.file);
                let key = diagnostic_file_key(&reported.canonicalize().unwrap_or(reported));
                let Some(&index) = staged.get(&key) else {
                    let (line, col) = (diagnostic.line, diagnostic.col);
                    return diagnostic.into_diagnostic(None, line, col);
                };
                let file = &self.files[index];
                let (line, col) = positions
                    .entry(index)
                    .or_insert_with(|| Positions::new(file))
                    .staged(
                        diagnostic.line.saturating_sub(1),
                        diagnostic.col.saturating_sub(1),
                    )
                    .map_or((1, 1), |(line, column)| (line + 1, column + 1));
                diagnostic.into_diagnostic(Some(slashed(&file.authored)), line, col)
            })
            .collect()
    }

    /// Move the mirror's declarations and maps to their place under
    /// `output`, with authored specifiers and positions, rebase every other
    /// declaration map onto `published_dir`, and remove the stage's
    /// directories from `output`. Returns a diagnostic for each carrier
    /// whose declaration was not emitted.
    fn publish(
        &self,
        fs: &NativeFs,
        output: &Path,
        published_dir: &Path,
        emitted: &mut Vec<PathBuf>,
    ) -> Vec<Diagnostic> {
        let declarations: HashMap<String, &MirroredFile> = self
            .files
            .iter()
            .map(|file| (slashed(&file.declaration), file))
            .collect();
        let Ok(stage) = self.mirror.strip_prefix(&self.root) else {
            return vec![];
        };
        let mirrored_output = output.join(stage);
        let mut written = Vec::new();
        collect(fs, output, &mut written);
        for path in &written {
            let Some(content) = fs.read_file(&path.to_string_lossy()) else {
                continue;
            };
            let (relative, from_mirror) = match path.strip_prefix(&mirrored_output) {
                Ok(relative) => (relative.to_path_buf(), true),
                Err(_) => match path.strip_prefix(output) {
                    Ok(relative) => (relative.to_path_buf(), false),
                    Err(_) => continue,
                },
            };
            let key = slashed(&relative);
            let directory = relative.parent().unwrap_or(Path::new(""));
            let map_path = PathBuf::from(format!("{}.map", path.to_string_lossy()));
            let target = output.join(&relative);
            if key.ends_with(".map") {
                // A mirrored declaration publishes its own map.
                if from_mirror {
                    continue;
                }
                // The engine wrote the map where it found it.
                let written_dir = path.parent().unwrap_or(output);
                let rebased = rebase_sources(&content, written_dir, &published_dir.join(directory));
                if fs.write_file(&target.to_string_lossy(), &rebased).is_ok() {
                    emitted.push(target);
                }
                continue;
            }
            if !from_mirror {
                emitted.push(target);
                continue;
            }
            let map = written
                .contains(&map_path)
                .then(|| fs.read_file(&map_path.to_string_lossy()))
                .flatten();
            let (text, map) = self.published(
                &content,
                map.as_deref(),
                declarations.get(&key).copied(),
                directory,
                &published_dir.join(directory),
            );
            if fs.write_file(&target.to_string_lossy(), &text).is_ok() {
                emitted.push(target.clone());
            }
            if let Some(map) = map {
                let target = PathBuf::from(format!("{}.map", target.to_string_lossy()));
                if fs.write_file(&target.to_string_lossy(), &map).is_ok() {
                    emitted.push(target);
                }
            }
        }
        // The stage's temp directory, mirrored into the output, holds
        // nothing published.
        if let Some(first) = stage.components().next() {
            let stage_root = output.join(first);
            let _ = fs.delete_dir_all(&stage_root.to_string_lossy());
            emitted.retain(|path| !path.starts_with(&stage_root));
        }
        emitted.sort();
        emitted.dedup();
        self.files
            .iter()
            .filter(|file| matches!(file.origin, Origin::Carrier { .. }))
            .filter(|file| !fs.file_exists(&output.join(&file.declaration).to_string_lossy()))
            .map(|file| {
                Diagnostic::verter(
                    slashed(&file.authored),
                    1,
                    1,
                    VerterCode::PublicApiProjectionFailure,
                    format!(
                        "Declaration was not published: the engine emitted no '{}'",
                        slashed(&file.declaration)
                    ),
                )
            })
            .collect()
    }

    /// A declaration the mirror emitted at `directory` (relative to the
    /// project) and its map, as published in `published_dir`: a rebound
    /// specifier gets its authored spelling back, a specifier that names a
    /// file outside the mirror is spelled relative to the authored
    /// directory, and one inside it keeps the engine's spelling. The map is
    /// carried over that rewrite and leads to the authored file.
    fn published(
        &self,
        content: &str,
        map: Option<&str>,
        file: Option<&MirroredFile>,
        directory: &Path,
        published_dir: &Path,
    ) -> (String, Option<String>) {
        let root = collapse_path(&slashed(&self.root));
        let mirror = collapse_path(&slashed(&self.mirror));
        let staged_dir = collapse_path(&format!("{mirror}/{}", slashed(directory)));
        let authored_dir = collapse_path(&format!("{root}/{}", slashed(directory)));
        let restore = |specifier: &str| {
            if let Some((_, authored)) =
                file.and_then(|file| file.replaced.iter().find(|(staged, _)| staged == specifier))
            {
                return Some(authored.clone());
            }
            let absolute = if is_absolute_specifier(specifier) {
                collapse_path(specifier)
            } else if is_relative_specifier(specifier) {
                let absolute = collapse_path(&format!("{staged_dir}/{specifier}"));
                if is_within(&absolute, &mirror) {
                    return None;
                }
                absolute
            } else {
                return None;
            };
            let absolute = match absolute.strip_prefix(&mirror) {
                Some(rest) if is_within(&absolute, &mirror) => format!("{root}{rest}"),
                _ => absolute,
            };
            Some(compute_relative_path(&authored_dir, &absolute))
        };
        let Some(spans) = collect_module_specifier_spans(content).ok().flatten() else {
            return (content.to_string(), map.map(str::to_string));
        };
        let engine_map = map.and_then(|map| SourceMap::from_json_string(map).ok());
        let Ok(rewritten) =
            rewrite_module_specifiers(content, &spans, restore, engine_map.as_ref())
        else {
            return (content.to_string(), map.map(str::to_string));
        };
        let map = match (engine_map, file) {
            (Some(_), Some(file)) => Some(authored_map(rewritten.map, file, published_dir)),
            (Some(_), None) => map.map(str::to_string),
            (None, _) => None,
        };
        (rewritten.text, map)
    }
}

/// Every file under `directory`, recursively.
fn collect(fs: &NativeFs, directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs.read_dir(&directory.to_string_lossy()) else {
        return;
    };
    for entry in entries {
        let path = PathBuf::from(entry.path);
        if entry.is_dir {
            collect(fs, &path, files);
        } else {
            files.push(path);
        }
    }
}

/// A published declaration's map: `carried` leads the published text to
/// the staged file; each position then leads through the binding's map, at
/// its offset within the binding's segment, to the authored file, named
/// relative to `published_dir`. A segment with no authored position is
/// dropped.
fn authored_map(carried: SourceMap<'static>, file: &MirroredFile, published_dir: &Path) -> String {
    let positions = Positions::new(file);
    let mut parts = carried.into_parts();
    let tokens: Vec<Token> = parts
        .tokens
        .iter()
        .filter_map(|token| {
            token.get_source_id()?;
            let (line, column) = positions.staged(token.get_src_line(), token.get_src_col())?;
            Some(Token::new(
                token.get_dst_line(),
                token.get_dst_col(),
                line,
                column,
                Some(0),
                token.get_name_id(),
            ))
        })
        .collect();
    parts.tokens = tokens.into_boxed_slice();
    parts.token_chunks = None;
    parts.source_root = None;
    parts.sources = vec![Cow::Owned(compute_relative_path(
        &collapse_path(&slashed(published_dir)),
        &collapse_path(&slashed(&file.authored)),
    ))];
    parts.source_contents = vec![None];
    SourceMap::from_parts(parts).to_json_string()
}

/// A declaration map the engine wrote in `written_dir`, with its sources
/// relative to `published_dir`, where it is published. A map that cannot
/// be read, or sets its own `sourceRoot`, is returned as is.
fn rebase_sources(content: &str, written_dir: &Path, published_dir: &Path) -> String {
    let Ok(map) = SourceMap::from_json_string(content) else {
        return content.to_string();
    };
    let mut parts = map.into_parts();
    if parts
        .source_root
        .as_deref()
        .is_some_and(|root| !root.is_empty())
    {
        return content.to_string();
    }
    let published_dir = collapse_path(&slashed(published_dir));
    let written_dir = collapse_path(&slashed(written_dir));
    parts.sources = parts
        .sources
        .iter()
        .map(|source| {
            let absolute = if is_absolute_specifier(source) {
                collapse_path(source)
            } else {
                collapse_path(&format!("{written_dir}/{source}"))
            };
            Cow::Owned(compute_relative_path(&published_dir, &absolute))
        })
        .collect();
    SourceMap::from_parts(parts).to_json_string()
}
