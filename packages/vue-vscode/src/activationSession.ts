/**
 * One extension activation session: the composition root's owned lifetime,
 * not an extension-wide service locator.
 *
 * `createActivationRoot` serializes activate() through the existing
 * activation gate, disposes registrations on failure or deactivate(), and
 * refuses to stack a second live session's listeners or processes.
 */
import { createActivationGate } from "./activationGate";
import { StartAttemptScope, type Disposable } from "./startAttemptScope";

export class ActivationSession implements Disposable {
  readonly scope = new StartAttemptScope();
  private mcpEndpoint: unknown;
  private mcpRetry: (() => void) | undefined;

  get isDisposed(): boolean {
    return this.scope.isDisposed;
  }

  setMcpEndpoint(record: unknown): void {
    if (this.scope.isDisposed) {
      return;
    }
    this.mcpEndpoint = record;
  }

  getMcpEndpoint(): unknown {
    return this.mcpEndpoint;
  }

  setMcpRetry(fn: (() => void) | undefined): void {
    if (this.scope.isDisposed) {
      this.mcpRetry = undefined;
      return;
    }
    this.mcpRetry = fn;
  }

  retryMcp(): void {
    this.mcpRetry?.();
  }

  dispose(): void {
    this.mcpEndpoint = undefined;
    this.mcpRetry = undefined;
    this.scope.dispose();
  }
}

export interface ActivationRoot<TRuntime> {
  ensureSession(): ActivationSession;
  run(): Promise<TRuntime>;
  deactivate(): void;
  getRuntime(): TRuntime | undefined;
  getSession(): ActivationSession | undefined;
}

export function createActivationRoot<TRuntime>(
  start: (session: ActivationSession) => Promise<TRuntime>,
): ActivationRoot<TRuntime> {
  let session: ActivationSession | undefined;
  let runtime: TRuntime | undefined;
  const gate = createActivationGate(async () => {
    if (!session) {
      throw new Error("Verter activation session was not initialized");
    }
    try {
      runtime = await start(session);
      return runtime;
    } catch (error) {
      session.dispose();
      session = undefined;
      runtime = undefined;
      throw error;
    }
  });

  return {
    ensureSession(): ActivationSession {
      if (!session || session.isDisposed) {
        session = new ActivationSession();
        gate.reset();
      }
      return session;
    },

    run(): Promise<TRuntime> {
      return gate.run();
    },

    deactivate(): void {
      session?.dispose();
      session = undefined;
      runtime = undefined;
      gate.reset();
    },

    getRuntime(): TRuntime | undefined {
      return runtime;
    },

    getSession(): ActivationSession | undefined {
      return session;
    },
  };
}
