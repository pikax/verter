// Reached via the `@/*` path alias from the off-disk carrier.
export interface FormatOptions {
  upper: boolean;
}

export function formatLabel(input: string, opts: FormatOptions): string {
  return opts.upper ? input.toUpperCase() : input.toLowerCase();
}
