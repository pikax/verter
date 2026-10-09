//! The per-file declaration header index: type, value and enum headers, their contributors,
//! namespace and augmentation blocks. Built by the header walk.

use crate::analysis::top_level_owners::DeclMap;
use crate::declarations::headers::EnumMemberPosition;
use crate::declarations::{AugmentationScopeKind, TypeDeclKind, ValueDeclKind};
use rustc_hash::{FxHashMap, FxHashSet};
use verter_span::Span;
use verter_type_expr::facts::VueIgnoredHeritageFact;
use verter_type_expr::span_origins::DeclContributorAnchor;
use verter_type_expr::{
    AuthoredPropertyKey, DeclBindingKey, ObjectMethodKind, TopLevelOwnerId, TypeAuthoredPropertyKey,
};

/// Exact authored contributor record retained by the shallow header index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclHeaderContributor {
    pub anchor: DeclContributorAnchor,
    pub declaration_span: Span,
    pub name_span: Span,
}

/// A `namespace N { … }` block recorded by the shallow walk — one record
/// per namespace block with its dotted qualified name (`"NS"` /
/// `"NS.Inner"`), in source order. EMPTY blocks are recorded too: a
/// block with zero members is still a named lexical scope, and the
/// family-A binder-identity projection needs the complete scope
/// inventory (it must not derive scopes only from registered members).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceBlockRecord {
    pub owner: TopLevelOwnerId,
    pub qualified_name: String,
    pub span: Span,
    /// Whether the block is INSTANTIATED — it declares a value, exported
    /// or not (a variable, a function, a class, a non-`const` enum, a
    /// statement, an instantiated nested namespace) — so the namespace is
    /// a value too. A block holding only types, `const` enums and
    /// uninstantiated namespaces is none (the checker's TS2708).
    pub instantiated: bool,
}

/// A `declare module "X" { … }` / `declare global { … }` augmentation
/// block recorded by the shallow walk, in source order. EMPTY blocks
/// are recorded too (an empty augmentation block still introduces the
/// augmentation scope + target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AugmentationBlockRecord {
    pub scope: AugmentationScopeKind,
    pub owner: TopLevelOwnerId,
    pub span: Span,
    /// The name of the block's `export default function / class Name`
    /// declaration — what the module's `default` export names.
    pub default_export: Option<String>,
    /// The local name the block assigns the module to with `export = X`.
    pub export_assignment: Option<String>,
}

/// Exact parser-authored locator for a JSDoc typedef declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsdocTypedefHeader {
    pub owner: TopLevelOwnerId,
    pub attached_to: u32,
    pub comment_span: Span,
    pub name_span: Span,
    pub statement_index: Option<u32>,
    pub owner_local_ordinal: Option<u32>,
}

/// One direct syntactic member header: the member's name plus the
/// header-level flags the declaration states syntactically. No member
/// VALUE type is recorded — that is body data, lowered on demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberHeader {
    pub key: TypeAuthoredPropertyKey,
    pub method_kind: Option<ObjectMethodKind>,
    pub has_implementation_body: bool,
    pub optional: bool,
    pub readonly: bool,
}

impl MemberHeader {
    /// Borrow the ordinary string spelling when this header has a string key.
    #[must_use]
    pub fn string_name(&self) -> Option<&str> {
        self.key.as_string()
    }
}

/// Direct member headers in exact source order, with a key → position index.
///
/// Key identity is the authored key's exact equality: `1` and `"1"` are
/// distinct keys here, as are distinct `unique symbol` identities and
/// distinct computed keys. Every insertion is one index probe, so a wide
/// interface, type literal or object literal costs linear key work instead
/// of a scan of the members already recorded.
#[derive(Debug, Clone, Default)]
pub struct MemberHeaderList {
    members: Vec<MemberHeader>,
    positions: FxHashMap<TypeAuthoredPropertyKey, u32>,
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    key_probes: u64,
}

