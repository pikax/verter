// Vendored minimal `UIMessage` declaration from the `ai` package
// for the §9.7 Pick / Omit golden snapshot stability. This is NOT
// a full mirror of the `ai` package; only the minimal type
// declarations needed to keep the goldens stable across upstream
// version changes.
//
// Per the B-B5-callback-pick sidecar §17.7 vendoring constraint:
// pull only the minimal type declarations needed; do NOT pull the
// entire `ai` package.

export type UIDataTypes = Record<string, unknown>;
export type UITools = Record<string, unknown>;

export interface UIMessage<
  TMetadata = unknown,
  TDataParts extends UIDataTypes = UIDataTypes,
  TTools extends UITools = UITools,
> {
  id: string;
  role: 'user' | 'assistant' | 'system';
  parts?: unknown[];
  metadata?: TMetadata;
  __dataParts?: TDataParts;
  __tools?: TTools;
}

export type ChatStatus = 'idle' | 'streaming' | 'error';
