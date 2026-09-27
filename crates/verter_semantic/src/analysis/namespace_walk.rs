//! A namespace and the namespaces nested in it, walked from an explicit
//! stack.
//!
//! A namespace declares namespaces two ways: a dotted name
//! (`namespace A.B { … }`, one declaration per segment) and a statement of
//! its block (`namespace B { … }`, exported or not). Every walk that
//! registers a namespace's members under their qualified names — the header
//! index, the declaration-body collection, each for the file scope and for
//! an augmentation block — descends both. Descending them by native
//! recursion took a native level per nesting; here each namespace being
//! walked is a frame of an explicit stack, so the walk's native depth does
//! not depend on the input.

use oxc_ast::ast::{Declaration, Statement, TSNamespaceDeclaration, TSNamespaceDeclarationBody};

/// The qualified name of the namespace a walk is in (`A.B.C`), grown by
/// a segment as a namespace is entered and cut back as it is left, so a
/// walk holds one name however deep the nest, never one per namespace.
#[derive(Debug, Default)]
pub(crate) struct QualifiedPath(String);

impl QualifiedPath {
    /// A path whose root namespace is declared under `prefix`.
    pub(crate) fn under(prefix: Option<&str>) -> Self {
        Self(prefix.unwrap_or_default().to_owned())
    }

    /// Enter the namespace `segment`; returns the length to leave it at.
    pub(crate) fn enter(&mut self, segment: &str) -> usize {
        let enclosing = self.0.len();
        if !self.0.is_empty() {
            self.0.push('.');
        }
        self.0.push_str(segment);
        enclosing
    }

    /// Leave the namespace entered at `enclosing`.
    pub(crate) fn leave(&mut self, enclosing: usize) {
        self.0.truncate(enclosing);
    }

    /// The qualified name.
    pub(crate) fn name(&self) -> &str {
        &self.0
    }
}

/// How a namespace nests in the one around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Nesting {
    /// The namespace the walk starts at.
    Root,
    /// A dotted name's next segment.
    Dotted,
    /// A statement of the enclosing namespace's block.
    Statement {
        /// Whether the statement is written `export`.
        exported: bool,
    },
}

/// What a walk does at each namespace and each of its other statements.
pub(crate) trait NamespaceVisitor<'s, 'a> {
    /// The walk's state for one namespace.
    type Frame;

    /// Enter `decl`, nested as `nesting` in `parent` (none at the root).
    fn enter(
        &mut self,
        decl: &'s TSNamespaceDeclaration<'a>,
        parent: Option<&Self::Frame>,
        nesting: Nesting,
    ) -> Self::Frame;

    /// One statement of a namespace's block that declares no namespace, in
    /// source order (a nested namespace's statements come between its
    /// siblings', where it is declared).
    fn statement(&mut self, frame: &mut Self::Frame, statement: &'s Statement<'a>);

    /// Leave a namespace, once everything nested in it is walked.
    fn exit(&mut self, _frame: Self::Frame, _parent: Option<&mut Self::Frame>) {}
}

/// The namespace a namespace block's statement declares, and whether the
/// statement is written `export`.
pub(crate) fn nested_namespace<'s, 'a>(
    statement: &'s Statement<'a>,
) -> Option<(&'s TSNamespaceDeclaration<'a>, bool)> {
    match statement {
        Statement::TSNamespaceDeclaration(module) => Some((module, false)),
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::TSNamespaceDeclaration(module) => Some((module, true)),
            _ => None,
        },
        _ => None,
    }
}

/// One namespace being walked.
struct Open<'s, 'a, F> {
    frame: F,
    /// A dotted name's next segment, still to walk.
    inner: Option<&'s TSNamespaceDeclaration<'a>>,
    /// Its block's statements still to walk.
    statements: std::slice::Iter<'s, Statement<'a>>,
}

fn open<'s, 'a, F>(decl: &'s TSNamespaceDeclaration<'a>, frame: F) -> Open<'s, 'a, F> {
    match &decl.body {
        TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) => Open {
            frame,
            inner: Some(inner),
            statements: [].iter(),
        },
        TSNamespaceDeclarationBody::TSModuleBlock(block) => Open {
            frame,
            inner: None,
            statements: block.body.iter(),
        },
    }
}

/// Walk `decl` and every namespace nested in it with `visitor`, in source
/// order: a namespace is entered before its statements and left after the
/// last of them.
pub(crate) fn walk_namespaces<'s, 'a, V: NamespaceVisitor<'s, 'a>>(
    decl: &'s TSNamespaceDeclaration<'a>,
    visitor: &mut V,
) {
    let root = visitor.enter(decl, None, Nesting::Root);
    let mut stack = vec![open(decl, root)];
    while let Some(top) = stack.last_mut() {
        if let Some(inner) = top.inner.take() {
            let frame = visitor.enter(inner, Some(&top.frame), Nesting::Dotted);
            stack.push(open(inner, frame));
            continue;
        }
        match top.statements.next() {
            Some(statement) => match nested_namespace(statement) {
                Some((module, exported)) => {
                    let frame =
                        visitor.enter(module, Some(&top.frame), Nesting::Statement { exported });
                    stack.push(open(module, frame));
                }
                None => visitor.statement(&mut top.frame, statement),
            },
            None => {
                let done = stack.pop().expect("the namespace just read");
                visitor.exit(done.frame, stack.last_mut().map(|open| &mut open.frame));
            }
        }
    }
}