impl MemberHeaderList {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Union headers into the list: a key already present keeps its first
    /// header and position.
    pub fn extend_first_wins(&mut self, headers: impl IntoIterator<Item = MemberHeader>) {
        for header in headers {
            self.insert_first_wins(header);
        }
    }

    /// Union another list into this one, first-wins. An empty list takes
    /// `other` whole, keeping its index.
    pub fn union_first_wins(&mut self, other: MemberHeaderList) {
        if self.members.is_empty() {
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            let key_probes = self.key_probes;
            *self = other;
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            {
                self.key_probes += key_probes;
            }
            return;
        }
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            self.key_probes += other.key_probes;
        }
        self.extend_first_wins(other.members);
    }

    /// Insert one header unless its key is already present. Returns whether
    /// it was inserted.
    pub fn insert_first_wins(&mut self, header: MemberHeader) -> bool {
        self.count_key_probe();
        if self.positions.contains_key(&header.key) {
            return false;
        }
        self.count_key_probe();
        let next = self.next_position();
        self.positions.insert(header.key.clone(), next);
        self.members.push(header);
        true
    }

    /// Build the list where a repeated key's LAST header wins, at its last
    /// position: `{ a, b, a }` yields `b, a`.
    #[must_use]
    pub fn from_last_wins(headers: impl IntoIterator<Item = MemberHeader>) -> Self {
        let mut list = Self::default();
        let mut slots: Vec<Option<MemberHeader>> = Vec::new();
        for header in headers {
            list.count_key_probe();
            let slot = u32::try_from(slots.len()).expect("member count fits u32");
            if let Some(previous) = list.positions.insert(header.key.clone(), slot) {
                slots[previous as usize] = None;
            }
            slots.push(Some(header));
        }
        // Compact the vacated slots and remap each key's slot to its final
        // position.
        let mut final_position = vec![0_u32; slots.len()];
        list.members.reserve(list.positions.len());
        for (slot, header) in slots.into_iter().enumerate() {
            if let Some(header) = header {
                final_position[slot] = list.next_position();
                list.members.push(header);
            }
        }
        for position in list.positions.values_mut() {
            *position = final_position[*position as usize];
        }
        list
    }

    /// The header recorded under `key`.
    #[must_use]
    pub fn get(&self, key: &TypeAuthoredPropertyKey) -> Option<&MemberHeader> {
        self.positions
            .get(key)
            .map(|&position| &self.members[position as usize])
    }

    #[must_use]
    pub fn as_slice(&self) -> &[MemberHeader] {
        &self.members
    }

    /// Key-index operations (lookups and insertions) performed while
    /// building this list: at most two per offered header.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    #[must_use]
    pub fn key_probes(&self) -> u64 {
        self.key_probes
    }

    #[inline]
    fn count_key_probe(&mut self) {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            self.key_probes += 1;
        }
    }

    fn next_position(&self) -> u32 {
        u32::try_from(self.members.len()).expect("member count fits u32")
    }
}

impl PartialEq for MemberHeaderList {
    fn eq(&self, other: &Self) -> bool {
        self.members == other.members
    }
}

impl Eq for MemberHeaderList {}

impl std::ops::Deref for MemberHeaderList {
    type Target = [MemberHeader];

    fn deref(&self) -> &[MemberHeader] {
        &self.members
    }
}

impl<'a> IntoIterator for &'a MemberHeaderList {
    type Item = &'a MemberHeader;
    type IntoIter = std::slice::Iter<'a, MemberHeader>;

    fn into_iter(self) -> Self::IntoIter {
        self.members.iter()
    }
}

impl IntoIterator for MemberHeaderList {
    type Item = MemberHeader;
    type IntoIter = std::vec::IntoIter<MemberHeader>;

    fn into_iter(self) -> Self::IntoIter {
        self.members.into_iter()
    }
}

