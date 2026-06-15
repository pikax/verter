// Vendored minimal `svelte/store` type declarations for the B8g store
// auto-subscription (F11) type-check validity gate (Testing-Hermeticity: no npm
// install, no third-party checkout). Pinned to the audited Svelte 5.56.x store
// contract the F11 projection depends on — a DELIBERATELY minimal subset
// (`Readable<T>` / `Writable<T>` / `get`), not the full package.

/** Callback to inform of a value update. */
export type Subscriber<T> = (value: T) => void;

/** Unsubscribes from value updates. */
export type Unsubscriber = () => void;

/** Callback to update a value. */
export type Updater<T> = (value: T) => T;

/** Readable interface for subscribing. */
export interface Readable<T> {
  /**
   * Subscribe on value changes.
   * @param run subscription callback
   * @param invalidate cleanup callback
   */
  subscribe(this: void, run: Subscriber<T>, invalidate?: () => void): Unsubscriber;
}

/** Writable interface for both updating and subscribing. */
export interface Writable<T> extends Readable<T> {
  /**
   * Set value and inform subscribers.
   * @param value to set
   */
  set(this: void, value: T): void;
  /**
   * Update value using callback and inform subscribers.
   * @param updater callback
   */
  update(this: void, updater: Updater<T>): void;
}

/** Get the current value from a store by subscribing and immediately unsubscribing. */
export function get<T>(store: Readable<T>): T;
