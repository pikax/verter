---
ruling_id: "CSS-FRAMEWORK-CONSTRUCT-VALIDITY"
type: "architecture-ruling"
date: "2026-08-21"
date_source: "stated"
binds: ["J1", "J4", "CSS/style pipeline architecture"]
source_file: "ARCH-RULING-CSS-FRAMEWORK-CONSTRUCT-VALIDITY.md"
summary: "Codex decides how framework-specific CSS constructs are handled once StyleSyntaxIr is the sole parser for every carrier. Validity is a SEPARATE carrier-aware pass in verter_semantic, not in the parser: verter_css_syntax stays carrier-blind and lowers neutral pseudos/functions, and the validation profile comes from FrameworkAdapterRegistry rather than a hardcoded Vue/Svelte match. `:global()` interns as ONE neutral occurrence bound to a registered semantic handler — no VueGlobal/SvelteGlobal/ModulesGlobal parser variants — and two claimants without a precedence row is an ambiguity ERROR. `v-bind()` is well-formed generic CSS, so Verter is NOT entitled to reject it outside Vue: it passes through everywhere. Provenance does NOT travel through an ordinary import; only an explicit typed `<style src>` edge is Vue context. A validity ERROR is an LSP error diagnostic AND fatal to emission — no warning-and-compile fallback, because that preserves silently wrong CSS. This is a recordable breaking change. J4 owns the validity policy, diagnostics, ambiguity handling and capability matrix; J1 only supplies the prerequisite neutral IR facts and deletes private scanners. No DAG edge change."
supersedes: []
superseded_by: []
contradicts: []
notes: "Answers the maintainer's question of 2026-08-21: framework-specific CSS must only be valid in its own framework, parsed everywhere but handled properly elsewhere. Two framings in the question were rejected: v-bind() cannot be blanket-rejected outside Vue because its spelling is legal CSS functional notation, and ownership is J4 rather than J1 (J1 supplies neutral facts only). Records five open questions, including whether `<style src>` is even distinguishable from an ordinary imported stylesheet in the current APIs. An earlier consult on the same question exhausted its budget dumping whole files and produced no verdict; this is the re-run."
---

# Architecture Ruling — Framework-specific CSS construct validity

**Status:** RATIFIED 2026-08-21 by the codex architect.

The maintainer asked how Vue-specific CSS (`v-bind()`, `:deep()`, `:slotted()`,
`:global()`) and Svelte-specific CSS should behave once one parser accepts them
in every carrier: *"we can still parse it but if not in Vue we need to handle it
properly maybe error or warning, same thing for svelte specific CSS syntax."*

The verdict is recorded verbatim below.

---

## 1. Where validity lives

Decision: a separate carrier-aware validation pass in `verter_semantic`, at a proposed seam such as:

```text
validate_style_ir(
    ir: &StyleSyntaxIr,
    profile: &StyleValidationProfile,
    style_options: StyleOptions,
) -> StyleValidation
```

`verter_css_syntax` remains carrier-blind and may parse/lower neutral functions and pseudos. Carrier-conditional acceptance inside it would violate the syntax/lowering-only boundary. `verter_compiler` transforms only occurrences already validated and bound to a semantic owner.

`StyleValidationProfile` must come from `FrameworkAdapterRegistry`: extend each registered adapter descriptor with style-construct claims. Do not match directly on `Vue`/`Svelte`; an unregistered tag confers no semantics. Invariant: support exists only through registration. [CLAUDE.md:289](CLAUDE.md:289)

## 2. `:global()`

One neutral IR occurrence:

```text
SelectorPseudoSyntax {
    name,
    form: Bare | Functional,
    raw_argument,
    selector_projection,
    completeness,
    span,
}
```

No `VueGlobal`, `SvelteGlobal`, or `ModulesGlobal` parser variants. Validation produces a separate binding from occurrence to registered semantic handler. The Vue, Svelte, and CSS-Modules transforms consume that binding and apply their own semantics.

If two active domains claim the occurrence—such as scoped Vue plus CSS Modules—the registry needs an explicit composition/precedence row. Without one, compilation is an ambiguity error. This is possible without parser framework branching because all three forms share lexical/balanced-selector structure; their differences are semantic. Invariant: Vue and Svelte separately consume the same trusted IR. [.claude/skills/architecture/SKILL.md:52](.claude/skills/architecture/SKILL.md:52)

