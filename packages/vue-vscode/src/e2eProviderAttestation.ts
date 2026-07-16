export type E2eTypeProviderRoute = "tsserver" | "tsgo" | "shared-tsgo";

export interface E2eTypeProviderAttestation {
  publicKind: "tsgo" | "tsserver" | "editor-tsserver";
  reason?: string;
  route: "tsserver" | "managed-tsgo" | "shared-tsgo";
}

const STATUS_PATTERN =
  /Type provider status:\s+(tsgo|tsserver|editor-tsserver|none)(?: \((.+?)\))?/g;
const SHARED_ARMED_PATTERN = /\[shared-tsgo\] armed:[^\n]*\bcontrolDir=/;

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

  if (requested === "tsserver") {
    if (publicKind !== "tsserver" && publicKind !== "editor-tsserver") {
      throw new Error(`Requested tsserver, but the public provider status reported ${publicKind}`);
    }
    return { publicKind, reason, route: "tsserver" };
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
