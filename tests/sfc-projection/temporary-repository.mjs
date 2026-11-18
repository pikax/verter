import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// Mutation proofs own writable source bytes. The checkout may be a read-only
// build view, and concurrently running proofs must never see another plant.
function copyRepository(source, destination) {
  fs.mkdirSync(destination, { recursive: true });
  for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
    if ([".git", ".gin", ".feedback", "target"].includes(entry.name)) continue;
    const from = path.join(source, entry.name);
    const to = path.join(destination, entry.name);
    if (entry.name === "node_modules") {
      fs.symlinkSync(from, to, "junction");
    } else if (entry.isDirectory()) copyRepository(from, to);
    else if (entry.isFile()) {
      fs.copyFileSync(from, to);
      fs.chmodSync(to, fs.statSync(from).mode | 0o200);
    }
  }
}

export function withTemporaryRepository(repoRoot, run) {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "verter-mutation-"));
  try {
    copyRepository(repoRoot, temporary);
    const result = run(temporary);
    if (result && typeof result.then === "function") {
      return result.finally(() => fs.rmSync(temporary, { recursive: true, force: true }));
    }
    fs.rmSync(temporary, { recursive: true, force: true });
    return result;
  } catch (error) {
    fs.rmSync(temporary, { recursive: true, force: true });
    throw error;
  }
}
