//! Generic substitution environments and applied-node keys.
//!
//! Substitution maps type parameter names to arena `NodeId`s. Applied-node
//! identities are used as memoization keys for instantiation results.

use rustc_hash::FxHashMap;

use super::arena::NodeId;

// ---------------------------------------------------------------------------
// Substitution environment
// ---------------------------------------------------------------------------

/// Maps type parameter names to their resolved `NodeId` in the current
/// instantiation context.
///
/// Substitution environments are typically query-local and short-lived.
/// They're created when instantiating a generic declaration with concrete
/// type arguments.
#[derive(Debug, Clone, Default)]
pub struct SubstitutionEnv {
    bindings: FxHashMap<String, NodeId>,
}

impl SubstitutionEnv {
    /// Create a new empty substitution.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a substitution from type parameter names and argument NodeIds.
    ///
    /// If `args` is shorter than `params`, remaining params are unbound.
    /// If `args` is longer, excess args are ignored.
    pub fn from_params_and_args(params: &[String], args: &[NodeId]) -> Self {
        let mut bindings = FxHashMap::default();
        for (name, &arg) in params.iter().zip(args.iter()) {
            bindings.insert(name.clone(), arg);
        }
        Self { bindings }
    }

    /// Bind a type parameter name to a NodeId.
    pub fn bind(&mut self, name: impl Into<String>, node: NodeId) {
        self.bindings.insert(name.into(), node);
    }

    /// Look up a type parameter binding.
    pub fn resolve(&self, name: &str) -> Option<NodeId> {
        self.bindings.get(name).copied()
    }

    /// Whether this substitution has any bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Iterate over bindings.
    pub fn iter(&self) -> impl Iterator<Item = (&str, NodeId)> {
        self.bindings.iter().map(|(k, v)| (k.as_str(), *v))
    }

    /// Extend with bindings from another substitution (overwrites on conflict).
    pub fn extend(&mut self, other: &SubstitutionEnv) {
        self.bindings
            .extend(other.bindings.iter().map(|(k, v)| (k.clone(), *v)));
    }
}

// ---------------------------------------------------------------------------
// Applied key (for instantiation memoization)
// ---------------------------------------------------------------------------

/// Unique identity for an applied (instantiated) generic declaration.
///
/// Used as a memoization key: if we've already instantiated `Partial<Props>`,
/// we return the cached result node instead of re-expanding.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppliedKey {
    /// Canonical declaration identity (file + symbol).
    pub canonical_id: String,
    pub symbol_name: String,
    /// The resolved type argument NodeIds.
    pub args: Vec<NodeId>,
}

impl AppliedKey {
    pub fn new(
        canonical_id: impl Into<String>,
        symbol_name: impl Into<String>,
        args: Vec<NodeId>,
    ) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            symbol_name: symbol_name.into(),
            args,
        }
    }
}

// ---------------------------------------------------------------------------
// Infer bindings
// ---------------------------------------------------------------------------

/// Accumulated `infer T` bindings during conditional type resolution.
///
/// When the relation engine encounters `infer T` positions, it records
/// candidate types here. After relation checking completes, these bindings
/// are used to populate the true branch's substitution environment.
#[derive(Debug, Clone, Default)]
pub struct InferBindings {
    /// Each infer variable may accumulate multiple candidates (from
    /// co-variant positions), which are then intersected for the final binding.
    candidates: FxHashMap<String, Vec<NodeId>>,
}

impl InferBindings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a candidate for an infer variable.
    pub fn add_candidate(&mut self, name: impl Into<String>, candidate: NodeId) {
        self.candidates
            .entry(name.into())
            .or_default()
            .push(candidate);
    }

    /// Get all candidates for a given infer variable.
    pub fn candidates(&self, name: &str) -> Option<&[NodeId]> {
        self.candidates.get(name).map(|v| v.as_slice())
    }

    /// Iterate over all infer variables and their candidates.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[NodeId])> {
        self.candidates
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Whether any infer bindings were recorded.
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// Number of distinct infer variables.
    pub fn len(&self) -> usize {
        self.candidates.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitution_env_from_params_and_args() {
        let params = vec!["T".to_string(), "U".to_string()];
        let args = vec![NodeId(0), NodeId(1)];
        let env = SubstitutionEnv::from_params_and_args(&params, &args);

        assert_eq!(env.resolve("T"), Some(NodeId(0)));
        assert_eq!(env.resolve("U"), Some(NodeId(1)));
        assert_eq!(env.resolve("V"), None);
        assert_eq!(env.len(), 2);
    }

    #[test]
    fn substitution_env_short_args() {
        let params = vec!["T".to_string(), "U".to_string()];
        let args = vec![NodeId(5)];
        let env = SubstitutionEnv::from_params_and_args(&params, &args);

        assert_eq!(env.resolve("T"), Some(NodeId(5)));
        assert_eq!(env.resolve("U"), None); // unbound
    }

    #[test]
    fn substitution_env_extend_overwrites() {
        let mut env = SubstitutionEnv::new();
        env.bind("T", NodeId(0));
        env.bind("U", NodeId(1));

        let mut other = SubstitutionEnv::new();
        other.bind("U", NodeId(99));
        other.bind("V", NodeId(2));

        env.extend(&other);
        assert_eq!(env.resolve("T"), Some(NodeId(0)));
        assert_eq!(env.resolve("U"), Some(NodeId(99))); // overwritten
        assert_eq!(env.resolve("V"), Some(NodeId(2)));
    }

    #[test]
    fn applied_key_equality() {
        let k1 = AppliedKey::new("/types.ts", "Partial", vec![NodeId(0)]);
        let k2 = AppliedKey::new("/types.ts", "Partial", vec![NodeId(0)]);
        let k3 = AppliedKey::new("/types.ts", "Partial", vec![NodeId(1)]);

        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn infer_bindings_accumulate_candidates() {
        let mut bindings = InferBindings::new();
        assert!(bindings.is_empty());

        bindings.add_candidate("T", NodeId(0));
        bindings.add_candidate("T", NodeId(1));
        bindings.add_candidate("U", NodeId(2));

        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings.candidates("T"), Some(&[NodeId(0), NodeId(1)][..]));
        assert_eq!(bindings.candidates("U"), Some(&[NodeId(2)][..]));
        assert_eq!(bindings.candidates("V"), None);
    }
}
