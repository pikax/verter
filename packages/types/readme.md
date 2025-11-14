@verter/types

TypeScript-first utility types and Vue emits helpers for Verter. Includes a string-build variant designed for language-server injection with safe name-prefixing.

Features
- Type helpers: `PatchHidden`, `ExtractHidden`, `PartialUndefined`, `UnionToIntersection`.
- Vue emits helpers: `FunctionToObject`, `IntersectionFunctionToObject`, `EmitsToProps`, `ComponentEmitsToProps`.
- String export for embedding in tooling: all declarations prefixed with `$V_` to avoid collisions, comments removable by flag.
- Bench harness to measure TypeScript checker performance for these helpers.

API Reference
- `UniqueKey`: unique symbol used to attach hidden properties in helper results.
- `PatchHidden<T, E>`: add hidden metadata of type `E` to `T` via `UniqueKey`.
- `ExtractHidden<T, R = never>`: retrieve hidden metadata from `T` or fallback to `R`.
- `FunctionToObject<T>`: converts a single emit function signature to an object map `{ [event]: args }` while preserving the function type.
- `IntersectionFunctionToObject<T>`: merges multiple emit function signatures into a single event→args object map.
- `PartialUndefined<T>`: properties in `T` that can be `undefined` become optional, others remain required.
- `UnionToIntersection<U>`: transforms a union of types into their intersection.
- `EmitsToProps<T>`: converts an emits function signature to Vue-style props `{ onXxx: (...args) => void }`.
- `ComponentEmitsToProps<T>`: infers emits from a Vue component instance and derives corresponding props.

Install
- In a pnpm workspace: add as a dev dependency where you need type helpers.

```sh
pnpm add -D @verter/types
```

Usage (TypeScript)
- Import the types you need from the package.

```ts
import type {
  PatchHidden,
  ExtractHidden,
  PartialUndefined,
  UnionToIntersection,
} from '@verter/types';

type WithHidden = PatchHidden<{ id: number }, { meta: true }>;
type OnlyHidden = ExtractHidden<WithHidden>; // { meta: true }

type A = { a?: number; b: string | undefined; c: string };
type Optionalized = PartialUndefined<A>; // `{ a?: number; b?: string; c: string }`

type U = { x: 1 } | { y: 2 };
type I = UnionToIntersection<U>; // `{ x: 1 } & { y: 2 }`
```

Usage (Vue emits helpers)

```ts
import type {
  FunctionToObject,
  IntersectionFunctionToObject,
  EmitsToProps,
  ComponentEmitsToProps,
} from '@verter/types';

// Single function → object of event payloads
type EmitFn = (e: 'save', id: number) => void;
type AsObject = FunctionToObject<EmitFn>;
// { [UniqueKey]?: { save: [number] } } & ((e: 'save', id: number) => void)

// Multiple overloads → merged object of event payloads
type Overloads =
  & ((e: 'open', path: string) => void)
  & ((e: 'close') => void);
type Merged = IntersectionFunctionToObject<Overloads>;

// Props from emits signature
type PropsFromEmits = EmitsToProps<Overloads>; // { onOpen: (path: string) => void; onClose: () => void }
```

String Export (for language servers / tooling)
- Import the prebuilt TypeScript source as a string with all declarations prefixed (`$V_…`).
- Comments are stripped by default; use the flag below to keep them.

```ts
import typeHelpersSource from '@verter/types/string';

// `typeHelpersSource` is a string containing TypeScript declarations like:
//   export declare const $V_UniqueKey: unique symbol;
//   export type $V_ExtractHidden<...> = ...
```

Notes on string export
- All declarations are prefixed with `$V_` and internal references are updated to match.
- Comments are stripped by default (smaller payload); pass `--keep-comments` to retain JSDoc.
- Output is a single JS module exporting a template string; safe to embed into language tools.

Build
- Build both the compiled package and the string export:

```sh
pnpm -w run -C packages/types build
```

- Only rebuild the string export (strip comments by default):

```sh
pnpm -w run -C packages/types build:string
```

- Keep comments in the string export:

```sh
node packages/types/scripts/build-string.mjs --keep-comments
```

Test
- Tests are type-only and run in Vitest “typecheck-only” mode (no runtime assertions).
- Files live alongside sources as `*.spec.ts` in `src/`.
- Config: see `vitest.config.ts` (typecheck.only, checker `tsc`, `tsconfig.test.json`).
- Globals: `src/test-utils.d.ts` provides `assertType` and `assertNever` as ambient helpers.

Run
```sh
# from repo root
pnpm -w run -C packages/types test

# or explicitly enable typecheck mode
pnpm -w run -C packages/types vitest --typecheck
```

Writing tests
- Positive type assertions:
```ts
import { assertType } from 'vitest';
import type { PartialUndefined } from './helpers';

type Original = { a: string; b: number | undefined };
type Result = PartialUndefined<Original>;
type Expected = { a: string; b?: number | undefined };

assertType<Result>({} as Expected);
```

- Negative assertions with `@ts-expect-error` to ensure incorrect shapes are rejected:
```ts
// @ts-expect-error missing required property
assertType<Result>({} as { b?: number });
```

- Emits helpers and intersections:
```ts
import { assertType } from 'vitest';
import type { IntersectionFunctionToObject, ExtractHidden } from './helpers';

type Emits =
  & ((e: 'open', path: string) => void)
  & ((e: 'close') => void);

type AsObj = ExtractHidden<IntersectionFunctionToObject<Emits>>;
assertType<AsObj>({} as { open: [path: string]; close: [] });
```

Benchmark (TypeScript checker performance)
- Generate scalable bench files and run `tsc --extendedDiagnostics` for several sizes.

```sh
# Default sizes (10,50,100,200,500)
pnpm -w run -C packages/types bench

# Custom sizes
node packages/types/scripts/bench-types.mjs --sizes=10,25,50,75,100

# With TypeScript trace (inspect in Chrome DevTools Performance)
pnpm -w run -C packages/types bench:trace
```

Benchmark details
- Sizes control number of properties and union members; event signatures are capped to avoid `TS2589` on very large intersections.
- Reported metrics come from `--extendedDiagnostics`: total/check time, memory, nodes, types.
- Compare before/after changes or across branches to catch regressions.

Compatibility
- TypeScript: tested with `5.8.x`. Other 5.x versions may work, but are not verified.
- Vue: designed around Vue 3’s emits pattern; Vue 2 is not supported.
- Runtime: types-only (no runtime side-effects). The string entry exports a string.

Notes
- The string export prefixes all declarations (types, interfaces, classes, enums, functions, variables, namespaces) with `$V_` and updates references in type positions (`typeof`, `TypeReference`, computed keys) to avoid collisions.
- The benchmark caps extremely large unions/intersections to avoid `TS2589` and focuses on realistic growth patterns.

Contributing
- Useful scripts while iterating:

```sh
# TypeScript watch build
pnpm -w run -C packages/types dev

# Clean outputs
pnpm -w run -C packages/types clean
```

License
- MIT — see repository root `LICENSE`.
