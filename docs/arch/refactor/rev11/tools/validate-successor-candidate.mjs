#!/usr/bin/env node
import { loadAuthority, validateAuthority } from "./lib.mjs";
import { validateSuccessorSourcePack } from "./build-successor-source-pack-lock.mjs";

const authority = loadAuthority();
const errors = [
  ...validateSuccessorSourcePack(authority.packageRoot),
  ...validateAuthority(authority, { strict: true, checkGenerated: true, checkAmendments: false }),
];
if (errors.length) {
  for (const error of errors) console.error(`ERROR: ${error}`);
  process.exit(1);
}
console.log(
  `validate-successor-candidate: PASS (${authority.nodes.length} nodes; authority-amendment custody intentionally checked only by the canonical gate)`,
);
