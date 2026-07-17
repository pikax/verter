//! Managed-tsgo JSX authority for Vue IDE carriers.
//!
//! A compiler-produced Vue carrier starts with one unmapped
//! `@jsxImportSource vue` line. Native tsgo does not apply the host's private
//! compiler-option preferences to a configured project, so a project-wide
//! React JSX namespace can otherwise become the classic fallback and reject
//! valid Vue `class` attributes or nested slot content. For the managed-tsgo
//! topology only, this module replaces that generated line with one generated
//! classic-JSX line importing an owner-bound adapter. The adapter aliases the
//! installed Vue package's official `vue/jsx-runtime` namespace. Its explicit
//! empty local `ElementChildrenAttribute` prevents TypeScript implementations
//! that honor the classic factory namespace from falling back to an unrelated
//! global `React.JSX.children` convention; Vue JSX children are slots, not a
//! synthetic component prop.
//!
//! Replacing one unmapped line with one unmapped line keeps all authored source
//! lines and columns stable. Other provider topologies receive the compiler
//! bytes unchanged.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use verter_session::framework::descriptor::{
    classify_carrier_companion, vue_descriptor, CarrierCompanionKind,
};

const VUE_JSX_PRAGMA: &str = "/** @jsxImportSource vue */\n";

pub(crate) struct PreparedManagedTsgoVueCarrier {
    pub(crate) content: String,
    #[cfg(test)]
    pub(crate) adapter_path: PathBuf,
    #[cfg(test)]
    pub(crate) adapter_content: String,
}

struct ResolvedVuePackage {
    root: PathBuf,
    jsx_runtime_types: PathBuf,
    package_json: Vec<u8>,
    runtime_types: Vec<u8>,
    version: String,
}

fn invalid_package(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(ErrorKind::InvalidData, message.into())
}

fn exported_types(value: &serde_json::Value) -> Option<&str> {
    match value {
        serde_json::Value::String(target) => Some(target),
        serde_json::Value::Array(entries) => entries.iter().find_map(exported_types),
        serde_json::Value::Object(conditions) => conditions
            .get("types")
            .and_then(serde_json::Value::as_str)
            .or_else(|| conditions.get("import").and_then(exported_types))
            .or_else(|| conditions.get("default").and_then(exported_types)),
        _ => None,
    }
}

fn resolve_declaration_target(package_root: &Path, target: &str) -> std::io::Result<PathBuf> {
    let relative = target.strip_prefix("./").unwrap_or(target);
    if Path::new(relative).is_absolute()
        || relative.split(['/', '\\']).any(|segment| segment == "..")
    {
        return Err(invalid_package(format!(
            "Vue jsx-runtime declaration target escapes its package: {target}"
        )));
    }
    let path = package_root.join(relative);
    if !path.is_file() {
        return Err(invalid_package(format!(
            "Vue jsx-runtime declaration target does not exist: {}",
            path.display()
        )));
    }
    std::fs::canonicalize(path)
}

fn resolve_vue_package(candidate: &Path) -> std::io::Result<ResolvedVuePackage> {
    let package_json_path = candidate.join("package.json");
    let package_json = std::fs::read(&package_json_path)?;
    let manifest: serde_json::Value = serde_json::from_slice(&package_json)
        .map_err(|error| invalid_package(format!("invalid Vue package.json: {error}")))?;
    if manifest.get("name").and_then(serde_json::Value::as_str) != Some("vue") {
        return Err(invalid_package(format!(
            "package at {} is not named `vue`",
            candidate.display()
        )));
    }
    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid_package("Vue package.json has no version"))?
        .to_owned();
    let target = manifest
        .get("exports")
        .and_then(|exports| exports.get("./jsx-runtime"))
        .and_then(exported_types)
        .ok_or_else(|| invalid_package("Vue package has no typed ./jsx-runtime export"))?;
    let root = std::fs::canonicalize(candidate)?;
    let jsx_runtime_types = resolve_declaration_target(&root, target)?;
    let runtime_types = std::fs::read(&jsx_runtime_types)?;
    Ok(ResolvedVuePackage {
        root,
        jsx_runtime_types,
        package_json,
        runtime_types,
        version,
    })
}

