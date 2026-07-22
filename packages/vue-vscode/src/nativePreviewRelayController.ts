/**
 * Bounded, testable orchestration for making Native Preview own Verter's relay.
 *
 * The controller never spawns an engine. It temporarily points Native Preview at
 * a staged `tsgo` alias, waits for that editor-owned relay to advertise, and uses
 * Native Preview's public API to attest the current language client's exact
 * Program. The user's global tsdk value is restored after every transition.
 */

export interface DisposableLike {
  dispose(): void;
}

export interface NativePreviewApi {
  onLanguageServerInitialized(listener: () => void): DisposableLike;
  initializeAPIConnection(pipe?: string): Promise<string>;
}

export interface NativePreviewRelayControllerOptions {
  stagedTsdk: string;
  isExtensionActive(): boolean;
  activate(): Promise<NativePreviewApi>;
  restart(): Promise<void>;
  readGlobalTsdk(): string | undefined;
  writeGlobalTsdk(value: string | undefined): Promise<void>;
  hasAdvertisement(): boolean;
  /**
   * Give Native Preview a reason to start a language-server session.
   *
   * Forcing activation does not create one: the extension activates on
   * `onLanguage:{java,type}script[react]` and starts its server for those
   * documents, so a workspace whose open editor is a `.vue`/`.svelte` carrier
   * leaves it activated with no session. Invoked at most once, only after the
   * first attestation reports no running server.
   */
  startSession?(): PromiseLike<unknown> | unknown;
  timeoutMs?: number;
  pollMs?: number;
  onBackgroundError?(error: unknown): void;
}

const DEFAULT_TIMEOUT_MS = 10_000;
const DEFAULT_POLL_MS = 25;

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error(`${label} timed out after ${timeoutMs} ms`)),
          timeoutMs,
        );
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

export class NativePreviewRelayController implements DisposableLike {
  private readonly timeoutMs: number;
  private readonly pollMs: number;
  private api: NativePreviewApi | undefined;
  private initializedListener: DisposableLike | undefined;
  private transition: Promise<string> | undefined;
  private disposed = false;

  constructor(private readonly opts: NativePreviewRelayControllerOptions) {
    this.timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    this.pollMs = opts.pollMs ?? DEFAULT_POLL_MS;
  }

  establish(): Promise<string> {
    if (this.disposed) {
      return Promise.reject(new Error("Native Preview relay controller is disposed"));
    }
    return this.runTransition(false);
  }

  dispose(): void {
    this.disposed = true;
    this.initializedListener?.dispose();
    this.initializedListener = undefined;
  }

  private runTransition(restartExisting: boolean): Promise<string> {
    if (this.transition) return this.transition;

    const transition = this.transitionOnce(restartExisting).finally(() => {
      if (this.transition === transition) this.transition = undefined;
    });
    this.transition = transition;
    return transition;
  }

  private async transitionOnce(restartExisting: boolean): Promise<string> {
    const previousTsdk = this.opts.readGlobalTsdk();
    let temporaryTsdkWritten = false;
    let primaryError: unknown;
    try {
      await withTimeout(
        this.opts.writeGlobalTsdk(this.opts.stagedTsdk),
        this.timeoutMs,
        "configuring Native Preview relay tsdk",
      );
      temporaryTsdkWritten = true;

      if (!this.api) {
        const wasActive = this.opts.isExtensionActive();
        this.api = await withTimeout(
          this.opts.activate(),
          this.timeoutMs,
          "activating Native Preview",
        );
        // Native Preview only registers its restart command after a server
        // session starts. For an enabled inactive extension, activation awaits
        // that startup and already observes the staged tsdk. An existing active
        // session must be restarted because it predates the temporary tsdk.
        if (wasActive) {
          await withTimeout(
            this.opts.restart(),
            this.timeoutMs,
            "restarting Native Preview on the relay",
          );
        }
      } else if (restartExisting) {
        await withTimeout(
          this.opts.restart(),
          this.timeoutMs,
          "restarting Native Preview on the relay",
        );
      }

      const apiPipe = await this.attestCurrentProgram(this.api);

      await this.waitForAdvertisement();
      this.installRestartListener();
      return apiPipe;
    } catch (error) {
      primaryError = error;
      throw error;
    } finally {
      if (temporaryTsdkWritten) {
        try {
          await withTimeout(
            this.opts.writeGlobalTsdk(previousTsdk),
            this.timeoutMs,
            "restoring Native Preview tsdk",
          );
        } catch (restoreError) {
          if (primaryError === undefined) throw restoreError;
          this.opts.onBackgroundError?.(
            new Error(
              `Native Preview relay failed (${errorText(primaryError)}) and its tsdk could not be restored (${errorText(restoreError)})`,
            ),
          );
        }
      }
    }
  }

  /**
   * Attest Native Preview's CURRENT Program, waiting for it to have one.
   *
   * This public API delegates to the current Native Preview language client, so
   * a non-empty pipe attests its exact editor-owned Program. It reports
   * "Language server is not running." until a session exists, and a single
   * attempt the instant after activation loses to that startup — which is how a
   * carrier-only editor declined the whole shared tier in under a second. Nudge
   * a session into existence once, keep asking until the budget runs out, and
   * surface the engine's own reason rather than a generic timeout.
   */
  private async attestCurrentProgram(api: NativePreviewApi): Promise<string> {
    const deadline = Date.now() + this.timeoutMs;
    let nudged = false;
    for (;;) {
      try {
        const apiPipe = await withTimeout(
          api.initializeAPIConnection(),
          this.timeoutMs,
          "attesting Native Preview's current Program",
        );
        if (apiPipe.trim()) return apiPipe;
        throw new Error("Native Preview attestation returned an empty API pipe");
      } catch (error) {
        if (!nudged && this.opts.startSession) {
          nudged = true;
          try {
            await this.opts.startSession();
          } catch (startError) {
            // A nudge that fails must not replace the engine's own reason.
            this.opts.onBackgroundError?.(startError);
          }
        }
        if (Date.now() >= deadline) throw error;
        await new Promise<void>((resolve) => setTimeout(resolve, this.pollMs));
      }
    }
  }

  private async waitForAdvertisement(): Promise<void> {
    const started = Date.now();
    while (!this.opts.hasAdvertisement()) {
      if (Date.now() - started >= this.timeoutMs) {
        throw new Error(`relay advertisement timed out after ${this.timeoutMs} ms`);
      }
      await new Promise<void>((resolve) => setTimeout(resolve, this.pollMs));
    }
  }

  private installRestartListener(): void {
    if (this.initializedListener || !this.api) return;
    this.initializedListener = this.api.onLanguageServerInitialized(() => {
      if (this.disposed || this.transition || this.opts.hasAdvertisement()) return;
      void this.runTransition(true).catch((error) => this.opts.onBackgroundError?.(error));
    });
  }
}
