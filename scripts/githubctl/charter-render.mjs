import fs from "node:fs";

import { confinedFile, loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { IssueSyncError, SelectionError } from "./errors.mjs";

const BODY_SECTIONS = [
  "Independently acceptable outcome",
  "Source-specific scope",
  "Deletions and forbidden designs",
  "Abort conditions",
];

function extractSection(text, heading) {
  const lines = text.replaceAll("\r\n", "\n").split("\n");
  const start = lines.findIndex((line) => line === `## ${heading}`);
  if (start === -1) throw new IssueSyncError(`charter missing section ${heading}`);
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    if (lines[index].startsWith("## ")) {
      end = index;
      break;
    }
  }
  return lines.slice(start, end).join("\n").trimEnd();
}

export function renderIssueDescription({ nodeId, model, authority = loadAuthority() }) {
  if (typeof model !== "string" || model.length === 0) {
    throw new IssueSyncError("model is required");
  }
  const node = authority.nodes.find((candidate) => candidate.id === nodeId);
  if (!node) throw new SelectionError(`unknown node ${nodeId}`);
  const charterPath = confinedFile(authority.packageRoot, node.charter, `${nodeId} charter`);
  const raw = fs.readFileSync(charterPath, "utf8");
  const text = raw.replace(/^<!-- unified-charter-v2\n[\s\S]*?\n-->\n*/u, "");
  const sections = BODY_SECTIONS.map((heading) => extractSection(text, heading));
  return {
    title: node.name,
    body: `${sections.join("\n\n")}\n\nModel: ${model}\n`,
  };
}
