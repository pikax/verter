// `formatCount` is a workspace-sibling export (`./utils`) that is intentionally
// NOT imported here, so the type provider offers it as a cross-file auto-import
// completion. Accepting it must, on `completionItem/resolve`, supply the missing
// `import { formatCount } from "./utils"` statement via additionalTextEdits.
//
// This file physically exists on disk under the fixture's tsconfig
// `include: ["src"]`, so it is a CONFIGURED-PROJECT member on both providers —
// the realistic shape for a PLAIN TypeScript use-site whose import source is a
// real on-disk sibling. (An in-memory-only sibling at a synthetic path lands in
// an inferred project whose auto-import map excludes configured-project siblings.
// This fixture models the plain-TS case only; the `.vue`/`.svelte` carrier
// surface, whose generated TSX project membership is a separate concern, is not
// covered here.)
//
// The reference is deliberately un-imported: that is the precondition the
// auto-import resolve test exercises. Suppress the resulting unresolved-name
// diagnostic so this committed configured-project member does not carry a raw
// project error (the directive itself self-validates — it would error if
// `formatCount` ever resolved here without an import).
// @ts-expect-error intentional unresolved reference: the auto-import resolve test asserts the provider supplies the missing `import { formatCount } from "./utils"`.
export const formattedUsage = formatCount(42);
