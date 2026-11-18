/** Work counters of one session. Counters only; latency is measured elsewhere. */
export interface PipelineCounters {
  hookInvocations: number;
  /** `host.upsert` calls for carriers. */
  registrations: number;
  /** `host.compileRequest` calls. */
  compileRequests: number;
  /** Dependency files read for type hydration. */
  hydrationReads: number;
  /** `host.setImportDependencies` calls. */
  routePushes: number;
  /** Bytes of externally processed content handed to the host. */
  preparedInputBytes: number;
  /** Source maps rewritten by this package (SSR rewrites). */
  mapCompositions: number;
  /** Flights replaced by a newer source revision. */
  supersededFlights: number;
  /** Flights that finished under an older epoch and published nothing. */
  staleDiscards: number;
  /** Compile repeats made after supplying inputs the host named. */
  inputRounds: number;
}

export function createPipelineCounters(): PipelineCounters {
  return {
    hookInvocations: 0,
    registrations: 0,
    compileRequests: 0,
    hydrationReads: 0,
    routePushes: 0,
    preparedInputBytes: 0,
    mapCompositions: 0,
    supersededFlights: 0,
    staleDiscards: 0,
    inputRounds: 0,
  };
}
