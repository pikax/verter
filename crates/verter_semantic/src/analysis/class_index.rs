//! Collection of the per-file class index during function-program
//! discovery.
//!
//! Discovery already walks every served function body once, to fold its
//! hashes: [`ClassCollector`] records the classes that walk meets. The
//! classes outside every served body are recorded by a walk of the top-level
//! statements ([`collect_top_level_classes`]) that skips each served
//! function, so no syntax is walked twice.
//!
//! A class records its members, the name it is bound to (a declaration's
//! name, or the `const` a class expression initializes) and the bare name
//! its `extends` clause reads. Once discovery has recorded every frame's
//! bindings, [`ClassCollector::finish`] resolves that name lexically — in
//! the frame the class is authored in, then the frames around it, then the
//! file — to the class this file authors that it binds, when it binds one.

use std::sync::Arc;

use oxc_ast::ast::{
    BindingPattern, Class, ClassElement, Expression, MethodDefinitionKind, VariableDeclaration,
    VariableDeclarationKind,
};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_session_query::function_program::{
    ClassBase, ClassSyntaxDiscovery, ClassSyntaxRecord, FunctionBindingKind,
    FunctionProgramDiscovery, FunctionProgramKey,
};

use super::function_program::{LexicalBinding, LexicalScopeIndex};

/// The frame a class is authored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClassFrame {
    /// Outside every function.
    File,
    /// The served function at this discovery ordinal.
    Entry(usize),
    /// A function no entry serves: its bindings are not inventoried.
    Unserved,
}

/// How a class is bound to a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassBindingKind {
    /// A class declaration's own name.
    Declaration,
    /// A `const` declarator the class expression initializes.
    ConstInitializer,
}

struct CollectedClass {
    record: ClassSyntaxRecord,
    frame: ClassFrame,
    /// Outside every function, but inside a scope narrower than the file
    /// (a block, a class body, a namespace): a name there may bind
    /// something the file scope does not.
    scoped: bool,
    /// The name the class is bound to, and the binding identifier's span.
    binding: Option<(Arc<str>, verter_span::Span, ClassBindingKind)>,
    /// The bare name the `extends` clause reads, and where it reads it.
    heritage: Option<(Arc<str>, verter_span::Span)>,
}

/// The classes one discovery walk meets.
#[derive(Default)]
pub(super) struct ClassCollector {
    classes: Vec<CollectedClass>,
    /// The span start of each class recorded, so a class two walks meet is
    /// recorded once.
    seen: FxHashSet<u32>,
    frames: Vec<ClassFrame>,
    /// Outside every function, how many scopes narrower than the file
    /// enclose the walk.
    file_scopes: u32,
    /// The `const` declarator each class expression initializes, by the
    /// class's span start.
    const_initializers: FxHashMap<u32, (Arc<str>, verter_span::Span)>,
}

impl ClassCollector {
    /// Walk inside `frame` until the matching [`Self::exit_frame`].
    pub(super) fn enter_frame(&mut self, frame: ClassFrame) {
        self.frames.push(frame);
    }

    pub(super) fn exit_frame(&mut self) {
        self.frames.pop();
    }

    /// Walk inside a scope narrower than the file (a block, a class body,
    /// a namespace) until the matching [`Self::exit_scope`].
    pub(super) fn enter_scope(&mut self) {
        self.file_scopes += 1;
    }

    pub(super) fn exit_scope(&mut self) {
        self.file_scopes -= 1;
    }

    /// Note the class expressions a `const` declaration initializes, so the
    /// class records the name it is bound to.
    pub(super) fn note_variable_declaration(&mut self, declaration: &VariableDeclaration<'_>) {
        if declaration.kind != VariableDeclarationKind::Const {
            return;
        }
        for declarator in &declaration.declarations {
            if let (
                BindingPattern::BindingIdentifier(id),
                Some(Expression::ClassExpression(class)),
            ) = (&declarator.id, declarator.init.as_ref())
            {
                self.const_initializers.insert(
                    class.span.start,
                    (
                        Arc::from(id.name.as_str()),
                        verter_span::Span::new(id.span.start, id.span.end),
                    ),
                );
            }
        }
    }

    /// Record `class`, once.
    pub(super) fn record(&mut self, class: &Class<'_>) {
        if !self.seen.insert(class.span.start) {
            return;
        }
        let mut members = Vec::with_capacity(class.body.body.len());
        for element in &class.body.body {
            members.push(verter_span::Span::new(
                element.span().start,
                element.span().end,
            ));
            if let ClassElement::MethodDefinition(method) = element {
                if method.kind == MethodDefinitionKind::Constructor {
                    members.extend(
                        method
                            .value
                            .params
                            .items
                            .iter()
                            .filter(|parameter| {
                                parameter.accessibility.is_some()
                                    || parameter.readonly
                                    || parameter.r#override
                            })
                            .map(|parameter| {
                                verter_span::Span::new(parameter.span.start, parameter.span.end)
                            }),
                    );
                }
            }
        }
        let expression = class.r#type == oxc_ast::ast::ClassType::ClassExpression;
        let binding = if expression {
            self.const_initializers
                .get(&class.span.start)
                .map(|(name, span)| (Arc::clone(name), *span, ClassBindingKind::ConstInitializer))
        } else {
            class.id.as_ref().map(|id| {
                (
                    Arc::from(id.name.as_str()),
                    verter_span::Span::new(id.span.start, id.span.end),
                    ClassBindingKind::Declaration,
                )
            })
        };
        let heritage = class.heritage.as_ref().and_then(|heritage| {
            let mut base = &heritage.expression;
            while let Expression::ParenthesizedExpression(inner) = base {
                base = &inner.expression;
            }
            match base {
                Expression::Identifier(name) => Some((
                    Arc::from(name.name.as_str()),
                    verter_span::Span::new(name.span.start, name.span.end),
                )),
                _ => None,
            }
        });
        let frame = self.frames.last().copied().unwrap_or(ClassFrame::File);
        self.classes.push(CollectedClass {
            record: ClassSyntaxRecord {
                span: verter_span::Span::new(class.span.start, class.span.end),
                expression,
                has_heritage: class.heritage.is_some(),
                members: Arc::from(members.into_boxed_slice()),
            },
            frame,
            scoped: frame == ClassFrame::File && self.file_scopes > 0,
            binding,
            heritage,
        });
    }

