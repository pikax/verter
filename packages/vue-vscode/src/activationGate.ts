/**
 * Serialize extension activation so concurrent callers share one startup path.
 */
export interface ActivationGate<T> {
  run(): Promise<T>;
  isActive(): boolean;
  reset(): void;
}

export function createActivationGate<T>(start: () => Promise<T>): ActivationGate<T> {
  let active = false;
  let current: Promise<T> | undefined;

  return {
    run(): Promise<T> {
      if (current) {
        return current;
      }

      const attempt = start().then(
        (value) => {
          if (current === attempt) {
            active = true;
          }
          return value;
        },
        (error) => {
          // Only the attempt the gate still holds may retire the shared
          // promise. A run superseded by reset() must not clear its
          // replacement's promise, or a late stale rejection would unlock a
          // third start while the replacement activation is still live.
          if (current === attempt) {
            active = false;
            current = undefined;
          }
          throw error;
        },
      );
      current = attempt;

      return attempt;
    },

    isActive(): boolean {
      return active;
    },

    reset(): void {
      active = false;
      current = undefined;
    },
  };
}
