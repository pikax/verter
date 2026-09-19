# Contribution

Guide structure for contributing to the Verter SDK and the official
extensions. Status: **pending** — the executable example slot for this topic
(`sdk/contribution`) is produced by the SDK authoring tools node; see
[`../manifest.json`](../manifest.json).

The finished contribution guide must cover:

- how an SDK change reaches a published `@verter/*` package (workspace
  package, public `exports`, version pin in the example home)
- how an official-extension change reaches a shipped `verter-*` bin or the
  VS Code extension, using shipped bins only — never `cargo` invocations
- the review path for documentation examples: an example enters this home
  only through the docs reference harness, as static proof over public
  exports and shipped bins

Existing shipped truth this guide links to, and must not duplicate:
[repository contributing docs](../../../docs/contributing/index.md) and the
[VS Code extension guide](../../../docs/editor/vscode.md).

Required example slot: an executable contribution walkthrough
(`sdk/contribution`), supplied with an executable source file plus a
shipped-bin entry. A static sample manifest does not satisfy this topic.
