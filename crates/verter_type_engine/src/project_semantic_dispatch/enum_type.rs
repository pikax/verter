//! The checker's enum model.
//!
//! Every member of an enum declaration has its own NOMINAL literal type
//! ([`SemanticNodeData::EnumLiteral`]): `E.A` is identified by the enum's
//! type declaration and the member's name, and stands for the member's
//! value (its base). The enum's type is the union of its members' literal
//! types — the one member's literal type when the enum declares one
//! member — so `E.A | E.B` over a two-member `E` IS `E`.
//!
//! One inventory feeds every surface: the prepared value declaration's
//! member facts, merged across every declaration of a merged enum — the
//! blocks of one file, and for a script's global enum every script's
//! blocks in program order ([`ProjectSemanticDispatch::enum_blocks`]). The
//! type-position `E` and `E.A`, the enum object `typeof E` and the widening
//! of a fresh `E.A` all read it through [`EnumDeclaration`], so the members
//! a type names and the members the object carries are the same nodes.

use std::sync::Arc;

use verter_session_query::enum_constant::{
    evaluate_enum_constant, global_number_spelling, EnumConstant,
};
use verter_session_query::type_solver::host::ResolvedRootIdentity;
use verter_type_expr::facts::{EnumConstantStep, EnumMemberEntry, EnumPrimitiveDomain, EnumScalar};

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, EnumLiteralType, LiteralValue, NodeScopeId, PrimitiveKind, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, ValueRootKey,
};

/// One enum declaration: the identity its type carries and its members, in
/// declaration order across every declaration of a merged enum.
pub(super) struct EnumDeclaration {
    /// The enum's type declaration — the identity every member literal
    /// carries and a reference to the enum resolves to. For an enum merged
    /// across files, its first file's declaration, whichever file the
    /// reference reached.
    pub(super) decl: DeclIdentity,
    /// The members, in declaration order.
    pub(super) members: Arc<[EnumMemberEntry]>,
    /// The files declaring the members, in the members' order: each file's
    /// members are the next `len` entries of [`Self::members`].
    blocks: Arc<[EnumBlock]>,
}

/// The members one file declares of an enum: an initializer written in
/// them reads names in that file's scope.
#[derive(Clone)]
struct EnumBlock {
    canonical: Arc<str>,
    owner: verter_type_expr::TopLevelOwnerId,
    /// How many of the enum's members, in order, the file declares.
    len: usize,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The enum the VALUE declaration `symbol` in `canonical`'s `owner`
    /// scope declares, following the export-target chase a `typeof`
    /// reference takes, so a re-exported enum is its declaring file's.
    /// `None` when the declaration is not an enum.
    pub(super) fn enum_declaration(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol: &str,
    ) -> Option<EnumDeclaration> {
        let (declaring_canonical, declaring_owner, declaring_symbol, prepared) =
            self.effective_prepared_value_decl(canonical, owner, symbol)?;
        Self::enum_declaration_of(
            self,
            declaring_canonical,
            declaring_owner,
            declaring_symbol,
            &prepared,
        )
    }

    /// The enum declared AT `symbol` in `canonical`'s `owner` scope — no
    /// export chase: the value declaration beside a type declaration of
    /// the same name. `None` when that value is not an enum.
    pub(super) fn enum_declared_at(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol: &str,
    ) -> Option<EnumDeclaration> {
        let prepared = self
            .ctx
            .prepared_value_decl_return_only(canonical, owner, symbol)?;
        Self::enum_declaration_of(
            self,
            Arc::from(canonical),
            owner,
            Arc::from(symbol),
            &prepared,
        )
    }

    pub(super) fn enum_declaration_of(
        &self,
        declaring_canonical: Arc<str>,
        declaring_owner: verter_type_expr::TopLevelOwnerId,
        declaring_symbol: Arc<str>,
        prepared: &verter_session_query::type_solver::PreparedValueDecl,
    ) -> Option<EnumDeclaration> {
        let enumeration = self.enum_blocks(
            declaring_canonical,
            declaring_owner,
            declaring_symbol,
            prepared,
        )?;
        if enumeration
            .members
            .iter()
            .all(|entry| entry.initializer.is_none())
        {
            return Some(enumeration);
        }
        // A member whose value depends on another declaration is evaluated
        // now that the declaration can be read.
        let mut evaluation = EnumEvaluation::default();
        evaluation.read_enum(&enumeration);
        let members: Vec<EnumMemberEntry> = enumeration
            .members
            .iter()
            .map(|entry| {
                if entry.initializer.is_none() {
                    return entry.clone();
                }
                let key = (
                    enumeration.decl.clone(),
                    Arc::<str>::from(entry.name.as_str()),
                );
                let value = self.enum_member_constant(&mut evaluation, key).map_or(
                    EnumScalar::Primitive(EnumPrimitiveDomain::Number),
                    |constant| constant.to_scalar(),
                );
                EnumMemberEntry {
                    name: entry.name.clone(),
                    value,
                    initializer: None,
                }
            })
            .collect();
        Some(EnumDeclaration {
            members: Arc::from(members.into_boxed_slice()),
            ..enumeration
        })
    }

