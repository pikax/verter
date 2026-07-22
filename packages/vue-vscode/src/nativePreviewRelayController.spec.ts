import { describe, expect, it, vi } from "vitest";

import {
  NativePreviewRelayController,
  type NativePreviewApi,
} from "./nativePreviewRelayController";

function fakeApi(pipe = "editor-program-pipe") {
  let listener: (() => void) | undefined;
  const api: NativePreviewApi = {
    onLanguageServerInitialized(callback) {
      listener = callback;
      callback();
      return { dispose: () => (listener = undefined) };
    },
    initializeAPIConnection: vi.fn(async () => pipe),
  };
  return { api, fireInitialized: () => listener?.() };
}

describe("NativePreviewRelayController", () => {
  it("activates an enabled inactive Native Preview on the staged relay, attests its Program, and restores config", async () => {
    const { api } = fakeApi();
    let configuredTsdk: string | undefined = "/user/tsgo";
    let advertised = false;
    const writes: Array<string | undefined> = [];
    const restart = vi.fn(async () => {});
    const controller = new NativePreviewRelayController({
      stagedTsdk: "/tmp/relay-tsdk",
      isExtensionActive: () => false,
      activate: async () => {
        expect(configuredTsdk).toBe("/tmp/relay-tsdk");
        // Native Preview's activation promise resolves after its enabled server
        // has started. The restart command does not exist before that startup.
        advertised = true;
        return api;
      },
      restart,
      readGlobalTsdk: () => configuredTsdk,
      writeGlobalTsdk: async (value) => {
        writes.push(value);
        configuredTsdk = value;
      },
      hasAdvertisement: () => advertised,
      timeoutMs: 15,
      pollMs: 1,
    });

    await expect(controller.establish()).resolves.toBe("editor-program-pipe");
    expect(restart).not.toHaveBeenCalled();
    expect(api.initializeAPIConnection).toHaveBeenCalledTimes(1);
    expect(writes).toEqual(["/tmp/relay-tsdk", "/user/tsgo"]);
    expect(configuredTsdk).toBe("/user/tsgo");
  });

  it("restarts an already-active Native Preview before attestation", async () => {
    const { api } = fakeApi();
    let configuredTsdk: string | undefined;
    let advertised = false;
    const restart = vi.fn(async () => {
      expect(configuredTsdk).toBe("/tmp/relay-tsdk");
      advertised = true;
    });
    const controller = new NativePreviewRelayController({
      stagedTsdk: "/tmp/relay-tsdk",
      isExtensionActive: () => true,
      activate: async () => api,
      restart,
      readGlobalTsdk: () => configuredTsdk,
      writeGlobalTsdk: async (value) => {
        configuredTsdk = value;
      },
      hasAdvertisement: () => advertised,
    });

    await expect(controller.establish()).resolves.toBe("editor-program-pipe");
    expect(restart).toHaveBeenCalledTimes(1);
    expect(configuredTsdk).toBeUndefined();
  });

  it("fails closed and restores config when the current Program cannot be attested", async () => {
    const { api } = fakeApi("");
    let configuredTsdk: string | undefined = "/user/tsgo";
    const controller = new NativePreviewRelayController({
      stagedTsdk: "/tmp/relay-tsdk",
      isExtensionActive: () => false,
      activate: async () => api,
      restart: async () => {},
      readGlobalTsdk: () => configuredTsdk,
      writeGlobalTsdk: async (value) => {
        configuredTsdk = value;
      },
      hasAdvertisement: () => true,
      // Attestation now RETRIES within this budget rather than declining on the
      // first miss, so the budget is what bounds this fail-closed case.
      timeoutMs: 40,
      pollMs: 1,
    });

    await expect(controller.establish()).rejects.toThrow(/empty API pipe/i);
    expect(configuredTsdk).toBe("/user/tsgo");
  });

  it("fails closed when no relay advertisement appears and still restores config", async () => {
    const { api } = fakeApi();
    let configuredTsdk: string | undefined = "/user/tsgo";
    const controller = new NativePreviewRelayController({
      stagedTsdk: "/tmp/relay-tsdk",
      isExtensionActive: () => false,
      activate: async () => api,
      restart: async () => {},
      readGlobalTsdk: () => configuredTsdk,
      writeGlobalTsdk: async (value) => {
        configuredTsdk = value;
      },
      hasAdvertisement: () => false,
      timeoutMs: 15,
      pollMs: 1,
    });

    await expect(controller.establish()).rejects.toThrow(/advertisement|timed out/i);
    expect(configuredTsdk).toBe("/user/tsgo");
  });

  it("re-interposes once when Native Preview later initializes without the relay", async () => {
    const { api, fireInitialized } = fakeApi();
    let configuredTsdk: string | undefined = "/user/tsgo";
    let advertised = true;
    let releaseRestart: (() => void) | undefined;
    const restart = vi.fn(() => {
      return new Promise<void>((resolve) => {
        releaseRestart = () => {
          advertised = true;
          resolve();
        };
      });
    });
    const controller = new NativePreviewRelayController({
      stagedTsdk: "/tmp/relay-tsdk",
      isExtensionActive: () => false,
      activate: async () => api,
      restart,
      readGlobalTsdk: () => configuredTsdk,
      writeGlobalTsdk: async (value) => {
        configuredTsdk = value;
      },
      hasAdvertisement: () => advertised,
      onBackgroundError: (error) => {
        throw error;
      },
    });
    await controller.establish();
    expect(restart).not.toHaveBeenCalled();

    advertised = false;
    fireInitialized();
    fireInitialized();
    await vi.waitFor(() => expect(restart).toHaveBeenCalledTimes(1));
    releaseRestart?.();
    await vi.waitFor(() => expect(configuredTsdk).toBe("/user/tsgo"));
    expect(api.initializeAPIConnection).toHaveBeenCalledTimes(2);
  });
});

