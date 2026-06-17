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

export type PatchClient<T> = OmitNotification<OmitRequest<T>> & NotificationTyped & RequestTyped;

export function patchClient<T extends {}>(client: T): PatchClient<T>;
export function patchClient<T>(client?: T): PatchClient<T> | undefined;
export function patchClient<T>(client: T): PatchClient<T> {
  return client as any;
}

export * as VirtualFiles from "./virtual";
