# @verter/svelte-jsx

Verter's types-only JSX namespace for Svelte IDE projections. It supplies the
`jsx-runtime` type entrypoints used by generated `.svelte.tsx` carriers; it does
not contain a runtime JSX factory and generated carriers are never executed.

Svelte support in Verter is experimental and has not yet been validated in
real-world use.

## Resolution contract

Normal Verter users do not need to install this package. The language host
materializes a version-matched copy outside the workspace and maps its
entrypoints into each inferred TypeScript project. The TypeScript plugin also
ships a matching copy.

`svelte` is an optional peer because the shim must use the owning project's
Svelte types, not a version bundled by Verter. When a carrier is evaluated, the
host maps `svelte` and `svelte/*` to that project's installed Svelte package. A
workspace without Svelte does not receive fallback `any` types: module
resolution fails and Verter reports the missing package.

The package exposes HTML, SVG, and MathML entrypoints:

- `@verter/svelte-jsx/jsx-runtime`
- `@verter/svelte-jsx/jsx-dev-runtime`
- `@verter/svelte-jsx/svg/jsx-runtime`
- `@verter/svelte-jsx/svg/jsx-dev-runtime`
- `@verter/svelte-jsx/mathml/jsx-runtime`
- `@verter/svelte-jsx/mathml/jsx-dev-runtime`

The declarations are the canonical source mirrored and byte-checked by the
Rust host. Package and compiler versions must remain aligned.

The namespace is private to each generated carrier. It admits Svelte 5's
callable `Component<Props, Exports, Bindings>` for template checking without
changing the public component module type or merging global JSX declarations.
HTML and SVG attributes come from the owning project's official
`svelte/elements` tables. Svelte does not currently publish a MathML element
table, so that closed attribute vocabulary is Verter-owned while its event base
still uses `DOMAttributes<MathMLElement>`.

## License

MIT