## 3. Disposition and provenance

`v-bind(x)` outside Vue receives no Vue interpretation. Because the spelling is well-formed generic CSS functional notation, Verter is not entitled to reject it merely by name; it passes through in Svelte and plain CSS. A diagnostic would require additional evidence that Vue semantics were intended.

Provenance does not travel through an ordinary CSS/JS import. The imported file keeps plain-CSS identity. An explicitly attached external Vue style region—such as a typed `<style src>` edge—is Vue context because the carrier relationship itself is explicit, not because Vue happens to import it.

Invariant: unchanged plain CSS is a required passthrough surface. [MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:417](docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:417)

## 4. User-visible result

A semantic-validity `ERROR`:

- is an LSP error diagnostic;
- is fatal for build/compiler emission;
- blocks transformation/emission of the invalid style;
- does not prevent recovery-based parsing or unrelated IDE features.

No warning-and-compile fallback for unambiguously mismatched framework pseudos: that would preserve silently wrong CSS. Invariant: unsupported or unmappable semantic regions fail closed rather than producing misleading output. [CLAUDE.md:166](CLAUDE.md:166)

## 5. Breaking change

Yes. Inputs currently accepted, preserved, or accidentally routed through Vue-oriented preprocessing will begin failing when an incompatible carrier owns them. The registry requirement and non-propagating import provenance are also contract changes.

Record this explicitly as a semantic-acceptance breaking change. The current path unconditionally runs the Vue `v-bind`/pseudo prepass before examining style options. [crates/verter_compiler/src/css/mod.rs:97](crates/verter_compiler/src/css/mod.rs:97)

## 6. J ownership

J4 owns the carrier/dialect capability matrix, validity policy, diagnostics, ambiguity handling, and recovery behavior.

J1 must supply the prerequisite neutral IR facts and migrate/delete private scanners, but should not own the carrier-acceptance contract. No new block and no DAG edge are required: J4 already follows J3 → J2 → J1. Invariant: parser coverage moved to J1 while capability and recovery remain J4. [ARCH-RULING-J-TRAIN-FIVE-FORKS.md:33](docs/arch/refactor/rev11/rulings/ARCH-RULING-J-TRAIN-FIVE-FORKS.md:33)

## Disposition table

`PASSTHROUGH` means the validity pass accepts without changing bytes. `†` means the subsequently selected semantic transform may rewrite it. Vue cells assume the required Vue style option, such as `scoped`, is active.

| construct | Vue SFC | Svelte | plain `.css` | `.css` imported by a Vue SFC |
|---|---|---|---|---|
| `v-bind()` | PASSTHROUGH† | PASSTHROUGH | PASSTHROUGH | PASSTHROUGH |
| `:deep()` | PASSTHROUGH† | ERROR | ERROR | ERROR |
| `:slotted()` | PASSTHROUGH† | ERROR | ERROR | ERROR |
| `:global()` | PASSTHROUGH† | PASSTHROUGH† | ERROR | ERROR |
| Svelte-specific forms¹ | ERROR | PASSTHROUGH† | ERROR | ERROR |

¹ Only forms proven to be Svelte-exclusive and invalid as ordinary CSS. A spelling that is also legal generic CSS follows the `v-bind()` rule and cannot be blanket-rejected.

Explicit CSS-Modules mode independently claims `:global`/`:local`; those occurrences pass to the CSS-Modules transform. It does not activate merely because a file is imported.

## DAG and ownership changes

- No DAG-edge change.
- Add style-construct claims to `FrameworkAdapterDescriptor`.
- Add carrier-neutral validation/profile types to `verter_semantic`.
- Keep neutral parsing in `verter_css_syntax`.
- Keep byte-changing framework implementations in `verter_compiler`.
- Add the neutral-fact prerequisite to J1 acceptance and the full disposition/diagnostic matrix to J4.

## Genuine open questions

- The exact inventory and grammar of “Svelte-specific forms” is unspecified.
- Vue’s required behavior for `:deep`/`:global`/`:slotted` in an unscoped style needs an oracle-backed contract.
- Vue scoped-style plus CSS-Modules `:global` composition/precedence is not yet specified.
- Final runtime CSS-Modules ownership remains unresolved.
- Existing APIs must be inventoried to determine whether `<style src>` is distinguishable from an ordinary imported stylesheet.
