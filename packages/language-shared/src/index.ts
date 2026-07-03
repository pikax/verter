import type { OmitNotification, NotificationTyped } from "./notifications";
import type { OmitRequest, RequestTyped } from "./request";

export * from "./notifications";
export * from "./request";
export * from "./statistics";
export * from "./analysis";
export * from "./routes";
export * from "./generated";
// The descriptor-generated client framework manifest — the single authority
// for the VS Code extension + TS-plugin client wiring (activation, document
// selector, configurePlugin trigger ids). Byte-pinned by the Rust
// `client_framework_manifest_ts_freshness` guard. (File-watching is server-
// owned, so the manifest carries no client watch-glob surface.)
export * from "./client-framework-manifest.generated";
// The descriptor-generated virtual-file naming column mirror — the single
// authority for every adapter's IDE / API / testing-API / sidecar /
// declaration-surface virtual-file suffixes. Byte-pinned by the Rust
// `virtual_file_naming_ts_freshness` guard.
export * from "./virtual-file-naming.generated";
// The browser-safe carrier naming/classification CORE derived from that
// column — ONE implementation shared by the Node tsserver plugin and the
// WASM in-context LanguageService (no Node builtin imports).
export * from "./carrier/naming";
// The carrier-store reader CONTRACT (interface + manifest value types). The
// Node tsserver plugin implements it with a Node-fs disk adapter; a
// browser/WASM host implements it over in-memory snapshots.
export * from "./carrier/store";
// The strict fail-closed V3 carrier position mapper — ONE mapper shared by
// both instantiations (greatest-lower-bound lookup, anti-extrapolation extent
// bound, same-source span endpoints or drop, atomic workspace edits).
export * from "./carrier/mapper";
// The companion→source response-remap orchestration over the reader interface
// and the strict mapper (definition/reference/rename/code-action remapping +
// the inserted-import specifier rewrite).
export * from "./carrier/remap";
// The carrier script-kind / root-membership policy (TS enums INJECTED — this
// package never imports the `typescript` package at module scope).
export * from "./carrier/policy";

export type PatchClient<T> = OmitNotification<OmitRequest<T>> & NotificationTyped & RequestTyped;

export function patchClient<T extends {}>(client: T): PatchClient<T>;
export function patchClient<T>(client?: T): PatchClient<T> | undefined;
export function patchClient<T>(client: T): PatchClient<T> {
  return client as any;
}

export * as VirtualFiles from "./virtual";