    /// The constant value of the member `key` names, evaluating its pending
    /// initializer and every pending initializer it reads through, each at
    /// most once per `evaluation`; `None` for a computed member.
    ///
    /// The members an initializer reads are evaluated from an explicit
    /// stack before it, so a chain of references of any length evaluates
    /// without a native frame per link. A member reached again while its
    /// own initializer is being evaluated is circular — a reference the
    /// checker reads before the member is computed — and no constant.
    fn enum_member_constant(
        &self,
        evaluation: &mut EnumEvaluation,
        key: EnumMemberKey,
    ) -> Option<EnumConstant> {
        /// One member on the stack: the targets of its initializer's
        /// references once they are resolved (and the members among them
        /// pushed above it).
        struct Frame {
            key: EnumMemberKey,
            targets: Option<Vec<ReferenceTarget>>,
        }
        let root = key.clone();
        let mut stack = vec![Frame { key, targets: None }];
        while let Some(frame) = stack.pop() {
            if let Some(MemberState::Done(_)) = evaluation.states.get(&frame.key) {
                continue;
            }
            let Some(entry) = evaluation.member(&frame.key).cloned() else {
                evaluation.states.insert(frame.key, MemberState::Done(None));
                continue;
            };
            let Some(initializer) = entry.initializer.as_ref() else {
                let constant = EnumConstant::from_scalar(&entry.value);
                evaluation
                    .states
                    .insert(frame.key, MemberState::Done(constant));
                continue;
            };
            match frame.targets {
                None => {
                    evaluation
                        .states
                        .insert(frame.key.clone(), MemberState::InProgress);
                    let targets: Vec<ReferenceTarget> = initializer
                        .steps
                        .iter()
                        .filter_map(|step| match step {
                            EnumConstantStep::Reference(path) => {
                                Some(self.enum_reference_target(evaluation, &frame.key, path))
                            }
                            _ => None,
                        })
                        .collect();
                    let unread: Vec<EnumMemberKey> = targets
                        .iter()
                        .filter_map(|target| match target {
                            ReferenceTarget::Member(member)
                                if !evaluation.states.contains_key(member) =>
                            {
                                Some(member.clone())
                            }
                            _ => None,
                        })
                        .collect();
                    stack.push(Frame {
                        key: frame.key,
                        targets: Some(targets),
                    });
                    stack.extend(
                        unread
                            .into_iter()
                            .rev()
                            .map(|key| Frame { key, targets: None }),
                    );
                }
                Some(targets) => {
                    #[cfg(any(test, feature = "test-support"))]
                    ENUM_INITIALIZER_EVALUATIONS.with(|count| count.set(count.get() + 1));
                    let mut targets = targets.into_iter();
                    let constant = evaluate_enum_constant(initializer, |_| {
                        match targets.next()? {
                            ReferenceTarget::Member(member) => match evaluation.states.get(&member)
                            {
                                Some(MemberState::Done(constant)) => constant.clone(),
                                // Still being evaluated: the reference is
                                // circular.
                                Some(MemberState::InProgress) | None => None,
                            },
                            ReferenceTarget::Constant(constant) => Some(constant),
                            ReferenceTarget::Nothing => None,
                        }
                    });
                    evaluation
                        .states
                        .insert(frame.key, MemberState::Done(constant));
                }
            }
        }
        match evaluation.states.get(&root) {
            Some(MemberState::Done(constant)) => constant.clone(),
            Some(MemberState::InProgress) | None => None,
        }
    }

