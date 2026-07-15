// A global ambient type provided ONLY via tsconfig `types` + `typeRoots`.
// If the carrier resolves identically to an on-disk file, `VerterGlobalMarker`
// is in scope WITHOUT any import. If `types`/`typeRoots` are NOT applied
// (inferred/config-less project), referencing it yields TS2304 (cannot find name).
declare global {
  interface VerterGlobalMarker {
    readonly kind: "verter-global";
  }
  const VERTER_GLOBAL_FLAG: VerterGlobalMarker;
}

export {};
