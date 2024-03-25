import type { OmitNotification, NotificationTyped } from "./notifications";
import type { OmitRequest, RequestTyped } from "./request";

export * from "./notifications";
export * from "./request";

export type PatchClient<T> = OmitNotification<OmitRequest<T>> &
  NotificationTyped &
  RequestTyped;

export function patchClient<T extends {}>(client: T): PatchClient<T>;
export function patchClient<T>(client?: T): PatchClient<T> | undefined;
export function patchClient<T>(client: T): PatchClient<T> {
  return client as any;
}

export * as VirtualFiles from "./virtual";