    /// What a path written in an initializer of the enum `enumeration`
    /// names: a bare name is first a member of the enum itself; otherwise
    /// the path resolves as a value reference written in the enum's body
    /// does — in each enclosing namespace, innermost first, then in the
    /// file — to another enum's member or to a `const` variable. `Infinity`
    /// and `NaN` are the global numbers when they resolve to the global
    /// declaration or to none, as the checker's evaluator reads them.
    ///
    /// A declaration of the enum's own file written after the member
    /// `user` whose initializer this is reads as the checker's
    /// declared-before-use rule reads it ([`Self::declared_after_use`]): a
    /// later enum member is `0` (TS2651), a later `const` no constant.
    fn enum_reference_target(
        &self,
        evaluation: &mut EnumEvaluation,
        user: &EnumMemberKey,
        path: &[String],
    ) -> ReferenceTarget {
        #[cfg(any(test, feature = "test-support"))]
        ENUM_INITIALIZER_EVALUATIONS.with(|count| count.set(count.get() + 1));
        let enumeration = &user.0;
        if let [member] = path {
            let key = (enumeration.clone(), Arc::<str>::from(member.as_str()));
            if evaluation.member(&key).is_some() {
                return self.member_target(evaluation, user, key);
            }
        }
        // The file whose block declares the member: its scope is the one
        // the initializer is written in.
        let Some(block) = evaluation.block(user).cloned() else {
            return ReferenceTarget::Nothing;
        };
        let namespaces: Vec<&str> = enumeration
            .decl_name
            .rsplit_once('.')
            .map(|(namespace, _)| namespace.split('.').collect())
            .unwrap_or_default();
        for depth in (0..=namespaces.len()).rev() {
            let candidate: Vec<Arc<str>> = namespaces[..depth]
                .iter()
                .map(|segment| Arc::from(*segment))
                .chain(path.iter().map(|segment| Arc::from(segment.as_str())))
                .collect();
            let Some((root, rest)) = candidate.split_first() else {
                continue;
            };
            let value_root = ValueRootKey {
                scope: crate::semantic_query::ScopeId {
                    canonical_id: Arc::clone(&block.canonical),
                    owner: block.owner,
                    local_scope: None,
                    binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(block.owner),
                },
                name: Arc::clone(root),
            };
            let resolved = if depth > 0 {
                self.namespace_private_value(evaluation, user, &candidate[..=depth])
                    .map(|identity| (identity, candidate[depth + 1..].to_vec()))
            } else {
                None
            };
            let Some((identity, remaining)) = resolved.or_else(|| {
                self.value_path_declaration(
                    &value_root,
                    rest,
                    &mut rustc_hash::FxHashSet::default(),
                )
            }) else {
                continue;
            };
            match remaining.as_slice() {
                [] => {
                    if let Some(global) = global_number_spelling(path) {
                        let is_global = self
                            .global_value_declarations(&block.canonical, &path[0])
                            .is_some_and(|declarations| {
                                declarations
                                    .iter()
                                    .any(|(declaration, _)| declaration == &identity)
                            });
                        if is_global {
                            return ReferenceTarget::Constant(global);
                        }
                    }
                    let declared = self.value_declaration_start(&identity);
                    if self.declared_after_use(
                        evaluation,
                        user,
                        &identity.canonical_id,
                        identity.owner,
                        declared,
                    ) {
                        return ReferenceTarget::Nothing;
                    }
                    return self
                        .const_variable_constant(&identity)
                        .map_or(ReferenceTarget::Nothing, ReferenceTarget::Constant);
                }
                [member] => {
                    if let Some(other) = self.enum_declaration_raw(&identity) {
                        evaluation.read_enum(&other);
                        let key = (other.decl, Arc::<str>::from(member.as_ref()));
                        if evaluation.member(&key).is_none() {
                            return ReferenceTarget::Nothing;
                        }
                        return self.member_target(evaluation, user, key);
                    }
                }
                _ => {}
            }
        }
        global_number_spelling(path).map_or(ReferenceTarget::Nothing, ReferenceTarget::Constant)
    }

    /// The member `key` as the target of a reference in `user`'s
    /// initializer: `0` when it is declared after its use, the checker's
    /// value for a member referenced before its declaration (TS2651).
    fn member_target(
        &self,
        evaluation: &mut EnumEvaluation,
        user: &EnumMemberKey,
        key: EnumMemberKey,
    ) -> ReferenceTarget {
        let declared = self
            .enum_member_position(evaluation, &key)
            .map(|position| position.start);
        let Some(block) = evaluation.block(&key).cloned() else {
            return ReferenceTarget::Member(key);
        };
        if self.declared_after_use(evaluation, user, &block.canonical, block.owner, declared) {
            ReferenceTarget::Constant(EnumConstant::Number(0.0))
        } else {
            ReferenceTarget::Member(key)
        }
    }

    /// The private value member `qualified` (`N.k`) of the namespace
    /// `user`'s enum is declared in, as a reference in the member's
    /// initializer names it: a class, enum, variable or function the
    /// namespace does not export is visible inside the block that declares
    /// it, and only there (another block of a merged namespace does not see
    /// it). `None` for any other name.
    fn namespace_private_value(
        &self,
        evaluation: &mut EnumEvaluation,
        user: &EnumMemberKey,
        qualified: &[Arc<str>],
    ) -> Option<ResolvedRootIdentity> {
        let used = self.enum_member_position(evaluation, user)?;
        let file = evaluation.block(user)?.clone();
        let name = qualified.join(".");
        let indexed = self
            .ctx
            .ensure_indexed_ready_serve(file.canonical.as_ref())?
            .indexed;
        let block = *indexed
            .shallow_state
            .headers
            .namespace_private_value_blocks
            .get(&verter_type_expr::facts::DeclBindingKey::new(
                file.owner,
                name.as_str(),
            ))?;
        (block.start <= used.start && used.start < block.end).then(|| {
            ResolvedRootIdentity::new_in_owner(
                Arc::clone(&file.canonical),
                file.owner,
                name.as_str(),
            )
        })
    }

