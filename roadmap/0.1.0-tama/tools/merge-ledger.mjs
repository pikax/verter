#!/usr/bin/env node
// Deterministic semantic merge for the trusted implementation ledger.
//
// Usage:
//   node merge-ledger.mjs <base> <ours> <theirs>            # merged text to stdout
//   node merge-ledger.mjs <base> <ours> <theirs> --driver   # write result into <ours> (git merge driver: %O %A %B --driver)
//
// Exit codes: 0 merged; 1 semantic conflict (fail closed); 2 usage.
//
// Register as a git merge driver:
//   git config merge.tama-ledger.name "Tama implementation ledger merge"
//   git config merge.tama-ledger.driver "node roadmap/0.1.0-tama/tools/merge-ledger.mjs %O %A %B --driver"
// with .gitattributes:
//   roadmap/0.1.0-tama/authority/state/implemented.toml merge=tama-ledger

import fs from "node:fs";
import { mergeLedgerTexts } from "./ledger.mjs";

const args = process.argv.slice(2);
const driver = args.includes("--driver");
const files = args.filter((arg) => arg !== "--driver");
if (files.length !== 3) {
  console.error("usage: merge-ledger.mjs <base> <ours> <theirs> [--driver]");
  process.exit(2);
}

const [baseFile, oursFile, theirsFile] = files;
const result = mergeLedgerTexts({
  base: fs.readFileSync(baseFile, "utf8"),
  ours: fs.readFileSync(oursFile, "utf8"),
  theirs: fs.readFileSync(theirsFile, "utf8"),
});

if (!result.ok) {
  console.error("merge-ledger: CONFLICT (fail closed, no guessing):");
  for (const conflict of result.conflicts) console.error(`- ${conflict}`);
  process.exit(1);
}

if (driver) {
  fs.writeFileSync(oursFile, result.text);
  console.error("merge-ledger: merged deterministically");
} else {
  process.stdout.write(result.text);
}