impl FromIterator<MemberHeader> for MemberHeaderList {
    /// First-wins collection.
    fn from_iter<T: IntoIterator<Item = MemberHeader>>(iter: T) -> Self {
        let mut list = Self::default();
        list.extend_first_wins(iter);
        list
    }
}

/// One type-parameter header: the parameter name plus the source locators
/// of its constraint / default clauses (the clauses themselves lower with
/// the body, on demand).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParamHeader {
    pub name: String,
    pub constraint_span: Option<Span>,
    pub default_span: Option<Span>,
}

/// Header record for one declared TYPE symbol (file scope or augmentation
/// scope): everything later stages need to ADDRESS the declaration without
/// lowering its body.
#[derive(Debug, Clone)]
pub struct TypeDeclHeader {
    pub kind: TypeDeclKind,
    /// Full declaration span of the LAST contributor (the last-wins
    /// representative, matching `TypeDeclGroup::primary`).
    pub span: Span,
    /// Name-identifier span of the last contributor.
    pub name_span: Span,
    /// Type-parameter headers, unioned across contributors in first-seen
    /// order (matching the lowered group's parameter-union rule).
    pub type_params: Vec<TypeParamHeader>,
    /// Direct syntactic member headers, unioned across contributors in
    /// first-seen order (matching `TypeDeclGroup::merged_member_header_facts`'
    /// own-member inventory: heritage members are NOT included).
    pub member_headers: MemberHeaderList,
    /// Source-order locators of every contributing declaration
    /// (deduplicated by `(statement anchor, declaration span)` — one
    /// statement can contribute several same-name declarations; DISTINCT
    /// declarations in one statement or block, e.g. a repeated
    /// `interface A` inside one `declare module`, are each recorded at
    /// their authored positions).
    pub contributors: Vec<DeclHeaderContributor>,
    /// Vue runtime-only heritage suppression, addressed against the exact
    /// lowered contributor/heritage-arm shape. This is parser-authored
    /// comment meaning captured once at indexing; consumers never rescan
    /// source text or comments.
    pub vue_ignored_heritage: Vec<VueIgnoredHeritageFact>,
    /// `true` when the name exists ONLY as a JSDoc `@typedef` (no TS
    /// declaration claimed it — TS-decl precedence applied at build).
    pub from_jsdoc_typedef: bool,
    /// Exact comment identity for a JSDoc-only typedef header.
    pub jsdoc_typedef: Option<JsdocTypedefHeader>,
}

/// Header record for one declared VALUE symbol.
#[derive(Debug, Clone)]
pub struct ValueDeclHeader {
    pub kind: ValueDeclKind,
    /// Full declaration span of the last contributor.
    pub span: Span,
    /// Name-identifier span of the last contributor.
    pub name_span: Span,
    /// Direct syntactic member headers of an object-literal initializer
    /// (`const x = { a, b }`) or a class's static members. Within one
    /// object literal a repeated key's last occurrence wins, at its last
    /// position; across contributors the first-seen key wins. Empty for
    /// other values.
    pub object_member_headers: MemberHeaderList,
    /// Source-order top-level statement indices of every contributing
    /// statement (deduplicated).
    pub contributors: Vec<DeclHeaderContributor>,
}

/// Header record for one `enum` declaration. The dedicated table carries
/// the member NAMES + statement locators for the member-presence facts rail
/// (each variant is a `MemberKind::EnumMember`). The enum is ALSO registered
/// as a dual-space type + value header (see [`index_enum`]) so it resolves
/// through the shared demand path; this table is the member-name authority,
/// not a sign that enums are absent from the value/type inventory.
#[derive(Debug, Clone)]
pub struct EnumDeclHeader {
    pub span: Span,
    pub name_span: Span,
    pub member_names: Vec<String>,
    /// Where each member of [`Self::member_names`] is declared, in the same
    /// order: the position the checker's declared-before-use rule compares
    /// an initializer's reference by. Empty in the `from_eval_env` mirror.
    pub member_positions: Vec<EnumMemberPosition>,
    pub contributors: Vec<DeclHeaderContributor>,
}