    /// Whether a declaration of `canonical`'s `owner` starting at
    /// `declared` is written after the member `user` reads it — the
    /// checker's `isBlockScopedNameDeclaredBeforeUse` for an enum member
    /// initializer. Only a declaration of the file declaring the user has
    /// an order (another file's never follows it), and a use in an ambient
    /// context reads every declaration as declared before it.
    fn declared_after_use(
        &self,
        evaluation: &mut EnumEvaluation,
        user: &EnumMemberKey,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        declared: Option<u32>,
    ) -> bool {
        if !evaluation
            .block(user)
            .is_some_and(|block| block.canonical.as_ref() == canonical && block.owner == owner)
        {
            return false;
        }
        let Some(declared) = declared else {
            return false;
        };
        self.enum_member_position(evaluation, user)
            .is_some_and(|used| !used.ambient && declared > used.start)
    }

    /// Where the member `key` names is declared, read off its file's
    /// enum header (the positions of an enum read once per evaluation).
    /// `None` when the header records no positions for the member list the
    /// evaluation read.
    fn enum_member_position(
        &self,
        evaluation: &mut EnumEvaluation,
        key: &EnumMemberKey,
    ) -> Option<verter_session_query::declarations::headers::EnumMemberPosition> {
        let slot = *evaluation.enums.get(&key.0)?.positions.get(&key.1)?;
        if !evaluation.header_positions.contains_key(&key.0) {
            let read = &evaluation.enums[&key.0];
            let positions = self.enum_header_positions(&key.0, &read.members, &read.blocks);
            evaluation.header_positions.insert(key.0.clone(), positions);
        }
        evaluation
            .header_positions
            .get(&key.0)?
            .as_ref()?
            .get(slot)
            .copied()
    }

    /// The declared positions of `decl`'s members, in the order of
    /// `members`, each read off the enum header of the file whose block
    /// declares it — `None` when a file's enum header does not list exactly
    /// its block's members.
    fn enum_header_positions(
        &self,
        decl: &DeclIdentity,
        members: &[EnumMemberEntry],
        blocks: &[EnumBlock],
    ) -> Option<Arc<[verter_session_query::declarations::headers::EnumMemberPosition]>> {
        let mut positions = Vec::with_capacity(members.len());
        let mut rest = members;
        for block in blocks {
            let (block_members, after) = rest.split_at_checked(block.len)?;
            rest = after;
            let indexed = self
                .ctx
                .ensure_indexed_ready_serve(block.canonical.as_ref())?
                .indexed;
            let header = indexed.shallow_state.headers.enum_headers.get(
                &verter_type_expr::facts::DeclBindingKey::new(block.owner, decl.decl_name.as_ref()),
            )?;
            if header.member_positions.len() != block_members.len()
                || !header
                    .member_names
                    .iter()
                    .zip(block_members)
                    .all(|(name, entry)| *name == entry.name)
            {
                return None;
            }
            positions.extend_from_slice(&header.member_positions);
        }
        rest.is_empty()
            .then(|| Arc::from(positions.into_boxed_slice()))
    }

    /// Where the value declaration `identity` names starts in its file,
    /// read off the file's value header.
    fn value_declaration_start(&self, identity: &ResolvedRootIdentity) -> Option<u32> {
        let indexed = self
            .ctx
            .ensure_indexed_ready_serve(identity.canonical_id.as_ref())?
            .indexed;
        indexed
            .shallow_state
            .headers
            .value_header_in(identity.owner, &identity.symbol_name)
            .map(|header| header.span.start)
    }

    /// The enum a resolved value declaration declares, its pending members
    /// left unevaluated.
    fn enum_declaration_raw(&self, identity: &ResolvedRootIdentity) -> Option<EnumDeclaration> {
        let (canonical, owner, symbol, prepared) = self.effective_prepared_value_decl(
            &identity.canonical_id,
            identity.owner,
            &identity.symbol_name,
        )?;
        self.enum_blocks(canonical, owner, symbol, &prepared)
    }