fn nearest_vue_for_carrier(provider_path: &str) -> std::io::Result<Option<ResolvedVuePackage>> {
    let provider = Path::new(provider_path);
    let mut directory = provider.parent();
    while let Some(current) = directory {
        let candidate = current.join("node_modules/vue");
        if candidate.join("package.json").is_file() {
            return resolve_vue_package(&candidate).map(Some);
        }
        directory = current.parent();
    }
    Ok(None)
}

fn normalized_module_path(path: &Path) -> String {
    verter_span::path::canonicalize_path_cow(&path.to_string_lossy()).into_owned()
}

fn is_vue_ide_companion(provider_path: &str) -> bool {
    let Some(companion) = classify_carrier_companion(provider_path) else {
        return false;
    };
    if companion.kind != CarrierCompanionKind::Ide {
        return false;
    }
    vue_descriptor()
        .carrier_companion_identities(&companion.source)
        .into_iter()
        .any(|candidate| {
            candidate.kind == CarrierCompanionKind::Ide && candidate.path == companion.path
        })
}

fn owner_asset_key(package: &ResolvedVuePackage) -> String {
    let mut identity = blake3::Hasher::new();
    for field in [
        package.version.as_bytes(),
        normalized_module_path(&package.root).as_bytes(),
        normalized_module_path(&package.jsx_runtime_types).as_bytes(),
        package.package_json.as_slice(),
        package.runtime_types.as_slice(),
    ] {
        identity.update(&(field.len() as u64).to_le_bytes());
        identity.update(field);
    }
    identity.finalize().to_hex()[..24].to_owned()
}

fn adapter_asset_key(owner_key: &str, adapter_content: &str) -> String {
    let mut identity = blake3::Hasher::new();
    for field in [owner_key.as_bytes(), adapter_content.as_bytes()] {
        identity.update(&(field.len() as u64).to_le_bytes());
        identity.update(field);
    }
    identity.finalize().to_hex()[..24].to_owned()
}

fn collision_free_binding(asset_key: &str, content: &str) -> String {
    for nonce in 0_u64.. {
        let suffix = if nonce == 0 {
            asset_key.to_owned()
        } else {
            blake3::hash(format!("{asset_key}\0{nonce}").as_bytes()).to_hex()[..24].to_owned()
        };
        let candidate = format!("__verter_vue_jsx_{suffix}");
        if !content.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("the unbounded hash namespace always has a collision-free identifier")
}

fn classic_jsx_adapter(package: &ResolvedVuePackage) -> String {
    let runtime = serde_json::to_string(&normalized_module_path(
        &package.jsx_runtime_types.with_extension(""),
    ))
    .expect("a filesystem path always serializes as a JSON string");
    format!(
        r#"import type {{ JSX as __VerterAutomaticJSX }} from {runtime};
export function h(...args: unknown[]): __VerterAutomaticJSX.Element;
export const Fragment: unique symbol;
export namespace JSX {{
  type Element = __VerterAutomaticJSX.Element;
  type ElementClass = __VerterAutomaticJSX.ElementClass;
  type ElementAttributesProperty = __VerterAutomaticJSX.ElementAttributesProperty;
  interface ElementChildrenAttribute {{}}
  type IntrinsicElements = __VerterAutomaticJSX.IntrinsicElements;
  type IntrinsicAttributes = __VerterAutomaticJSX.IntrinsicAttributes;
}}
"#
    )
}

fn host_adapter_dir(asset_key: &str) -> PathBuf {
    std::env::temp_dir()
        .join("verter-host")
        .join(format!("vue-jsx-{}-{asset_key}", env!("CARGO_PKG_VERSION")))
}

fn write_immutable(path: &Path, content: &str) -> std::io::Result<()> {
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == content => return Ok(()),
        Ok(_) => {
            return Err(std::io::Error::new(
                ErrorKind::AlreadyExists,
                format!(
                    "immutable Vue JSX asset has unexpected bytes: {}",
                    path.display()
                ),
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            ErrorKind::InvalidInput,
            format!("Vue JSX asset has no parent: {}", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid_package("Vue JSX asset has no UTF-8 file name"))?;
    let temp_path = loop {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), id));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                if let Err(error) = file
                    .write_all(content.as_bytes())
                    .and_then(|()| file.sync_all())
                {
                    drop(file);
                    let _ = std::fs::remove_file(&candidate);
                    return Err(error);
                }
                drop(file);
                break candidate;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };

    match std::fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let concurrent_match =
                std::fs::read_to_string(path).is_ok_and(|existing| existing == content);
            let _ = std::fs::remove_file(&temp_path);
            if concurrent_match {
                Ok(())
            } else {
                Err(rename_error)
            }
        }
    }
}

