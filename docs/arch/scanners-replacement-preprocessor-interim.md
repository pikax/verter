# Scanners replacement — native content and stamped handoff contract

The JS/bundler `BlockPreprocessor` → host `applyBlockOverrides` integration is
permanent. Verter never executes a preprocessor or compiles a stylesheet. The
host alone admits bytes, either by reading a registered VFS source or by
validating a supplied result after the caller's asynchronous work completes.

## Closed availability

Every selected block has exactly one of these states:

| State | Meaning |
| --- | --- |
| `NativeAvailable` | Inline-authored or registered-VFS bytes are directly parseable by Verter. |
| `ProcessedContentRequired` | The declared language needs JS/bundler processing before Verter can consume it. |
| `SuppliedAvailable` | Supplied bytes passed all correlation, stamp, hash, and map checks. |
| `Missing` | The referenced content source is absent. |
| `Conflict` | The carrier declares incompatible simultaneous sources. |
| `Stale` | The requested basis no longer names the current owner/block artifact. |

Matches are exhaustive. Unavailable or untrusted bytes never become empty or
identity success: analysis fails open against false unused claims, while edits
fail closed against untrusted spans.

Native external style and script sources are registered as their own VFS files
and retain their own source-space identity. Inline style dialects `css`,
`scss`, `sass`, `less`, and `stylus` are parsed through
`verter_css_syntax`; no synthetic SFC is built and the carrier is not
reparsed. Source-located analysis for external or supplied style bytes remains
unavailable (`css: None`) until its output carries an explicit source-space
descriptor; publishing block-local offsets as carrier-absolute spans is
forbidden.

## Sealed handoff and capture point

The session, protocol/FFI, NAPI, WASM, native TypeScript, and unplugin surfaces
carry nominal opaque block, owner-revision, artifact, content-basis,
source-space, correlation, content-artifact, and hash tokens. Untrusted wire
strings are parsed at the protocol boundary; internal APIs do not admit raw
strings in their place.

Before capture, a request may name only its optional prior basis. Once the host
selects bytes, it issues a captured echo containing the exact pre-capture
request and newly minted basis. The caller must return that captured echo
unchanged with hashes for code and any source map. After the asynchronous
preprocessor await, the host validates the complete echo, current owner and
artifact stamps, language, hashes, source-map structure, and source spaces
under the same publication fence used by owner updates. A stale, missing,
mismatched, malformed, duplicate, or replayed result is a typed terminal
refusal and mutates no content cache. Pending and retained terminal
correlations are both bounded.

Accepted native and supplied artifacts expose explicit source-space
descriptors, a final output space, immediate qualified maps, and the composed
map. Native bytes carry an identity map. Supplied bytes receive a distinct
host-minted derived output space and content-artifact token, so transformed
code can never masquerade as the authored input space.

Block ordinals are private parse-time selectors only. They are not handoff
identity and are absent from public result DTOs.

## Compiler lowering boundary

Classification and admission do not imply compiler availability. Selected
template or script bytes outside the carrier source space return typed
runtime-unavailable and IDE-unavailable results. They are not compiled in an
isolated pass, and the IDE surface is never emitted without them. Multi-unit
lowering must first provide script-to-template semantic transfer, composed
multi-source maps, and per-output source-space descriptors.

Selection returns one origin and one byte sequence. In this boundary, supplied
results are admitted only for `ProcessedContentRequired`; a native-readable
block remains native-authoritative. Validated supplied-over-native precedence
is deferred until the same multi-unit lowering can consume that supplied
artifact truthfully. A carrier that declares incompatible authored sources is
`Conflict`, never two simultaneously live sources.

The synthetic `ContentOverrideWithParse` cache and its callers are deleted.
`applyBlockOverrides` remains as the admission boundary; B-83 deletion does
not apply.

## External refresh ordering

Bundler hooks resolve external `src` content before the host request is
applied. The resolved bytes are registered under the host-minted canonical ID,
then the owner is byte-identically re-upserted once. That refreshed owner
result supplies the stamps echoed by preprocessing. Selection itself is
read-only and only observes registered VFS snapshots; it never performs file
I/O, registers sources, or mutates a cache.
