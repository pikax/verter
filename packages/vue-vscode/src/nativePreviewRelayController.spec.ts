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
