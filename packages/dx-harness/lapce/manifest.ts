/**
 * Pinned Lapce / volt / server / provider version manifest (charter WSP1L
 * deliverable). Item names reuse the WSP0/WSP1 version-capture checklist. A
 * component with no captured reference build stays `unrecorded` with a reason —
 * never a guessed version.
 */

import {
  VERSION_MANIFEST_SCHEMA,
  type LapceVersionManifest,
  type VersionManifestItem,
} from "./types.js";

export const PINNED_LAPCE_VERSION_MANIFEST: LapceVersionManifest = {
  schema: VERSION_MANIFEST_SCHEMA,
  items: [
    {
      item: "lapce-client",
      version: "0.4.6+Nightly.126d356",
      status: "pinned",
      reason:
        "the recorded WSP1L reference build: Lapce v0.4.6 with the WSP1L instrumentation patch " +
        "(packages/dx-harness/lapce/instrumented-client/lapce-0.4.6-wsp1l.patch), sealed in " +
        "tests/workspace-responsiveness/WSP1L/products/real-client-capture.v1.json",
    },
    {
      item: "verter-plugin-adapter",
      version: "0.1.0",
      status: "pinned",
    },
    {
      item: "verter-lsp-server",
      version: "0.0.1-beta.5",
      status: "pinned",
    },
    {
      item: "provider-engines-and-modes",
      version: "tsgo",
      status: "pinned",
    },
  ],
  recordedAs: "tests/workspace-responsiveness/WSP1L/products/version-manifest.v1.json",
};

export interface ManifestDrift {
  readonly ok: boolean;
  readonly drift: readonly string[];
}

/**
 * A run may only be certified against the pinned manifest: every pinned item
 * must match exactly, and every unrecorded item must carry a reason instead of
 * a guessed version.
 */
export function versionsMatchPinnedManifest(manifest: LapceVersionManifest): ManifestDrift {
  const drift: string[] = [];
  const byItem = new Map(manifest.items.map((item) => [item.item, item]));
  for (const pinned of PINNED_LAPCE_VERSION_MANIFEST.items) {
    const actual = byItem.get(pinned.item);
    if (actual === undefined) {
      drift.push(`manifest item '${pinned.item}' missing`);
      continue;
    }
    if (pinned.status === "pinned") {
      if (actual.status !== pinned.status) {
        drift.push(
          `'${pinned.item}' is recorded with status '${actual.status}' but the pinned manifest pins it; a pinned item matches on status and version, never on the version string alone`,
        );
      } else if (actual.version !== pinned.version) {
        drift.push(
          `'${pinned.item}' version '${String(actual.version)}' does not match the pinned '${pinned.version}'`,
        );
      }
    } else if (actual.version !== null) {
      drift.push(
        `'${pinned.item}' is unrecorded in the pinned manifest but carries '${actual.version}'; unavailable components are never given guessed versions`,
      );
    } else if (!actual.reason || actual.reason.length === 0) {
      drift.push(`'${pinned.item}' is unrecorded without a reason`);
    }
  }
  return { ok: drift.length === 0, drift };
}