    /// The enum the value declaration `prepared` (`symbol` in
    /// `canonical`'s `owner` scope) declares, its pending members left
    /// unevaluated; `None` when it is not an enum.
    ///
    /// A script's top-level enum is GLOBAL, and every script declaring an
    /// enum of its name declares a block of the same enum: its members are
    /// every such file's, files in declaration precedence order (the
    /// configured file sequence, then the canonical path — the order
    /// [`Self::global_value_declarations`] lists them in), and its identity
    /// is the first file's declaration, so every block, and every reference
    /// reaching any one of them, names the one enum. Any other enum's
    /// blocks are its own file's, which its prepared declaration already
    /// merges.
    fn enum_blocks(
        &self,
        canonical: Arc<str>,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol: Arc<str>,
        prepared: &verter_session_query::type_solver::PreparedValueDecl,
    ) -> Option<EnumDeclaration> {
        use verter_session_query::declarations::ValueDeclKind;
        use verter_session_query::inputs::contributors::{ContributorOrigin, FileModuleKind};
        if prepared.kind != ValueDeclKind::Enum {
            return None;
        }
        let own_members = Arc::clone(&prepared.enum_members.as_ref()?.members);
        let single = |canonical: Arc<str>, members: Arc<[EnumMemberEntry]>| EnumDeclaration {
            decl: DeclIdentity {
                whole_hash: self.decl_whole_hash(&canonical),
                canonical_id: Arc::clone(&canonical),
                owner,
                decl_name: Arc::clone(&symbol),
            },
            blocks: Arc::from([EnumBlock {
                canonical,
                owner,
                len: members.len(),
            }]),
            members,
        };
        let is_script = !symbol.contains('.')
            && self
                .ctx
                .ensure_indexed_ready_serve(canonical.as_ref())
                .is_some_and(|serve| {
                    verter_session_query::inputs::contributors::classify_shallow_module_kind(
                        &serve.indexed.shallow_state,
                    ) == FileModuleKind::Script
                });
        if !is_script {
            return Some(single(canonical, own_members));
        }
        let population = self.global_contributors_in(
            canonical.as_ref(),
            symbol.as_ref(),
            verter_session_query::facts::SymbolSpace::Value,
        );
        let mut files: Vec<&verter_session_query::inputs::contributors::ContributorEntry> =
            population
                .entries
                .iter()
                .filter(|entry| {
                    !entry.is_automatic_lib
                        && entry.origin == ContributorOrigin::FileScopeValue
                        && entry.module_kind == FileModuleKind::Script
                })
                .collect();
        if !files.iter().any(|entry| {
            entry.artifact_key.canonical.as_ref() == canonical.as_ref() && entry.owner == owner
        }) || files.len() < 2
        {
            return Some(single(canonical, own_members));
        }
        files.sort_by(|left, right| {
            self.ctx
                .declaration_sequence_rank(left.artifact_key.canonical.as_ref())
                .cmp(
                    &self
                        .ctx
                        .declaration_sequence_rank(right.artifact_key.canonical.as_ref()),
                )
                .then_with(|| {
                    left.artifact_key
                        .canonical
                        .as_ref()
                        .cmp(right.artifact_key.canonical.as_ref())
                })
                .then_with(|| left.parse_stable_hash.cmp(&right.parse_stable_hash))
        });
        let mut members: Vec<EnumMemberEntry> = Vec::new();
        let mut blocks: Vec<EnumBlock> = Vec::with_capacity(files.len());
        for entry in files {
            let file = &entry.artifact_key.canonical;
            let file_members = if file.as_ref() == canonical.as_ref() && entry.owner == owner {
                Arc::clone(&own_members)
            } else {
                // A file declaring the name as anything but an enum (a
                // namespace merging with it) adds no member.
                match self
                    .ctx
                    .prepared_value_decl_return_only(file, entry.owner, symbol.as_ref())
                {
                    Some(other) if other.kind == ValueDeclKind::Enum => {
                        match other.enum_members.as_ref() {
                            Some(fact) => Arc::clone(&fact.members),
                            None => continue,
                        }
                    }
                    _ => continue,
                }
            };
            if blocks.iter().any(|block| {
                block.canonical.as_ref() == file.as_ref() && block.owner == entry.owner
            }) {
                continue;
            }
            // The enum's members are this file's too: a warm answer that
            // read them is valid only while the file is unchanged.
            if crate::resolver_core::resolver_context::fact_tracer_installed() {
                crate::resolver_core::resolver_context::observe_fan_out(
                    verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                        canonical_id: file.to_string(),
                        hash: self.decl_whole_hash(file),
                    },
                );
            }
            members.extend(file_members.iter().cloned());
            blocks.push(EnumBlock {
                canonical: Arc::clone(file),
                owner: entry.owner,
                len: file_members.len(),
            });
        }
        let first = blocks.first()?;
        Some(EnumDeclaration {
            decl: DeclIdentity {
                whole_hash: self.decl_whole_hash(&first.canonical),
                canonical_id: Arc::clone(&first.canonical),
                owner: first.owner,
                decl_name: symbol,
            },
            members: Arc::from(members.into_boxed_slice()),
            blocks: Arc::from(blocks.into_boxed_slice()),
        })
    }

    /// The whole-file hash a declaration identity in `canonical` carries.
    fn decl_whole_hash(&self, canonical: &str) -> crate::semantic_query::HashValue {
        self.ctx
            .shallow_file_state(canonical)
            .map_or(crate::semantic_query::HashValue::default(), |state| {
                state.whole_hash
            })
    }

    /// The constant value of a `const` variable an enum initializer names:
    /// an unannotated `const` whose declared type is the literal its
    /// initializer evaluates to (`const k = 5`, `const c = k`). Any other
    /// variable — annotated, mutable, or of a non-literal type — names no
    /// constant.
    fn const_variable_constant(&self, identity: &ResolvedRootIdentity) -> Option<EnumConstant> {
        use verter_type_expr::facts::DeclaredLiteralFreshness;
        let (canonical, owner, symbol, prepared) = self.effective_prepared_value_decl(
            &identity.canonical_id,
            identity.owner,
            &identity.symbol_name,
        )?;
        if prepared.kind != verter_session_query::declarations::ValueDeclKind::Const
            || !matches!(
                prepared.type_annotation.literal_freshness,
                DeclaredLiteralFreshness::Widening | DeclaredLiteralFreshness::Follows(_)
            )
        {
            return None;
        }
        // The declaration itself, under its qualified name (`N.k`): a
        // namespace's private member is no member a qualified read names.
        let path: Arc<[Arc<str>]> = Arc::from(Vec::new().into_boxed_slice());
        let value_root = ValueRootKey {
            scope: crate::semantic_query::ScopeId {
                canonical_id: canonical,
                owner,
                local_scope: None,
                binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(owner),
            },
            name: symbol,
        };
        let QueryResult::Value(crate::semantic_query::SemanticQueryOutput { value, .. }) = self
            .execute_type_node(self.typeof_key_with_path(
                value_root,
                path,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            ))
        else {
            return None;
        };
        match self.graph().node_data(value).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::Number(number))) => {
                Some(EnumConstant::Number(*number))
            }
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => {
                Some(EnumConstant::String(text.clone()))
            }
            _ => None,
        }
    }

    /// The literal type of `enumeration`'s member `member`; `None` when the
    /// enum declares no such member.
    pub(super) fn enum_member_type(
        &self,
        enumeration: &EnumDeclaration,
        member: &str,
    ) -> Option<SemanticNodeId> {
        let entry = enumeration
            .members
            .iter()
            .find(|entry| entry.name == member)?;
        Some(self.enum_member_literal(enumeration, entry))
    }

    /// The literal type of one member of `enumeration`.
    pub(super) fn enum_member_literal(
        &self,
        enumeration: &EnumDeclaration,
        entry: &EnumMemberEntry,
    ) -> SemanticNodeId {
        let base = self.enum_member_base(&entry.value);
        let scope = self.enum_scope(enumeration);
        self.graph().intern_node_with_scope(
            SemanticNodeData::EnumLiteral(EnumLiteralType {
                enum_decl: enumeration.decl.clone(),
                member: Arc::from(entry.name.as_str()),
                base,
                member_count: u32::try_from(enumeration.members.len()).unwrap_or(u32::MAX),
            }),
            scope,
        )
    }

    /// The enum's type: the union of its members' literal types, the one
    /// member's literal type for a one-member enum.
    pub(super) fn enum_type(&self, enumeration: &EnumDeclaration) -> SemanticNodeId {
        let members: Vec<SemanticNodeId> = enumeration
            .members
            .iter()
            .map(|entry| self.enum_member_literal(enumeration, entry))
            .collect();
        self.intern_normalized_union_or_intersection(&members, true)
    }

    /// The enum's object (`typeof E`): one readonly property per member,
    /// typed by the member's literal type, and — when a member's value is
    /// numeric or computed — the reverse mapping `readonly [x: number]:
    /// string` the checker gives it (`resolveAnonymousTypeMembers`'s
    /// `enumNumberIndexInfo`), so `E[0]` reads a member's name.
    pub(super) fn enum_object(&self, enumeration: &EnumDeclaration) -> SemanticNodeId {
        let reverse_mapping = enumeration.members.iter().any(|entry| {
            matches!(
                entry.value,
                EnumScalar::Number(_)
                    | EnumScalar::Primitive(
                        EnumPrimitiveDomain::Number | EnumPrimitiveDomain::NumberOrString
                    )
            )
        });
        let index_signature = reverse_mapping.then(|| {
            let graph = self.graph();
            crate::semantic_query::SurfaceEntry::IndexSignature(
                crate::semantic_query::IndexSignature {
                    key_type: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number)),
                    value_type: graph
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
                    readonly: true,
                    spans: verter_type_expr::IndexSignatureSpans::default(),
                    declaration_origin: Some(Arc::clone(&enumeration.decl.canonical_id)),
                },
            )
        });
        let members = enumeration
            .members
            .iter()
            .map(|entry| crate::semantic_query::SurfaceMember {
                key: crate::semantic_query::AuthoredPropertyKey::String(Arc::from(
                    entry.name.as_str(),
                )),
                value: self.enum_member_literal(enumeration, entry),
                optional: false,
                readonly: true,
                method_kind: None,
                has_implementation_body: false,
                visibility: verter_type_expr::MemberVisibility::Public,
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                // The member inventory carries no per-member source span.
                spans: verter_type_expr::MemberSpans::default(),
                declaration_origin: Some(Arc::clone(&enumeration.decl.canonical_id)),
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            })
            .map(crate::semantic_query::SurfaceEntry::Member)
            .chain(index_signature)
            .collect();
        self.graph().intern_node_with_scope(
            SemanticNodeData::Object(crate::semantic_query::SurfaceView::from_entries(
                members,
                None,
                reverse_mapping,
            )),
            self.enum_scope(enumeration),
        )
    }

    /// Whether `node` is the object of the enum `literal` is a member of —
    /// the enum object itself, or a `typeof` reference resolving to it.
    pub(super) fn is_enum_object_of(
        &self,
        node: SemanticNodeId,
        literal: &EnumLiteralType,
    ) -> bool {
        let Some(enumeration) = self.enum_declaration(
            literal.enum_decl.canonical_id.as_ref(),
            literal.enum_decl.owner,
            literal.enum_decl.decl_name.as_ref(),
        ) else {
            return false;
        };
        let object = self.enum_object(&enumeration);
        if node == object {
            return true;
        }
        matches!(
            self.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::TypeOf(_))
        ) && self
            .evaluate_deferred_semantic_node_with_context(
                node,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            )
            .into_active_query_build_node(self)
            == object
    }

    /// Whether `node` is an enum's object itself (`typeof E`): its reverse
    /// mapping is no key of it, as the checker leaves `enumNumberIndexInfo`
    /// out of `keyof` (`keyof typeof E` is its member names).
    pub(super) fn is_enum_object(&self, node: SemanticNodeId) -> bool {
        let literal = match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Object(surface)) => {
                match surface
                    .positive_members()
                    .first()
                    .map(|member| member.value)
                {
                    Some(value) => match self.graph().node_data(value).as_deref() {
                        Some(SemanticNodeData::EnumLiteral(literal)) => literal.clone(),
                        _ => return false,
                    },
                    None => return false,
                }
            }
            _ => return false,
        };
        self.enum_declaration(
            literal.enum_decl.canonical_id.as_ref(),
            literal.enum_decl.owner,
            literal.enum_decl.decl_name.as_ref(),
        )
        .is_some_and(|enumeration| self.enum_object(&enumeration) == node)
    }

    fn enum_scope(&self, enumeration: &EnumDeclaration) -> NodeScopeId {
        NodeScopeId::File {
            canonical_id: Arc::clone(&enumeration.decl.canonical_id),
            owner: enumeration.decl.owner,
            whole_hash: enumeration.decl.whole_hash,
            local_scope: None,
        }
    }

    /// The type a FRESH literal widens to (the checker's widened literal
    /// type): a plain literal widens to its primitive, an enum member's
    /// literal to its enum's type. Every other node is its own widening.
    pub(super) fn widened_literal(&self, node: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let primitive = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Literal(literal)) => match literal {
                LiteralValue::String(_) => PrimitiveKind::String,
                LiteralValue::Number(_) => PrimitiveKind::Number,
                LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
                LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
            },
            Some(SemanticNodeData::EnumLiteral(literal)) => {
                return self.enum_literal_base_type(literal).unwrap_or(node);
            }
            _ => return node,
        };
        graph.intern_node(SemanticNodeData::Primitive(primitive))
    }

    /// The type a FRESH enum member literal widens to: its enum's type
    /// (the checker's base type of an enum literal). `None` when the enum
    /// no longer resolves.
    pub(super) fn enum_literal_base_type(
        &self,
        literal: &EnumLiteralType,
    ) -> Option<SemanticNodeId> {
        let enumeration = self.enum_declaration(
            literal.enum_decl.canonical_id.as_ref(),
            literal.enum_decl.owner,
            literal.enum_decl.decl_name.as_ref(),
        )?;
        Some(self.enum_type(&enumeration))
    }

    /// The value a member stands for: the number or string literal of a
    /// constant member, the primitive domain of a member whose value is not
    /// a constant.
    fn enum_member_base(&self, value: &EnumScalar) -> SemanticNodeId {
        let graph = self.graph();
        match value {
            EnumScalar::Number(text) => {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(
                    text.parse::<f64>()
                        .expect("EnumScalar::Number stores the canonical f64 display string"),
                )))
            }
            EnumScalar::String(text) => graph.intern_node(SemanticNodeData::Literal(
                LiteralValue::String(text.clone()),
            )),
            EnumScalar::Primitive(domain) => match domain {
                EnumPrimitiveDomain::Number => {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number))
                }
                EnumPrimitiveDomain::String => {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String))
                }
                EnumPrimitiveDomain::NumberOrString => {
                    let number =
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                    let string =
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    self.intern_normalized_union_or_intersection(&[number, string], true)
                }
                EnumPrimitiveDomain::Unknown => {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown))
                }
            },
        }
    }
}

