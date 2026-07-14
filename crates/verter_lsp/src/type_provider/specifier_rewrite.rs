//! The SHARED module-specifier rewrite layer for engine-produced import edits.
//!
//! When an external TS engine (tsserver / tsgo) resolves a bare framework-carrier
//! import (`import Comp from "./Comp.vue"`) it does so through the carrier
//! COMPANION the engine actually sees on its FS/overlay: the `.d.<ext>.ts`
//! DECLARATION carrier `./Comp.d.vue.ts` / `./Comp.d.svelte.ts` (the
//! extension-middle surface a bare carrier import resolves to),
//! the component IDE carrier `./Comp.vue.tsx` / `./Comp.svelte.tsx` (`.jsx` under
//! a JSX project), or the redirect-reached public-API carrier
//! `./Comp.vue.verter.ts`. An auto-import / add-missing-import / organize-imports
//! / rename / code-action edit the engine emits for the USER's source file will
//! therefore, left raw, insert that companion specifier — or, when module
//! resolution strips the extension, a BARE `./Comp` with no extension at all. None
//! of those is what the user would have written (`./Comp.vue`).
//!
//! This module is the single SHARED authority that rewrites the inserted import
//! SPECIFIER back to the user-facing bare `.vue` / `.svelte` form. It is consumed
//! by the provider-neutral merge layer (so BOTH the tsgo and tsserver backends
//! reuse it) and is the layer the future SHARED-mode leak-suppression (§2.10)
//! reuses for the same `additionalTextEdits` / code-action / organize-imports /
//! `workspace/applyEdit` / file-rename edit channels.
//!
//! ## What it transforms — and what it never touches
//!
//! It transforms the engine's OUTPUT: the `new_text` of a `TextEdit` /
//! `WorkspaceEdit` / code-action edit / completion `additionalTextEdits` / rename
//! edit destined for the USER's source file. It is a PATH-string normalization of
//! an already-produced edit (a module specifier IS text), in the same allowed class
//! as [`merge::normalize_carrier_path`](crate::type_provider::merge) — NOT a
//! semantic type decision and NOT a string-sniff of type text. It NEVER rewrites
//! the `CodeTransform`-built carrier text (that stays CodeTransform-authored and
//! stable per the "CodeTransform is the single source of truth" rule, which governs
//! Verter's generated carrier, never an LSP edit the engine emits for user source).
//!
//! ## Fail-closed per whole user-visible action
//!
//! The rewrite is FAIL-CLOSED: if an edit names a carrier companion (or a bare
//! `./Comp` that resolves to a carrier) that CANNOT be unambiguously mapped to a
//! single user-facing `.vue` / `.svelte` form, the whole action is DROPPED
//! ([`SpecifierRewrite::Drop`]) — never partially rewritten and never emitted with
//! a leaking companion specifier. A specifier that is NOT a carrier companion (a
//! plain `./utils`, a real Svelte rune `./store.svelte.ts`, a `.tsx` whose stem is
//! not a carrier) is left [`SpecifierRewrite::Unchanged`].
//!
//! Carrier-path classification is the registry-derived
//! [`verter_workspace::path_is_carrier`] / `CARRIER_API_VIRTUAL_SUFFIX` authority —
//! never an ad-hoc string match. A new carrier extension extends the registry, not
//! this module.

use std::path::Path;