/// The shallow declaration-header index for one parsed program.
///
/// Augmentation tables are NESTED maps (`scope → name → header`) so a
/// scoped lookup needs no allocated tuple key.
#[derive(Debug, Clone, Default)]
pub struct DeclHeaderIndex {
    pub type_headers: DeclMap<TypeDeclHeader>,
    pub value_headers: DeclMap<ValueDeclHeader>,
    pub enum_headers: DeclMap<EnumDeclHeader>,
    pub augmentation_type_headers: FxHashMap<AugmentationScopeKind, DeclMap<TypeDeclHeader>>,
    pub augmentation_value_headers: FxHashMap<AugmentationScopeKind, DeclMap<ValueDeclHeader>>,
    /// Every `namespace N { … }` block in source order, EMPTY blocks
    /// included (recorded at block entry, before any member registers).
    /// Empty in the `from_eval_env` mirror (that env-seeded construction
    /// has no block-level view; consumers of the scope inventory go
    /// through the real parse path).
    pub namespace_blocks: Vec<NamespaceBlockRecord>,
    /// Every `declare module "X" { … }` / `declare global { … }` block
    /// in source order, EMPTY blocks included. Empty in the
    /// `from_eval_env` mirror (same block-level-view limitation).
    pub augmentation_blocks: Vec<AugmentationBlockRecord>,
    /// The file-scope class declarations authored `abstract` — the
    /// declaration fact behind the checker's "cannot create an instance of
    /// an abstract class" refusal of a `new` over the class's own
    /// construct signatures. Empty in the `from_eval_env` mirror (the
    /// lowered env carries no class modifiers).
    pub abstract_classes: FxHashSet<DeclBindingKey>,
    /// The accessibility each file-scope class declaration's FIRST
    /// constructor is authored with — the declaration fact behind the
    /// checker's `constructorVisibilitiesAreCompatible` over the class's
    /// construct signatures. A class that declares no constructor is
    /// absent (its construct signatures are its base's). Empty in the
    /// `from_eval_env` mirror.
    pub constructor_visibility: FxHashMap<DeclBindingKey, verter_type_expr::MemberVisibility>,
    /// The qualified names of namespace members their namespace does not
    /// export — a type, class or nested namespace declared without
    /// `export` in a namespace body that is not an export context. The
    /// qualified name serves references inside that body; a reference from
    /// outside it cannot name the member (the checker's TS2694). Empty in
    /// the `from_eval_env` mirror (same block-level-view limitation).
    pub namespace_private_members: FxHashSet<DeclBindingKey>,
    /// The block each private namespace VALUE member (a class, an enum, a
    /// variable or a function declared without `export`) is declared in:
    /// only a reference inside that block names it — another block of a
    /// merged namespace does not see it. Empty in the `from_eval_env`
    /// mirror (same block-level-view limitation).
    pub namespace_private_value_blocks: FxHashMap<DeclBindingKey, Span>,
    /// Every `namespace N { … }` block a `declare module "…"` block
    /// declares, with that block's scope, in source order. Empty in the
    /// `from_eval_env` mirror (same block-level-view limitation).
    pub augmentation_namespace_blocks: Vec<(AugmentationScopeKind, NamespaceBlockRecord)>,
    /// The one classification of every field of every class this walk
    /// indexes: which fields read through a synthetic value, and from what
    /// source. The class lowering and the function-program discovery read
    /// it instead of walking an initializer again. Empty in the
    /// `from_eval_env` mirror.
    pub class_field_values: std::sync::Arc<crate::declarations::class_fields::ClassFieldValues>,
}

