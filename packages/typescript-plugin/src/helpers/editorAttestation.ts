import fs from "node:fs";
import path from "node:path";

import {
  EDITOR_TSSERVER_ATTESTATION_VERSION,
  editorTsserverAttestationFileName,
  parseEditorTsserverAttestationRequest,
} from "@verter/language-shared";

/**
 * Write a project-bound activation receipt from inside the tsserver plugin.
 * Invalid/unbound requests fail closed and filesystem failures never crash tsserver.
 */
export function writeEditorTsserverAttestation(
  config: Record<string, unknown> | undefined,
  projects: Iterable<string>,
  pid: number = process.pid,
): string | undefined {
  const request = parseEditorTsserverAttestationRequest(config);
  const boundProjects = [...new Set(projects)].filter(Boolean).sort();
  if (!request || boundProjects.length === 0 || !Number.isSafeInteger(pid) || pid <= 0) {
    return undefined;
  }

  const receiptPath = path.join(
    request.directory,
    editorTsserverAttestationFileName(request.nonce),
  );
  const temporaryPath = `${receiptPath}.tmp-${pid}`;
  try {
    fs.mkdirSync(request.directory, { recursive: true });
    fs.writeFileSync(
      temporaryPath,
      JSON.stringify({
        version: EDITOR_TSSERVER_ATTESTATION_VERSION,
        nonce: request.nonce,
        pid,
        projects: boundProjects,
      }),
      { encoding: "utf8", mode: 0o600 },
    );
    fs.renameSync(temporaryPath, receiptPath);
    return receiptPath;
  } catch {
    try {
      fs.rmSync(temporaryPath, { force: true });
    } catch {
      // Best-effort cleanup; attestation remains absent and therefore fails closed.
    }
    return undefined;
  }
}
