// fluff
export type NotificationTyped = {
  onNotification: <T extends NotificationType>(
    type: T,
    cb: (e: NotificationParams[T]) => any,
  ) => Promise<void>;

  sendNotification: <T extends NotificationType>(
    type: T,
    params: NotificationParams[T],
  ) => Promise<void>;
};

type GenericNotificationHelper = {
  onNotification: any | ((type: any, cb: any) => Promise<void>);
  sendNotification: any | ((type: any, params: any) => Promise<void>);
};

export type OmitNotification<T> = Omit<T, keyof NotificationTyped>;
export type PatchNotification<T> = OmitNotification<T> & NotificationTyped;

export function createNotificationHelper<T extends GenericNotificationHelper>(
  connection: T,
): PatchNotification<T>;
export function createNotificationHelper<T extends GenericNotificationHelper>(
  connection?: T,
): PatchNotification<T> | undefined;
export function createNotificationHelper<T extends GenericNotificationHelper>(
  connection?: T,
): PatchNotification<T> | undefined {
  return connection;
}

// /fluff

export enum NotificationType {
  OnDidChangeTsOrJsFile = "$/onDidChangeTsOrJsFile",
  OnFileChanged = "$/onFileChanged",
  WatcherStateChanged = "$/verter/watcherStateChanged",
  TsgoStarted = "$/verter/tsgoStarted",
  TypeProviderStarted = "$/verter/typeProviderStarted",
  Heartbeat = "$/verter/heartbeat",
  Ready = "$/verter/ready",
  McpReady = "$/verter/mcpReady",
  ViteConfigTrustRequired = "$/verter/viteConfigTrustRequired",
  TypeProviderStatus = "$/verter/typeProviderStatus",
  TypeProviderSyncComplete = "$/verter/typeProviderSyncComplete",
  /**
   * The LSP publishes the resolved per-workspace carrier-store directory it
   * writes compiled `.vue`/`.svelte` carriers into. The extension forwards this
   * dir to VS Code's OWN TypeScript server via `configurePlugin`, so a plain
   * `.ts` opened in VS Code (served by VS Code's TS service, NOT the
   * LSP-spawned tsserver) reads the same store and gets real types for imported
   * carriers. The dir is authoritative — the LSP is the single source of the
   * `<temp>/verter-carrier-store/<host-version>/<workspace-hash>/` path
   * derivation (`blake3` over the canonicalized workspace root + the LSP package
   * version), which the extension cannot reproduce without mirroring that exact
   * recipe.
   */
  CarrierStoreReady = "$/verter/carrierStoreReady",
}

export type FileNotificationChange = "create" | "update" | "delete";

export type NotificationParams = {
  [NotificationType.OnDidChangeTsOrJsFile]: {
    uri: string;
    changes: Array<{
      text: string;
      range: {
        start: {
          line: number;
          character: number;
        };
        end: {
          line: number;
          character: number;
        };
      };
    }>;
  };
  [NotificationType.OnFileChanged]: {
    uri: string;
    type: FileNotificationChange;
  };
  [NotificationType.WatcherStateChanged]: {
    workspaceRoot: string;
    reason: "overflow" | "restart" | "dropped_state";
  };
  [NotificationType.TsgoStarted]: {
    pid: number;
  };
  [NotificationType.TypeProviderStarted]: {
    pid: number;
    kind: "tsgo" | "tsserver";
  };
  [NotificationType.Heartbeat]: {
    timestamp: number;
  };
  [NotificationType.Ready]: {
    gen: number;
  };
  [NotificationType.McpReady]: {
    port: number;
  };
  [NotificationType.ViteConfigTrustRequired]: {
    configPath: string;
    workspaceRoot: string;
    reason: string;
  };
  [NotificationType.TypeProviderStatus]: {
    kind: "tsgo" | "tsserver" | "editor-tsserver" | "none";
    reason?: string;
  };
  [NotificationType.TypeProviderSyncComplete]: {
    gen: number;
  };
  [NotificationType.CarrierStoreReady]: {
    /**
     * The absolute, forward-slash-normalized per-workspace carrier-store dir the
     * LSP publishes carriers into (and the dir the `@verter/typescript-plugin`
     * reads via `carrierStoreDir`). Identical to the dir the LSP delivers to its
     * own spawned tsserver through `VERTER_CARRIER_STORE_DIR`.
     */
    carrierStoreDir: string;
  };
};
