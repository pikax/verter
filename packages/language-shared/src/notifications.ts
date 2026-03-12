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
  TsgoStarted = "$/verter/tsgoStarted",
  TypeProviderStarted = "$/verter/typeProviderStarted",
  Heartbeat = "$/verter/heartbeat",
  Ready = "$/verter/ready",
  McpReady = "$/verter/mcpReady",
  ViteConfigTrustRequired = "$/verter/viteConfigTrustRequired",
  TypeProviderStatus = "$/verter/typeProviderStatus",
  TsgoLimitation = "$/verter/tsgoLimitation",
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
    kind: "tsgo" | "tsserver" | "none";
    reason?: string;
  };
  [NotificationType.TsgoLimitation]: {
    message: string;
  };
};
