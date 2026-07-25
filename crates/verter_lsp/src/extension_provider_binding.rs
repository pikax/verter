//! The per-file project-binding decision for [`ExtensionTypeProvider`].
//!
//! The provider's ownership vocabulary and the one place a file's declared
//! `projectRootPath` / `projectConfigPath` is decided (or refused). Wired back
//! as a child module (`mod binding`) of `extension_provider.rs`, so the inherent
//! `impl` below reaches the provider's private state exactly as the methods did
//! when they lived in that file.

use crate::type_provider::protocol::TypeProviderError;
use crate::type_provider::traits::ProjectOwnership;

use super::{ExtensionTypeProvider, TsQueryTransport};

impl<T: TsQueryTransport> ExtensionTypeProvider<T> {
    /// The `projectRootPath` + `projectConfigPath` stamped on every `open` /
    /// `updateOpen` envelope — the root the extension host resolves this file's
    /// TypeScript from, and the config that gives that project its compiler
    /// options and its identity.
    ///
    /// The CONFIGURED OWNER decides. Workspace folders cannot: a single-folder
    /// pnpm monorepo has one folder and many configured projects, so a
    /// folder-derived root names `/ws` for `/ws/packages/app/src/App.vue.tsx`
    /// and the extension host then looks for TypeScript in `/ws/node_modules`
    /// only — reporting a perfectly good `packages/app/node_modules/typescript`
    /// absent, and (with the fail-closed contract in force) disabling the
    /// provider for that package. So ownership comes from the shared
    /// provider-neutral authority, and folder membership is only the
    /// last-resort answer for a file no configured project claims at all.
    ///
    /// The config path travels WITH the root because the root cannot imply it:
    /// one directory holds several configured projects (`tsconfig.app.json` +
    /// `tsconfig.node.json`) with different options, and a project may be
    /// configured by `jsconfig.json` or any `tsconfig.*.json`. Without it the
    /// consumer has to search for a literally-named `tsconfig.json` and invent
    /// defaults when it finds none. In the folder last-resort case no configured
    /// owner is known, so NO config is declared — the consumer discovers one
    /// itself rather than being handed a guess.
    ///
    /// THREE outcomes, not two — see [`FileProjectBinding`]. "No authority yet"
    /// and "the authority says nobody owns this" are different facts and get
    /// different answers; conflating them is what turns a terminal `NoProject`
    /// into a served, invented project.
    pub(super) fn project_binding_for(&self, file: &str) -> FileProjectBinding {
        let authority = self.ownership.read().clone();
        match authority {
            Some(authority) => match authority.configured_owner(file) {
                ProjectOwnership::Owned(owner) => FileProjectBinding::Configured {
                    root: owner.root,
                    config: owner.config_path,
                },
                ProjectOwnership::NoProject => FileProjectBinding::Unowned,
            },
            None => {
                let roots = self.project_roots.read();
                FileProjectBinding::Bootstrap {
                    root: verter_span::path::longest_project_root(
                        file,
                        &roots,
                        &self.workspace_root,
                    )
                    .to_string(),
                }
            }
        }
    }

    /// The `open` / `updateOpen` envelope fields for `file`, or the fail-closed
    /// error for a file no configured project owns.
    pub(super) fn declared_project_for(
        &self,
        file: &str,
    ) -> Result<(String, Option<String>), TypeProviderError> {
        match self.project_binding_for(file) {
            FileProjectBinding::Configured { root, config } => Ok((root, Some(config))),
            FileProjectBinding::Bootstrap { root } => Ok((root, None)),
            FileProjectBinding::Unowned => Err(unowned_file_error(file)),
        }
    }
}

/// What the provider may declare to the extension host for one file.
///
/// Visible to the parent module (which matches on it in the resync sweep).
///
/// The bootstrap state is the ABSENCE of an authority, not an authority that
/// answered "nobody". Before init publishes the exact workspace snapshot only
/// the editor's folders are known, and a folder-derived root is the honest
/// last-resort answer — superseded by [`TypeProvider::resync_open_files`] as
/// soon as the authority lands. Once it HAS landed, an unclaimed file is the
/// contract's terminal `NoProject`: it is never re-derived from folders, because
/// the folder is not a project and serving it means answering under another
/// project's TypeScript, options and aliases.
pub(super) enum FileProjectBinding {
    /// A configured project owns the file: its root and its defining config.
    Configured { root: String, config: String },
    /// No authority published yet — the workspace folder, with no config
    /// asserted.
    Bootstrap { root: String },
    /// The authority is live and no configured project claims the file.
    Unowned,
}

/// The fail-closed error for a file the ownership authority does not place in
/// any configured project.
fn unowned_file_error(file: &str) -> TypeProviderError {
    TypeProviderError::new(format!(
        "no configured TypeScript project includes {file}, so the extension type provider \
         has no project to serve it from. Add it to a tsconfig.json `include`/`files`, or \
         open it inside a configured project."
    ))
}