/// Context an inserted-import rewrite resolves against: the user-facing file the
/// edit applies to, plus the carrier-source-existence probe (the host/VFS source
/// authority) used to resolve an extension-less bare `./Comp` specifier to its
/// concrete `.vue` / `.svelte` carrier.
///
/// `edit_target_path` is the USER-FACING path the edit is destined for — the
/// `.vue` / `.svelte` carrier SOURCE (for a carrier-IDE projection edit, already
/// stripped of the `.tsx` companion suffix) or a real `.ts` / `.js` importer. The
/// bare-specifier resolution joins a relative specifier against this path's
/// DIRECTORY. `carrier_source_exists` is the same source-of-truth probe the merge
/// layer's `carrier_source_exists` closures use (`host.get_source(p).is_some()`),
/// so a candidate carrier is "real" iff the host has its source.
pub struct SpecifierRewriteCtx<'a> {
    /// The user-facing file the edit applies to (a `.vue`/`.svelte` carrier source
    /// or a real `.ts`/`.js`). Forward- or back-slashed; normalized internally.
    pub edit_target_path: &'a str,
    /// Whether a carrier SOURCE path exists in the host/VFS (the source authority).
    pub carrier_source_exists: &'a dyn Fn(&str) -> bool,
}

/// The outcome of rewriting one inserted-import `new_text`.
///
/// Three states, fail-closed by construction:
/// - [`Unchanged`](Self::Unchanged): the text carries no carrier-companion / bare
///   carrier specifier to rewrite (a plain `./utils`, a rune `./store.svelte.ts`,
///   non-import text). Apply the edit verbatim.
/// - [`Rewritten`](Self::Rewritten): the specifier named a carrier and was
///   unambiguously mapped to the bare `.vue`/`.svelte` form. Apply this text.
/// - [`Drop`](Self::Drop): the specifier named a carrier (a companion, or a bare
///   `./Comp` resolving to a carrier) that could NOT be unambiguously mapped — e.g.
///   a bare `./Comp` matching BOTH `Comp.vue` AND `Comp.svelte`, or a companion
///   whose bare carrier source does not exist. Drop the WHOLE action — never emit a
///   leaking companion specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecifierRewrite {
    Unchanged,
    Rewritten(String),
    Drop,
}

/// Rewrite a carrier-companion / bare-carrier import specifier inside an
/// engine-produced `new_text` back to the bare `.vue`/`.svelte` form, resolving an
/// extension-less specifier against `ctx`.
///
/// See the module docs for the full contract. The returned [`SpecifierRewrite`]
/// tells the caller whether to apply the edit verbatim, apply the rewritten text,
/// or DROP the whole action (fail closed).
#[must_use]
pub fn rewrite_inserted_carrier_specifier(
    new_text: &str,
    ctx: &SpecifierRewriteCtx<'_>,
) -> SpecifierRewrite {
    // Locate the module-specifier string literal. An import statement carries it
    // as `from "…"` / `from '…'`; a side-effect import is a bare `import "…"`. We
    // rewrite the FIRST such literal only (an import line has exactly one).
    let Some((open_quote_idx, quote)) = find_specifier_quote(new_text) else {
        return SpecifierRewrite::Unchanged;
    };
    let spec_start = open_quote_idx + 1;
    let Some(rel_close) = new_text[spec_start..].find(quote) else {
        return SpecifierRewrite::Unchanged;
    };
    let spec_end = spec_start + rel_close;
    let specifier = &new_text[spec_start..spec_end];

    match classify_specifier(specifier, ctx) {
        SpecifierClass::NotCarrier => SpecifierRewrite::Unchanged,
        SpecifierClass::Unmappable => SpecifierRewrite::Drop,
        SpecifierClass::Bare(bare) => {
            let mut out = String::with_capacity(new_text.len() + bare.len());
            out.push_str(&new_text[..spec_start]);
            out.push_str(&bare);
            out.push_str(&new_text[spec_end..]);
            SpecifierRewrite::Rewritten(out)
        }
    }
}

/// Classification of one module specifier for the rewrite.
enum SpecifierClass {
    /// Not a carrier companion / bare carrier — leave the edit unchanged.
    NotCarrier,
    /// A carrier the specifier unambiguously maps to (the bare `.vue`/`.svelte`).
    Bare(String),
    /// A carrier specifier that cannot be unambiguously mapped — drop the action.
    Unmappable,
}

