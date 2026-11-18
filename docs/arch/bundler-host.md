# Bundler host: how `@verter/unplugin` uses the native host

`@verter/unplugin` is a thin bundler adapter over one native `VerterHost`. The
host owns every answer about a file; the plugin owns when a bundler hook asks,
what it hands the bundler back, and what it remembers between hooks.

## Authority

The plugin decides:

- which bundler hook runs and with what bytes (the bundler is the authority for
  a carrier's current content, aliases, and its own CSS and TypeScript
  pipelines);
- which graph a hook call belongs to (client or SSR, dev or prod);
- the published modules of each carrier in each graph, and when they are retired;
- the sequence of one compile: register, resolve, hydrate, process, compile, publish.

The host decides, and the plugin never recomputes:

- **Canonical ids.** `canonicalizeId` before a host exists; `upsert().canonicalId`
  and `resolve().canonicalId` after. Every map in the package is keyed by a
  host id.
- **The request-id grammar.** `resolve(rawId)` and the host-free
  `parseRequestId(rawId)` return the node, marker, language, `scoped`, raw path
  and custom block type, or a typed rejection. `renderBundlerId` writes the id
  of any sub-module of a carrier in the host's own spelling.
- **Carrier classification.** `isCarrierSourcePath` and the language registry
  decide what is a carrier and which framework it belongs to. A session may
  register further suffixes (`carriers`) when it creates the host.
- **Resolution.** `Workspace.resolveImport(importer, specifier, phase, kind)`
  resolves a specifier for code or for types; `isPackageBacked` classifies a
  file as installed.
- **Hashes.** `upsert().contentHash` is a carrier's revision;
  `hashBlockContent` hashes supplied block content; the plugin hashes only to
  mint its own identities.
- **Offsets.** `getDocumentStructure(id, "utf16.v1")` answers in JavaScript
  string offsets.

## Session and catalog

`BundlerHostSession` is one admitted project and tool graph: the frozen
binding (adapter, root, mode, framework selection, options, registered
carriers), a local identity minted from it, the module catalog, an epoch and
the native host. It is the only place a `VerterHost` is constructed. The host
always has a workspace: the one supplied in the binding, else one opened at the
root. `restart()` raises the epoch, clears the catalog and closes the host;
work begun under an older epoch publishes nothing.

`identify(rawId, graph)` asks the host to read the id and answers one of:
a generated module (the export helper, a fixed id), a carrier module (with its
module id), `foreign` (not a carrier this session serves), or `rejected` with
the host's typed reason. A marker that disagrees with the carrier's framework
is rejected.

`ModuleCatalog` is the only store of published modules. A record holds one
carrier in one graph: its revision and epoch, the render profile, the main
module, authored and compiled styles, the other compiled nodes, and a closure
of host ids of every file the record was built from. A change to any file in a
closure retires the record.

## Demand pipeline

`CarrierPipeline.demand(key, source, ctx)` is the one route that compiles a
carrier:

1. register the bytes (`upsert`) and take the revision from the host; a record
   of the same revision and epoch is returned without further work, and a
   running flight of the same revision is joined;
2. resolve external sources and module references (the bundler's resolver
   first, the workspace for what it does not answer) and contribute the routes
   through the route table;
3. hydrate the types a Vue carrier's macros need;
4. hand externally processed block content to the host;
5. compile once, admit the response, apply the SSR rewrites and publish.

A newer revision supersedes a running flight; a flight that finishes under an
older epoch is discarded. Nothing is published before the compile is admitted.
A compile that fails for missing inputs rejects with a `need-inputs` refusal.

The route table is the single writer of each owner's import routes: it keeps
the union of the module and declaration contributions and pushes the whole
union after each contribution.

## Style lanes

Where the bundler has a CSS pipeline and the carrier is Vue (`bundler-css`),
the plugin serves each style block's authored bytes and the bundler's pipeline
processes them; the plugin then applies native scoping to what comes back.
Elsewhere (`compiled`), the compile produces final CSS and the plugin serves
it, scoping only styles in a language the compile did not already scope.

## Type hydration

A Vue carrier whose macros name imported types needs those declarations in the
host. `typeTargetOf` asks the bundler for the runtime target, so its aliases
apply, then asks the workspace to resolve that target for types. The hydrator
reads each declaration file once, registers it, contributes its routes, and
walks its imports; it never enters a package from a bare import. A file counts
as hydrated only when its promise has resolved, so concurrent demands wait for
each other; a failed file is forgotten and tried again. Invalidating a file
forgets it and removes the copy the hydrator registered.

## Processed content

A block the host cannot process itself (Pug, CoffeeScript, Sass and the like
outside a bundler CSS pipeline, custom blocks) is processed by an external tool.
Each result travels as an envelope: the host's stamps returned unchanged, the
stage, the tool's identity and version, a fingerprint of its options, the
code and map with their hashes, and the files the tool read. The host accepts a
source map that spans several sources when one of them is the block. Every file
the tool read is registered with the bundler for watching and joins the
record's closure. The host's typed refusal code decides whether a refused
override is `stale` or `need-inputs`.

## Hooks

`resolveId`, `load`, `transformInclude`, `transform`, `buildStart` (pre-compile),
`watchChange` and Vite's `handleHotUpdate` each identify the id through the
session and then plan: pure functions map an identification to a load or
transform plan, and executors serve only what the carrier's record holds. A
request for a block the carrier does not have is refused by name. The public
`parseVueRequest` projects the host's grammar.