    /// Resolve every class's `extends` name against the frames discovery
    /// recorded, and hand the classes to the index.
    ///
    /// The name resolves in the frame the class is authored in, then in
    /// each frame around it (the binding inventories and lexical scopes the
    /// capture resolution reads), then at the file. It names a class when
    /// the binding it reaches is a class declaration's name or a `const`
    /// a class expression initializes. A frame no entry serves, a binding
    /// of any other kind, an unmodelled local, a namespace member and a
    /// class in a scope narrower than the file outside every function all
    /// leave the base unresolved.
    pub(super) fn finish(self, entries: &[FunctionProgramDiscovery]) -> Vec<ClassSyntaxDiscovery> {
        let mut by_binding: FxHashMap<verter_span::Span, (usize, ClassBindingKind)> =
            FxHashMap::default();
        let mut file_scope: FxHashMap<&str, Vec<usize>> = FxHashMap::default();
        for (position, class) in self.classes.iter().enumerate() {
            if let Some((name, span, kind)) = &class.binding {
                by_binding.entry(*span).or_insert((position, *kind));
                if class.frame == ClassFrame::File && !class.scoped {
                    file_scope.entry(name.as_ref()).or_default().push(position);
                }
            }
        }
        let position_of: FxHashMap<&FunctionProgramKey, usize> = entries
            .iter()
            .enumerate()
            .map(|(position, entry)| (&entry.key, position))
            .collect();
        let mut scopes: Vec<Option<LexicalScopeIndex>> = Vec::new();
        scopes.resize_with(entries.len(), || None);
        let mut bases = Vec::with_capacity(self.classes.len());
        for (position, class) in self.classes.iter().enumerate() {
            let Some((name, site)) = &class.heritage else {
                bases.push(if class.record.has_heritage {
                    ClassBase::Unresolved
                } else {
                    ClassBase::None
                });
                continue;
            };
            let at_file = |scoped: bool| match file_scope.get(name.as_ref()).map(Vec::as_slice) {
                Some([only]) if !scoped && *only != position => {
                    ClassBase::Class(u32::try_from(*only).unwrap_or(u32::MAX))
                }
                _ => ClassBase::Unresolved,
            };
            let base = match class.frame {
                ClassFrame::Unserved => ClassBase::Unresolved,
                ClassFrame::File => at_file(class.scoped),
                ClassFrame::Entry(frame) => {
                    let mut chain = Vec::new();
                    let mut current = Some(frame);
                    while let Some(at) = current {
                        chain.push(at);
                        current = entries[at]
                            .lexical_parent
                            .as_deref()
                            .and_then(|parent| position_of.get(parent).copied());
                    }
                    for &at in &chain {
                        if scopes[at].is_none() {
                            scopes[at] = Some(LexicalScopeIndex::build(&entries[at]));
                        }
                    }
                    let frame_scopes: Vec<&LexicalScopeIndex> = chain
                        .iter()
                        .map(|&at| scopes[at].as_ref().expect("built above"))
                        .collect();
                    let resolved = frame_scopes
                        .iter()
                        .enumerate()
                        .find_map(|(at, scope)| scope.resolve(name, *site).map(|slot| (at, slot)));
                    match resolved {
                        Some((at, LexicalBinding::Modeled(slot))) => {
                            let binding = &entries[chain[at]].bindings[slot as usize];
                            let wanted = match binding.kind {
                                FunctionBindingKind::Class => Some(ClassBindingKind::Declaration),
                                FunctionBindingKind::Const => {
                                    Some(ClassBindingKind::ConstInitializer)
                                }
                                _ => None,
                            };
                            match (wanted, by_binding.get(&binding.span)) {
                                (Some(wanted), Some(&(base, kind)))
                                    if kind == wanted && base != position =>
                                {
                                    ClassBase::Class(u32::try_from(base).unwrap_or(u32::MAX))
                                }
                                _ => ClassBase::Unresolved,
                            }
                        }
                        Some((_, LexicalBinding::UnmodeledLocal)) => ClassBase::Unresolved,
                        // No frame binds the name: it reads the file scope,
                        // unless the outermost frame is a namespace member.
                        None => {
                            let outermost = chain.last().copied().unwrap_or(frame);
                            at_file(entries[outermost].locator.descent.has_namespace_member())
                        }
                    }
                }
            };
            bases.push(base);
        }
        self.classes
            .into_iter()
            .zip(bases)
            .map(|(class, base)| ClassSyntaxDiscovery {
                record: class.record,
                base,
            })
            .collect()
    }
}