/// Whether `node` is a LITERAL type — a plain literal or an enum member's
/// literal — the types a fresh value carries and a widening position
/// widens.
pub(super) fn is_literal_type(
    graph: &impl crate::semantic_query::NodeRead,
    node: SemanticNodeId,
) -> bool {
    matches!(
        graph.node_data(node).as_deref(),
        Some(SemanticNodeData::Literal(_) | SemanticNodeData::EnumLiteral(_))
    )
}

/// One enum member an evaluation reads: its enum's declaration and its name.
type EnumMemberKey = (DeclIdentity, Arc<str>);

/// What a reference in a pending initializer names.
enum ReferenceTarget {
    /// A member of an enum, whose value the evaluation reads.
    Member(EnumMemberKey),
    /// A constant: a `const` variable's value, or a global number.
    Constant(EnumConstant),
    /// No constant.
    Nothing,
}

/// Where one member's evaluation stands.
enum MemberState {
    /// Its initializer is being evaluated: reached again, it is circular.
    InProgress,
    /// Its constant, `None` for a computed member.
    Done(Option<EnumConstant>),
}

/// One evaluation of an enum's pending members: every enum its
/// initializers read, with each member's position by name, and every
/// member evaluated or being evaluated. It is owned by one
/// [`ProjectSemanticDispatch::enum_declaration_of`] call and dropped when
/// that call returns; nothing of it outlives the call.
#[derive(Default)]
struct EnumEvaluation {
    enums: rustc_hash::FxHashMap<DeclIdentity, ReadEnum>,
    states: rustc_hash::FxHashMap<EnumMemberKey, MemberState>,
    /// The declared positions of a read enum's members, read off its
    /// file's header when a reference first needs one.
    header_positions: rustc_hash::FxHashMap<
        DeclIdentity,
        Option<Arc<[verter_session_query::declarations::headers::EnumMemberPosition]>>,
    >,
}

