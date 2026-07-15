/** Versioned handshake between the VS Code client and its editor-owned tsserver plugin. */
export const EDITOR_TSSERVER_ATTESTATION_VERSION = 1;
export const EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY = "editorTsserverAttestation";
export const EDITOR_TSSERVER_ATTESTATION_FILE_PREFIX = "verter-editor-tsserver-";

export interface EditorTsserverAttestationRequest {
  directory: string;
  nonce: string;
}

export interface EditorTsserverAttestationReceipt {
  version: typeof EDITOR_TSSERVER_ATTESTATION_VERSION;
  nonce: string;
  pid: number;
  projects: string[];
}

export function editorTsserverAttestationFileName(nonce: string): string {
  return `${EDITOR_TSSERVER_ATTESTATION_FILE_PREFIX}${nonce}.json`;
}

export function parseEditorTsserverAttestationRequest(
  config: Record<string, unknown> | undefined,
): EditorTsserverAttestationRequest | undefined {
  const raw = config?.[EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY];
  if (raw === null || typeof raw !== "object") return undefined;
  const { directory, nonce } = raw as Record<string, unknown>;
  if (typeof directory !== "string" || directory.length === 0) return undefined;
  if (typeof nonce !== "string" || !/^[0-9a-f]{32}$/.test(nonce)) return undefined;
  return { directory, nonce };
}

export function parseEditorTsserverAttestationReceipt(
  raw: unknown,
  expectedNonce: string,
): EditorTsserverAttestationReceipt | undefined {
  if (raw === null || typeof raw !== "object") return undefined;
  const value = raw as Record<string, unknown>;
  if (value.version !== EDITOR_TSSERVER_ATTESTATION_VERSION) return undefined;
  if (value.nonce !== expectedNonce) return undefined;
  if (!Number.isSafeInteger(value.pid) || (value.pid as number) <= 0) return undefined;
  if (
    !Array.isArray(value.projects) ||
    value.projects.length === 0 ||
    !value.projects.every((project) => typeof project === "string" && project.length > 0)
  ) {
    return undefined;
  }
  return {
    version: EDITOR_TSSERVER_ATTESTATION_VERSION,
    nonce: expectedNonce,
    pid: value.pid as number,
    projects: [...new Set(value.projects as string[])].sort(),
  };
}
