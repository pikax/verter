export type E2eTypeProviderRoute = "tsserver" | "tsgo" | "shared-tsgo" | "editor-tsserver";

export interface E2eTypeProviderAttestation {
  publicKind: "tsgo" | "tsserver" | "editor-tsserver";
  reason?: string;
  route: "tsserver" | "managed-tsgo" | "shared-tsgo" | "editor-tsserver";
}

const STATUS_PATTERN =
  /Type provider status:\s+(tsgo|tsserver|editor-tsserver|none)(?: \((.+?)\))?/g;
const SHARED_ARMED_PATTERN = /\[shared-tsgo\] armed:[^\n]*\bcontrolDir=/;
const SHARED_SERVED_PATTERN =
  /editor-owned tsgo served carrier (?:feature|diagnostics); managed fallback remained cold/;
const SHARED_FALLBACK_PATTERN =
  /editor-owned tsgo .*?(?:did not engage|timed out); activating managed fallback|managed (?:TSGO|tsgo) provider .*started with PID/i;

/**
 * Prove that a shared-tsgo E2E route served at least one carrier request through
 * the editor-owned Program and never admitted the managed fallback.
 */
export function assertSharedTsgoServedWithoutFallback(log: string): void {
  if (SHARED_FALLBACK_PATTERN.test(log)) {
    throw new Error("Requested shared-tsgo, but the managed fallback was activated");
  }
  if (!SHARED_SERVED_PATTERN.test(log)) {
    throw new Error(
      "Requested shared-tsgo, but no carrier feature was served by the editor-owned Program",
    );
  }
}

/**
 * Validate the public provider identity before an E2E feature assertion can run.
 * A sync notification alone is insufficient: provider-less initialization also
 * completes its scanner lifecycle and previously allowed a requested tsgo run to
 * continue with only Verter-native suggestions.
 */
export function attestE2eTypeProviderLog(
  log: string,
  requested: E2eTypeProviderRoute,
): E2eTypeProviderAttestation {
  const statuses = Array.from(log.matchAll(STATUS_PATTERN));
  const last = statuses[statuses.length - 1];
  if (!last) {
    throw new Error(`Requested ${requested}, but no public Type provider status was reported`);
  }

  const publicKind = last[1];
  const reason = last[2];
  if (publicKind === "none") {
    const label = requested === "tsgo" ? "managed tsgo" : requested;
    throw new Error(
      `Requested ${label}, but the public provider status reported none` +
        (reason ? ` (${reason})` : ""),
    );
  }

  // The workspace tsserver and the editor-owned plugin tier are DISTINCT engines
  // with distinct topologies, so each rail is held to the one it asked for.
  // Accepting the editor plugin for a `tsserver` run is what let a tier that
  // served nothing pass as "the workspace tsserver".
  if (requested === "tsserver" || requested === "editor-tsserver") {
    if (publicKind !== requested) {
      throw new Error(
        `Requested ${requested}, but the public provider status reported ${publicKind}`,
      );
    }
    return { publicKind, reason, route: requested };
  }

  if (publicKind !== "tsgo") {
    throw new Error(
      `Requested ${requested}, but the public provider status reported ${publicKind}`,
    );
  }

  const sharedArmed = SHARED_ARMED_PATTERN.test(log);
  if (requested === "tsgo") {
    if (sharedArmed) {
      throw new Error("Requested managed tsgo, but the shared route was armed");
    }
    return { publicKind, reason, route: "managed-tsgo" };
  }

  if (!reason || !/editor-owned Native Preview/i.test(reason)) {
    throw new Error(
      "Requested shared-tsgo, but public status is missing editor-owned Native Preview provenance",
    );
  }
  if (!sharedArmed) {
    throw new Error("Requested shared-tsgo, but no editor rendezvous was armed");
  }
  return { publicKind, reason, route: "shared-tsgo" };
}
