# Removed ambient-jsx.d.ts

Permissive `JSX.IntrinsicElements` ambient was deleted so clean-diagnostics
tests exercise the real production JSX environment (ISSUE-svelte-jsx-intrinsics).

Do not restore a catch-all `[elemName: string]: any` ambient into the required
svelte-parity fixture. If a typed Svelte HTML map is needed, inject it from the
host the same way production would.