describe("NativePreviewRelayController — Native Preview has no session yet", () => {
  /**
   * Native Preview activates on `onLanguage:{java,type}script[react]` only, and
   * its public `initializeAPIConnection` throws `Language server is not running.`
   * until a session exists. Forcing activation does NOT create one, so a
   * workspace whose open document is a `.vue` carrier attested against an
   * extension that had not started a server — and the whole tier declined.
   */
  function sessionlessApi(pipe = "editor-program-pipe") {
    let sessionStarted = false;
    const api: NativePreviewApi = {
      onLanguageServerInitialized(callback) {
        callback();
        return { dispose: () => {} };
      },
      initializeAPIConnection: vi.fn(async () => {
        if (!sessionStarted) throw new Error("Language server is not running.");
        return pipe;
      }),
    };
    return { api, startSession: () => void (sessionStarted = true) };
  }

  it("starts a session and attests, instead of declining the tier on the first throw", async () => {
    const { api, startSession } = sessionlessApi();
    const start = vi.fn(startSession);
    let configuredTsdk: string | undefined = "/user/tsgo";
    const controller = new NativePreviewRelayController({
      stagedTsdk: "/tmp/relay-tsdk",
      isExtensionActive: () => false,
      activate: async () => api,
      restart: async () => {},
      readGlobalTsdk: () => configuredTsdk,
      writeGlobalTsdk: async (value) => void (configuredTsdk = value),
      hasAdvertisement: () => true,
      startSession: start,
      timeoutMs: 200,
      pollMs: 1,
    });

    await expect(controller.establish()).resolves.toBe("editor-program-pipe");
    expect(start).toHaveBeenCalledTimes(1);
    expect(configuredTsdk).toBe("/user/tsgo");
  });

  it("reports the engine's own reason, not a generic timeout, when no session ever starts", async () => {
    const api: NativePreviewApi = {
      onLanguageServerInitialized: () => ({ dispose: () => {} }),
      initializeAPIConnection: vi.fn(async () => {
        throw new Error("Language server is not running.");
      }),
    };
    let configuredTsdk: string | undefined = "/user/tsgo";
    const controller = new NativePreviewRelayController({
      stagedTsdk: "/tmp/relay-tsdk",
      isExtensionActive: () => false,
      activate: async () => api,
      restart: async () => {},
      readGlobalTsdk: () => configuredTsdk,
      writeGlobalTsdk: async (value) => void (configuredTsdk = value),
      hasAdvertisement: () => true,
      startSession: () => {},
      timeoutMs: 40,
      pollMs: 1,
    });

    await expect(controller.establish()).rejects.toThrow(/Language server is not running/);
    // A failed tier must never leave the user's Native Preview pointed at the
    // staged relay tsdk.
    expect(configuredTsdk).toBe("/user/tsgo");
    expect(
      (api.initializeAPIConnection as ReturnType<typeof vi.fn>).mock.calls.length,
    ).toBeGreaterThan(1);
  });
});