/// An enum an evaluation reads: its members, pending ones unevaluated, and
/// the position of each member name (its first declaration).
struct ReadEnum {
    members: Arc<[EnumMemberEntry]>,
    blocks: Arc<[EnumBlock]>,
    positions: rustc_hash::FxHashMap<Arc<str>, usize>,
}

impl EnumEvaluation {
    /// Read the members of `enumeration` (once per enum).
    fn read_enum(&mut self, enumeration: &EnumDeclaration) {
        self.enums
            .entry(enumeration.decl.clone())
            .or_insert_with(|| ReadEnum {
                // Collected last to first, so a repeated name keeps its
                // first position.
                positions: enumeration
                    .members
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(position, entry)| (Arc::from(entry.name.as_str()), position))
                    .collect(),
                members: Arc::clone(&enumeration.members),
                blocks: Arc::clone(&enumeration.blocks),
            });
    }

    /// The file whose block declares the member `key` names, when its enum
    /// has been read and declares it.
    fn block(&self, key: &EnumMemberKey) -> Option<&EnumBlock> {
        let read = self.enums.get(&key.0)?;
        let mut slot = *read.positions.get(&key.1)?;
        read.blocks.iter().find(|block| {
            let inside = slot < block.len;
            slot = slot.saturating_sub(block.len);
            inside
        })
    }

    /// The member `key` names, when its enum has been read and declares it.
    fn member(&self, key: &EnumMemberKey) -> Option<&EnumMemberEntry> {
        let read = self.enums.get(&key.0)?;
        read.positions
            .get(&key.1)
            .map(|&position| &read.members[position])
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// How many initializer references this thread resolved and pending
    /// initializers it evaluated; test-only.
    static ENUM_INITIALIZER_EVALUATIONS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// How many initializer references this thread resolved, and pending
/// initializers it evaluated, so far (test-only).
#[cfg(any(test, feature = "test-support"))]
pub fn enum_initializer_work_for_tests() -> usize {
    ENUM_INITIALIZER_EVALUATIONS.with(std::cell::Cell::get)
}
