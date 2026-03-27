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

      current = start().then(
        (value) => {
          active = true;
          return value;
        },
        (error) => {
          active = false;
          current = undefined;
          throw error;
        },
      );

      return current;
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