/// Replace the compiler-owned Vue JSX pragma with an owner-bound classic JSX
/// adapter for managed tsgo. `Ok(None)` means the path/content is not a Vue IDE
/// carrier or the owner has no resolvable Vue package; in either case the
/// original bytes remain the honest fail-closed input.
pub(crate) fn prepare_managed_tsgo_vue_carrier(
    provider_path: &str,
    content: &str,
) -> std::io::Result<Option<PreparedManagedTsgoVueCarrier>> {
    if !is_vue_ide_companion(provider_path) || !content.starts_with(VUE_JSX_PRAGMA) {
        return Ok(None);
    }
    let Some(package) = nearest_vue_for_carrier(provider_path)? else {
        return Ok(None);
    };

    let adapter_content = classic_jsx_adapter(&package);
    // The host asset is immutable, so its identity must include BOTH the Vue
    // owner inputs and the final generated bytes. Keying only by the owner
    // leaves an earlier adapter schema at the same path after an in-place
    // Verter upgrade; `write_immutable` then correctly refuses the mismatch and
    // the carrier never reaches tsgo. Content-addressing the final adapter keeps
    // old assets harmless while preserving atomic immutable publication.
    let key = adapter_asset_key(&owner_asset_key(&package), &adapter_content);
    let adapter_path = host_adapter_dir(&key).join("classic.d.ts");
    write_immutable(&adapter_path, &adapter_content)?;

    let factory_namespace = collision_free_binding(&key, content);
    let adapter_specifier = adapter_path.with_extension("");
    let import_path = serde_json::to_string(&normalized_module_path(&adapter_specifier))
        .expect("a filesystem path always serializes as a JSON string");
    let provider_intro = format!(
        "/** @jsxRuntime classic */ /** @jsx {factory_namespace}.h */ /** @jsxFrag {factory_namespace}.Fragment */ import * as {factory_namespace} from {import_path};\n"
    );
    let mut prepared =
        String::with_capacity(content.len() - VUE_JSX_PRAGMA.len() + provider_intro.len());
    prepared.push_str(&provider_intro);
    prepared.push_str(&content[VUE_JSX_PRAGMA.len()..]);

    Ok(Some(PreparedManagedTsgoVueCarrier {
        content: prepared,
        #[cfg(test)]
        adapter_path,
        #[cfg(test)]
        adapter_content,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn vendor_vue(root: &Path) {
        let package = root.join("node_modules/vue");
        std::fs::create_dir_all(package.join("jsx-runtime")).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"vue","version":"3.5.40","exports":{"./jsx-runtime":{"types":"./jsx-runtime/index.d.ts"}}}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("jsx-runtime/index.d.ts"),
            r#"export namespace JSX {
  interface Element {}
  interface ElementClass { $props: {} }
  interface ElementAttributesProperty { $props: {} }
  interface IntrinsicElements { div: { class?: string }; [name: string]: any }
  interface IntrinsicAttributes {}
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn managed_tsgo_vue_carrier_replaces_only_the_unmapped_intro_line() {
        let tmp = tempdir().unwrap();
        vendor_vue(tmp.path());
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let carrier = tmp.path().join("src/App.vue.tsx");
        let authored_tail =
            "const label = 'ok';\nconst view = <div class=\"card\">{label}</div>;\n";
        let source = format!("{VUE_JSX_PRAGMA}{authored_tail}");

        let prepared = prepare_managed_tsgo_vue_carrier(&carrier.to_string_lossy(), &source)
            .expect("prepare")
            .expect("Vue IDE carrier must specialize");

        assert!(prepared.content.starts_with("/** @jsxRuntime classic */"));
        assert!(!prepared.content.contains("@jsxImportSource vue"));
        assert_eq!(
            prepared.content.split_once('\n').unwrap().1,
            authored_tail,
            "all authored bytes after the generated pragma line stay byte-identical"
        );
        assert_eq!(prepared.content.lines().count(), source.lines().count());
        assert!(prepared.adapter_path.starts_with(std::env::temp_dir()));
        assert!(!prepared.adapter_path.starts_with(tmp.path()));
        assert_eq!(
            std::fs::read_to_string(&prepared.adapter_path).unwrap(),
            prepared.adapter_content
        );
        assert!(prepared
            .adapter_content
            .contains("JSX as __VerterAutomaticJSX"));
        assert!(
            !prepared.adapter_content.contains("//?/"),
            "the TypeScript module specifier must not retain a Windows extended-path prefix"
        );
        assert!(prepared
            .adapter_content
            .contains("interface ElementChildrenAttribute {}"));
    }

    #[test]
    fn managed_tsgo_adapter_identity_survives_a_stale_prior_schema_asset() {
        let tmp = tempdir().unwrap();
        vendor_vue(tmp.path());
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let carrier = tmp.path().join("src/App.vue.tsx");
        let source = format!("{VUE_JSX_PRAGMA}const view = <div class=\"card\" />;\n");

        let package = nearest_vue_for_carrier(&carrier.to_string_lossy())
            .expect("resolve Vue owner")
            .expect("vendored Vue owner");
        let stale_dir = host_adapter_dir(&owner_asset_key(&package));
        let stale_path = stale_dir.join("classic.d.ts");
        std::fs::create_dir_all(&stale_dir).unwrap();
        std::fs::write(&stale_path, "// bytes from an earlier adapter schema\n").unwrap();

        let prepared = prepare_managed_tsgo_vue_carrier(&carrier.to_string_lossy(), &source)
            .expect("a stale prior-schema asset must not block the current content-addressed asset")
            .expect("Vue IDE carrier must specialize");

        assert_ne!(
            prepared.adapter_path, stale_path,
            "the immutable adapter path must include the final generated adapter bytes"
        );
        assert_eq!(
            std::fs::read_to_string(&prepared.adapter_path).unwrap(),
            prepared.adapter_content
        );

        let _ = std::fs::remove_dir_all(stale_dir);
        if let Some(current_dir) = prepared.adapter_path.parent() {
            let _ = std::fs::remove_dir_all(current_dir);
        }
    }

    #[test]
    fn non_vue_or_non_ide_inputs_are_not_specialized() {
        let tmp = tempdir().unwrap();
        vendor_vue(tmp.path());
        let plain = tmp.path().join("src/plain.tsx");
        let api = tmp.path().join("src/App.vue.verter.ts");
        let source = format!("{VUE_JSX_PRAGMA}const view = <div />;\n");
        assert!(
            prepare_managed_tsgo_vue_carrier(&plain.to_string_lossy(), &source)
                .unwrap()
                .is_none()
        );
        assert!(
            prepare_managed_tsgo_vue_carrier(&api.to_string_lossy(), &source)
                .unwrap()
                .is_none()
        );
    }
}