impl DeclHeaderIndex {
    /// Whether a reference from outside the namespace body can name the
    /// qualified member `name` (`Ns.Inner.T`): neither it nor a namespace
    /// on its path is private to its enclosing namespace.
    #[must_use]
    pub fn namespace_member_is_exported(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        name.match_indices('.')
            .map(|(dot, _)| dot)
            .skip(1)
            .chain([name.len()])
            .all(|end| {
                !self
                    .namespace_private_members
                    .contains(&DeclBindingKey::new(owner, &name[..end]))
            })
    }

    /// Look up a file-scope type header.
    pub fn type_header(&self, name: &str) -> Option<&TypeDeclHeader> {
        self.type_header_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn type_header_in(&self, owner: TopLevelOwnerId, name: &str) -> Option<&TypeDeclHeader> {
        self.type_headers.get(&DeclBindingKey::new(owner, name))
    }

    /// Look up a file-scope value header.
    pub fn value_header(&self, name: &str) -> Option<&ValueDeclHeader> {
        self.value_header_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn value_header_in(&self, owner: TopLevelOwnerId, name: &str) -> Option<&ValueDeclHeader> {
        self.value_headers.get(&DeclBindingKey::new(owner, name))
    }

    /// Look up an augmentation-scoped type header (borrowed-key two-level
    /// lookup — no tuple allocation).
    pub fn augmentation_type_header(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&TypeDeclHeader> {
        self.augmentation_type_header_in(scope, TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn augmentation_type_header_in(
        &self,
        scope: &AugmentationScopeKind,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<&TypeDeclHeader> {
        self.augmentation_type_headers
            .get(scope)?
            .get(&DeclBindingKey::new(owner, name))
    }

    /// Look up an augmentation-scoped value header (borrowed-key two-level
    /// lookup — no tuple allocation).
    pub fn augmentation_value_header(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&ValueDeclHeader> {
        self.augmentation_value_header_in(scope, TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn augmentation_value_header_in(
        &self,
        scope: &AugmentationScopeKind,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<&ValueDeclHeader> {
        self.augmentation_value_headers
            .get(scope)?
            .get(&DeclBindingKey::new(owner, name))
    }

    /// The one `declare module "…" { … }` block scope that declares the
    /// type `(owner, name)`; `None` when none or several do.
    pub fn sole_module_augmentation_type_scope(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<&AugmentationScopeKind> {
        let key = DeclBindingKey::new(owner, name);
        let mut declaring = self
            .augmentation_type_headers
            .iter()
            .filter(|(scope, types)| {
                matches!(scope, AugmentationScopeKind::Module(_)) && types.contains_key(&key)
            })
            .map(|(scope, _)| scope);
        let scope = declaring.next()?;
        declaring.next().is_none().then_some(scope)
    }

    /// The one `declare module "…" { … }` block scope that declares the
    /// value `(owner, name)`; `None` when none or several do.
    pub fn sole_module_augmentation_value_scope(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<&AugmentationScopeKind> {
        let key = DeclBindingKey::new(owner, name);
        let mut declaring = self
            .augmentation_value_headers
            .iter()
            .filter(|(scope, values)| {
                matches!(scope, AugmentationScopeKind::Module(_)) && values.contains_key(&key)
            })
            .map(|(scope, _)| scope);
        let scope = declaring.next()?;
        declaring.next().is_none().then_some(scope)
    }

    /// The value members a reference from outside `namespace` can read
    /// (the properties of the namespace's object), sorted: every exported
    /// value declared directly in it, and every exported namespace nested
    /// in it that is instantiated (a namespace holding only types is no
    /// value). `scope` selects the file's own declarations (`None`) or one
    /// augmentation block's; `namespace` `None` reads the block's own
    /// top level.
    #[must_use]
    pub fn namespace_value_members(
        &self,
        scope: Option<&AugmentationScopeKind>,
        owner: TopLevelOwnerId,
        namespace: Option<&str>,
    ) -> Vec<String> {
        let values = match scope {
            None => Some(&self.value_headers),
            Some(scope) => self.augmentation_value_headers.get(scope),
        };
        let mut members: Vec<String> = values
            .into_iter()
            .flat_map(DeclMap::keys)
            .filter(|key| key.owner == owner)
            .filter_map(|key| {
                let rest = match namespace {
                    Some(namespace) => key
                        .name
                        .strip_prefix(namespace)
                        .and_then(|rest| rest.strip_prefix('.'))?,
                    None => key.name.as_ref(),
                };
                let member = rest.split('.').next()?;
                let qualified = match namespace {
                    Some(namespace) => format!("{namespace}.{member}"),
                    None => member.to_string(),
                };
                self.namespace_member_is_exported(owner, &qualified)
                    .then(|| member.to_string())
            })
            .collect();
        let child_of = |qualified: &str| -> Option<String> {
            let rest = match namespace {
                Some(namespace) => qualified.strip_prefix(namespace)?.strip_prefix('.')?,
                None => qualified,
            };
            (!rest.contains('.')).then(|| rest.to_string())
        };
        let blocks: Vec<&NamespaceBlockRecord> = match scope {
            None => self.namespace_blocks.iter().collect(),
            Some(scope) => self
                .augmentation_namespace_blocks
                .iter()
                .filter(|(block_scope, _)| block_scope == scope)
                .map(|(_, block)| block)
                .collect(),
        };
        members.extend(
            blocks
                .into_iter()
                .filter(|block| block.owner == owner && block.instantiated)
                .filter(|block| self.namespace_member_is_exported(owner, &block.qualified_name))
                .filter_map(|block| child_of(&block.qualified_name)),
        );
        members.sort();
        members.dedup();
        members
    }

    /// Whether the namespace `namespace` declared by `owner` (in its own
    /// scope, or in the `declare module` block `scope`) is instantiated in
    /// some block ([`NamespaceBlockRecord::instantiated`]).
    #[must_use]
    pub fn namespace_is_instantiated(
        &self,
        scope: Option<&AugmentationScopeKind>,
        owner: TopLevelOwnerId,
        namespace: &str,
    ) -> bool {
        let matches = |block: &NamespaceBlockRecord| {
            block.owner == owner && block.qualified_name == namespace && block.instantiated
        };
        match scope {
            None => self.namespace_blocks.iter().any(matches),
            Some(scope) => self
                .augmentation_namespace_blocks
                .iter()
                .any(|(block_scope, block)| block_scope == scope && matches(block)),
        }
    }
}

impl DeclHeaderIndex {
    /// Synthesize a header index FROM an already-built [`EvalEnv`] — the
    /// env-seeded construction mirror (test fixtures and other
    /// already-built-env callers). Names/kinds/params/member names come
    /// from the env's groups; statement locators are empty (a seeded
    /// index never drives selective statement lowering — its memo is
    /// pre-filled).
    pub fn from_eval_env(env: &crate::declarations::EvalEnv) -> Self {
        use crate::declarations::{TypeDeclGroup, ValueDeclGroup};

        fn type_header_from_group(group: &TypeDeclGroup) -> TypeDeclHeader {
            let primary = group.primary();
            let mut type_params: Vec<TypeParamHeader> = Vec::new();
            for decl in group.contributors() {
                for param in decl.type_parameters.params.iter() {
                    if !type_params.iter().any(|p| p.name == param.name) {
                        type_params.push(TypeParamHeader {
                            name: param.name.clone(),
                            constraint_span: None,
                            default_span: None,
                        });
                    }
                }
            }
            let member_headers = group
                .merged_member_header_facts()
                .into_iter()
                .map(|fact| MemberHeader {
                    key: AuthoredPropertyKey::from_known(fact.key),
                    method_kind: fact.method_kind,
                    has_implementation_body: fact.has_implementation_body,
                    optional: fact.optional,
                    readonly: fact.readonly,
                })
                .collect();
            TypeDeclHeader {
                kind: primary.kind,
                span: Span::default(),
                name_span: Span::default(),
                type_params,
                member_headers,
                contributors: Vec::new(),
                vue_ignored_heritage: Vec::new(),
                from_jsdoc_typedef: false,
                jsdoc_typedef: None,
            }
        }

        fn value_header_from_group(group: &ValueDeclGroup) -> ValueDeclHeader {
            let primary = group.primary();
            let object_member_headers = primary
                .object_shape
                .as_ref()
                .map(|shape| {
                    shape
                        .members
                        .iter()
                        .filter_map(|member| {
                            let (key, method_kind, has_implementation_body) = match member {
                                verter_type_expr::facts::ObjectMemberFact::Property(p) => {
                                    (p.key.cloned_known()?, None, false)
                                }
                                verter_type_expr::facts::ObjectMemberFact::Method(m) => (
                                    m.key.cloned_known()?,
                                    Some(m.method_kind),
                                    m.function.has_implementation_body,
                                ),
                                _ => return None,
                            };
                            Some(MemberHeader {
                                key: AuthoredPropertyKey::from_known(key),
                                method_kind,
                                has_implementation_body,
                                optional: false,
                                readonly: false,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            ValueDeclHeader {
                kind: primary.kind,
                span: Span::default(),
                name_span: Span::default(),
                object_member_headers,
                contributors: Vec::new(),
            }
        }

        let mut index = DeclHeaderIndex::default();
        for (name, group) in &env.type_symbols {
            index
                .type_headers
                .insert(name.clone(), type_header_from_group(group));
        }
        for (name, group) in &env.value_symbols {
            index
                .value_headers
                .insert(name.clone(), value_header_from_group(group));
            // An `enum` is a dual-space symbol: post-merge it is a VALUE
            // symbol (kind Enum) carrying its ordered member inventory. The
            // production `index_enum` path ALSO records the member NAMES in
            // the dedicated `enum_headers` table (the member-presence
            // authority), so this env-seeded mirror must too — else a seeded
            // `DeclHeaderIndex` UNDER-COUNTS `enum_symbol_names()` /
            // `enum_member_names()` and the parse-stable-hash enum-header fold
            // plus the enum `MemberPresence` fact emission go wrong for seeded
            // artifacts. The presence rail is the FULL member-NAME set — the
            // stored `EnumMemberNamesFact` inventory unioned across
            // contributors (`merged_enum_member_names_fact`), EVERY
            // statically-named member including unfoldable-VALUE ones — which
            // is the SUPERSET the value rail (`merged_enum_members`) filters;
            // both resolve names via the same `static_name` helper
            // `index_enum` uses, so the seeded mirror reconstructs
            // `index_enum`'s exact union (a value subset would drop
            // unfoldable-value and computed-name members). `Some` exactly when
            // a contributor is an enum. Locators/spans stay empty like every
            // other seeded header (a seeded index never drives selective
            // statement lowering — its memo is pre-filled).
            if let Some(names_fact) = group.merged_enum_member_names_fact() {
                index.enum_headers.insert(
                    name.clone(),
                    EnumDeclHeader {
                        span: Span::default(),
                        name_span: Span::default(),
                        member_names: names_fact.names.iter().cloned().collect(),
                        member_positions: Vec::new(),
                        contributors: Vec::new(),
                    },
                );
            }
        }
        for ((scope, name), group) in &env.augmentation_scopes {
            index
                .augmentation_type_headers
                .entry(scope.clone())
                .or_default()
                .insert(name.clone(), type_header_from_group(group));
        }
        for ((scope, name), group) in &env.augmentation_value_scopes {
            index
                .augmentation_value_headers
                .entry(scope.clone())
                .or_default()
                .insert(name.clone(), value_header_from_group(group));
        }
        index
    }
}

#[cfg(test)]
#[path = "header_index_tests.rs"]
mod header_index_tests;