/// Classify `specifier` into [`SpecifierClass`].
///
/// Two families:
/// 1. **Carrier companion** (`.tsx`/`.jsx` IDE companion, the `.verter.ts` API
///    carrier, or the `.d.<ext>.ts` declaration carrier) — a deterministic suffix
///    strip whose (reconstructed) stem is a registry-classified carrier
///    (`path_is_carrier`). Context-free; never ambiguous.
/// 2. **Extension-less bare** (`./Comp`) — the engine stripped the extension under
///    module resolution. Resolve it against `ctx.edit_target_path`'s directory: for
///    each registry carrier extension, probe `{dir}/{spec}.{ext}` via
///    `carrier_source_exists`. ZERO matches ⇒ a plain module import (NotCarrier,
///    leave it); ONE match ⇒ append that extension; TWO+ matches ⇒ Unmappable
///    (ambiguous — drop, never guess).
fn classify_specifier(specifier: &str, ctx: &SpecifierRewriteCtx<'_>) -> SpecifierClass {
    // Family 1: carrier companion (context-free deterministic strip).
    if let Some(bare) = bare_carrier_companion(specifier) {
        return SpecifierClass::Bare(bare);
    }

    // A specifier that already carries any file extension is NOT an extension-less
    // bare specifier: an unrecognised extension (`./plain.tsx`, `./store.svelte.ts`,
    // `./utils.js`) is a plain import we must leave alone. Only an EXTENSION-LESS
    // relative specifier is a bare-carrier candidate.
    if !is_extension_less_relative(specifier) {
        return SpecifierClass::NotCarrier;
    }

    // Family 2: extension-less bare `./Comp` — resolve against the edit-target dir.
    let Some(target_dir) = parent_dir(ctx.edit_target_path) else {
        return SpecifierClass::NotCarrier;
    };
    let mut matched: Option<String> = None;
    let mut match_count = 0usize;
    for ext in verter_workspace::carrier_source_extensions() {
        // The candidate carrier SOURCE path the bare specifier would resolve to.
        let candidate = join_relative(&target_dir, specifier);
        let candidate = format!("{candidate}.{ext}");
        if (ctx.carrier_source_exists)(&candidate) {
            match_count += 1;
            matched = Some(format!("{specifier}.{ext}"));
        }
    }
    match match_count {
        0 => SpecifierClass::NotCarrier,
        1 => SpecifierClass::Bare(matched.expect("one match implies Some")),
        _ => SpecifierClass::Unmappable,
    }
}

/// The bare `.vue`/`.svelte` specifier for a carrier COMPANION specifier, or `None`
/// when `specifier` is not a carrier companion.
///
/// Strips a `.tsx`/`.jsx` IDE-companion suffix, the `.verter.ts` API-carrier
/// suffix, or a `.d.<ext>.ts` DECLARATION-carrier suffix, and keeps the result
/// only when the (reconstructed) stem is a registry-classified carrier path
/// ([`verter_workspace::path_is_carrier`]). A non-carrier specifier (a plain
/// `./utils`, a `.svelte.ts` rune module whose stem `./store.svelte` is itself a
/// carrier extension but which is a rune — NOT a `.svelte.tsx` companion nor a
/// `.d.svelte.ts` declaration — and a bare `./types.d.ts` whose stem is not a
/// carrier) yields `None`.
fn bare_carrier_companion(specifier: &str) -> Option<String> {
    // API carrier: `./Comp.vue.verter.ts` → `./Comp.vue`.
    if let Some(stem) = specifier.strip_suffix(verter_workspace::CARRIER_API_VIRTUAL_SUFFIX) {
        if verter_workspace::path_is_carrier(stem) {
            return Some(stem.to_string());
        }
        return None;
    }
    // Declaration carrier: `./Comp.d.vue.ts` → `./Comp.vue` (the EXTENSION-MIDDLE
    // `.d.<ext>.ts` surface a bare framework-carrier import resolves to). Strip
    // `.d.<ext>.ts`, reconstruct the carrier source
    // `{stem}.{ext}`, and keep it only when that source is a registry-classified
    // carrier — so a bare `./types.d.ts` (no carrier extension in the middle) and
    // a `.svelte.ts` rune (no `.d.` infix) are left alone. The carrier extension
    // is the registry authority, never a hardcoded literal.
    for ext in verter_workspace::carrier_source_extensions() {
        if let Some(stem) = specifier.strip_suffix(&format!(".d.{ext}.ts")) {
            let carrier_source = format!("{stem}.{ext}");
            if verter_workspace::path_is_carrier(&carrier_source) {
                return Some(carrier_source);
            }
        }
    }
    // IDE companion: `./Comp.vue.tsx` / `./Comp.svelte.jsx` → bare carrier.
    for ext in [".tsx", ".jsx"] {
        if let Some(stem) = specifier.strip_suffix(ext) {
            if verter_workspace::path_is_carrier(stem) {
                return Some(stem.to_string());
            }
        }
    }
    None
}

