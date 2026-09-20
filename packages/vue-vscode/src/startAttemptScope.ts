/**
 * Lifetime bag for one owned activation or language-server start attempt.
 *
 * Registrations and processes added here are disposed with the attempt:
 * failure tears the attempt down immediately, and a spent attempt cannot
 * hand the workspace an ownerless server or listener.
 */
export interface Disposable {
  dispose(): void;
}

export interface Lifetime {
  add(...items: Disposable[]): void;
}

export class StartAttemptScope implements Disposable, Lifetime {
  private items: Disposable[] = [];
  private disposed = false;

  /**
   * True once the attempt has been torn down.
   *
   * Work that was already in flight when disposal landed reads this to decide
   * whether it still has an owner — a restart that finishes afterwards must not
   * hand the workspace a server this attempt will never shut down.
   */
  get isDisposed(): boolean {
    return this.disposed;
  }

  add(...items: Disposable[]): void {
    for (const item of items) {
      if (this.disposed) {
        item.dispose();
        continue;
      }
      this.items.push(item);
    }
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    // Reverse order: later registrations may depend on earlier ones.
    for (const item of this.items.splice(0).reverse()) {
      try {
        item.dispose();
      } catch {
        // One faulty disposer must not strand the rest of the attempt.
      }
    }
  }
}
