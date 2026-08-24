# Direct-route byte-identity against the accepted predecessor base

Answers the review finding that the four-route identity test compared only
post-change routes against each other, never against the predecessor's frozen
output. That test proves the four routes agree with EACH OTHER; it cannot prove
they agree with what the direct route produced before this work.

## Method

A throwaway, uncommitted Rust example — never landed, present in neither commit's
history — ran identically and unmodified at two commits:

- **Base:** `e30b42ba1960e04178aff5c0248d0dde13891ac6`, the accepted predecessor and this
  branch's original branch point. It is no longer the branch's immediate parent (rebases
  have moved that to a later trunk commit), but only one intervening trunk commit touched
  the compiler crates at all — a dependency migration — so comparing against the accepted
  predecessor is if anything the stronger comparison.
- **Candidate:** `78e4579e532add3ec5e03f186a5eeeba03f71bc7`, the branch tip when the probe was run.

The probe hashes `StandaloneCompiler::compile`'s own output — per artifact its
kind, code, DIALECT and both source-map slots; then each style block's code,
map, lang, scope hash, global flag and OUTPUT DESCRIPTOR; then each diagnostic's
severity, code, message and span — with the hashing INLINED in the probe rather
than calling `direct_compile_output_digest`, because that function does not exist
at the base commit and the whole point is that the same logic runs at both.

### Oracle completeness

An earlier revision of this probe omitted artifact `dialect` and each style
block's `output_descriptor` while this record concluded "ZERO byte change" — a
conclusion broader than the oracle that produced it. Both are now covered, and
neither is hashed through `Debug`: `dialect` goes through an exhaustive
`dialect_rank` match (a renamed variant does not move the digest; a NEW variant
fails to compile rather than silently sharing a rank), and the descriptor is
hashed field by field — source-space token/kind/source-token/content-hash/byte
length, content-artifact token/space-token/content-hash/byte length, and the
source map's hash, destination token, ordered declared tokens, raw map and
fidelity.

Both additions were proved to REACH the digest rather than assumed to. Planting
a marker byte into the descriptor hasher moved all six digests, and XOR-ing the
dialect rank moved all six; a probe whose new fields were unreachable for these
fixtures would have left them unchanged. So the six digests below are a genuinely
wider oracle than the ones they replace, not the same measurement relabelled.

It compiles the six fixtures that exist at BOTH commits (`vue_simple`,
`vue_medium`, `vue_large_dual_runtime`, `svelte_markup_only`, `svelte_props`,
`svelte_state`) through `StandaloneCompiler::compile` — the direct route, whose
signature and `DirectExecutionInputs` shape are identical at both commits. The
`vue_vapor` fixture is excluded: it did not exist at the base, so there is no
baseline to diff it against.

The base worktree needed one manifest edit to build the probe at all —
`sha2 = { workspace = true }` added to `crates/verter_bench/Cargo.toml`, which the
candidate tree already has. That is a dependency of the throwaway probe, not of
anything under test.

## Result

```
vue_simple:             91395f10cf7739b20f5c0dfbd3af445032f82d1aa3ba1a8a610e22dbd85f6fdc
vue_medium:             0c0694831d0b4244df6fd1fe698d85b0480529a3c0f9bedd0bba01c4082fdad3
vue_large_dual_runtime: 1932d50a3196084ae522590408a7cc906678bcb7ef28a430a3970a4b9bca39cb
svelte_markup_only:     e2a786ddcc1e7fb4d74afb66ebcc6269438051546bcf89446944dd6f99a0912a
svelte_props:           0fcb18bfac3bf65387be4f1ccbb7c53351c21dccec854ae209e4abbfc569dfcf
svelte_state:           e68de5d4eab128484d3885b90b745d5c4c395b767770c6a180649d6155533176
```

Re-run at the candidate end after a later round changed rustdoc in `standalone.rs` and
`client_compile.rs`. This record's own rule requires re-running on ANY change to those
files, including a comment, and that rule was honoured rather than reasoned around: all six
digests came back identical, so the doc edits are now MEASURED not to have moved behaviour
instead of merely argued not to have. The base end was not re-run — `e30b42ba1` is an
immutable commit whose digests came from a real run and cannot have changed.

Identical at both commits, byte for byte, for all six fixtures; `diff` of the two
runs' output is empty.

## Conclusion

Splitting parse from compile and sharing one parsed carrier between a Vue
dual-runtime request's primary and secondary sub-compiles produced ZERO byte
change in `StandaloneCompiler::compile`'s own output relative to the accepted
base — measured, not argued from construction, and now over every field that
output exposes rather than most of them. Together with the in-tree four-route
identity test, that closes the base-comparison gap: the direct route is
unchanged, and the three new routes match it.

The digests differ from the ones an earlier revision of this record carried.
That is the widened oracle, not a behavioural move: the two ends agree with each
other under the wider oracle exactly as they did under the narrower one, and
`diff` of the two runs' output is empty.

## When this must be re-taken

On ANY change to `standalone.rs`, `assembly/publish.rs`, the Svelte runtime, or the
parser's retained-byte accounting — including a change that is only a comment. This
record has now been wrong twice in the same way, and both times the wrongness was in a
shortcut rather than in the measurement:

- An earlier revision named a candidate commit that a later rebase removed from the
  branch, while those files had moved several hundred lines underneath it.
- Its replacement tried to avoid re-taking by asserting HEAD-relatively that every later
  commit touched only `#[cfg(test)]` modules. A single doc-comment edit to `standalone.rs`
  made that literally false while leaving the measurement perfectly valid — which is
  exactly the trap: a true measurement wrapped in a false claim reads as verified and
  is not.

So there is no shortcut in this record any more. The candidate SHA above is the tree the
probe actually ran on. Change any of those files, for any reason, and run it again.

To check the record against a later tree without re-running: every one of those four paths
must satisfy `git rev-parse <candidate>:<path> == git rev-parse HEAD:<path>`. If any
differs — including by a comment — this record is out of date and the digests above are
not evidence for HEAD.