/// Whether `specifier` is an extension-less RELATIVE module specifier (`./Comp`,
/// `../x/Comp`) — the bare-carrier candidate shape. A specifier whose final path
/// segment contains a `.` (an extension, `./Comp.vue`, `./plain.tsx`,
/// `./store.svelte.ts`) is NOT extension-less. A non-relative specifier (a bare
/// package name `vue`, an alias `@/Comp`) is not handled here (the engine produces
/// relative companion edits for workspace carriers; an alias-form bare specifier
/// has no unambiguous on-disk sibling to probe and is left to the NotCarrier path).
fn is_extension_less_relative(specifier: &str) -> bool {
    if !(specifier.starts_with("./") || specifier.starts_with("../")) {
        return false;
    }
    let last = specifier.rsplit(['/', '\\']).next().unwrap_or(specifier);
    !last.contains('.')
}

/// The parent directory of `path` (forward-slashed), or `None` when it has none.
fn parent_dir(path: &str) -> Option<String> {
    let p = Path::new(path);
    p.parent().map(|d| d.to_string_lossy().replace('\\', "/"))
}

/// Join a relative specifier onto a directory, normalizing `.`/`..` segments and
/// returning a forward-slashed path. Used only to build the candidate carrier
/// SOURCE path for the existence probe (never emitted into the edit).
fn join_relative(dir: &str, specifier: &str) -> String {
    let mut segments: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in specifier.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    // Preserve a leading drive letter / root: rebuild from the original dir prefix.
    let leading = if dir.starts_with('/') { "/" } else { "" };
    format!("{leading}{}", segments.join("/"))
}

/// The byte index of the opening quote of the module specifier in an import
/// `new_text`, plus the quote char. Prefers a `from` clause; falls back to a bare
/// side-effect `import "…"`. Returns `None` when no specifier literal is found
/// (the text is not an import line).
fn find_specifier_quote(new_text: &str) -> Option<(usize, char)> {
    // `from` clause: scan for the keyword, then the next quote after it.
    if let Some(from_idx) = new_text.find("from ") {
        let after = from_idx + "from ".len();
        if let Some(rel) = new_text[after..].find(['"', '\'']) {
            let idx = after + rel;
            let quote = new_text.as_bytes()[idx] as char;
            return Some((idx, quote));
        }
    }
    // Bare side-effect import: `import "…";`.
    if let Some(import_idx) = new_text.find("import ") {
        let after = import_idx + "import ".len();
        let rest = new_text[after..].trim_start();
        if rest.starts_with('"') || rest.starts_with('\'') {
            let idx = new_text.len() - rest.len();
            let quote = new_text.as_bytes()[idx] as char;
            return Some((idx, quote));
        }
    }
    None
}

#[cfg(test)]
#[path = "specifier_rewrite_tests.rs"]
mod tests;
