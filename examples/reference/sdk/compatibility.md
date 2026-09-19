# Compatibility

Guide structure for compatibility of the Verter SDK and official
extensions. Status: **pending** — the executable example slot for this
topic (`sdk/compatibility`) is produced by the SDK authoring tools node;
see [`../manifest.json`](../manifest.json).

The finished compatibility guide must cover:

- the pinned engine and package basis every example declares (the example
  home pins `node`, `typescript` and `vue` versions in
  [`../manifest.json`](../manifest.json))
- the stability classes of the public packages and the deprecation notice
  rule for Stable APIs, deferring to the API stability page as the sole
  authority
- what counts as a compatibility claim: an upstream feature is Verter
  support only with the owned integration and public operating evidence —
  never a roadmap mention

Existing shipped truth this guide links to, and must not duplicate:
[API stability](../../../docs/api-stability.md) and the
[migration notes home](../../../docs/migration/getDeclaredComponentMeta-removal.md).

Required example slot: an executable compatibility walkthrough
(`sdk/compatibility`) exercising a public export at the pinned versions
with a shipped-bin entry. A static sample manifest does not satisfy this
topic.
