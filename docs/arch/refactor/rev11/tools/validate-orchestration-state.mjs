#!/usr/bin/env node
import path from "node:path";
import { PACKAGE_ROOT, defaultRuntimeRoot, deriveState, loadAuthority, validateAuthority } from "./lib.mjs";

const args = process.argv.slice(2);
const runtimeIndex = args.indexOf("--runtime-root");
const runtimeRoot = runtimeIndex >= 0 ? path.resolve(args[runtimeIndex + 1] || "") : defaultRuntimeRoot(PACKAGE_ROOT);
const authority = loadAuthority();
const staticErrors = validateAuthority(authority, { strict: true, checkGenerated: true });
if (staticErrors.length) {
  console.error(staticErrors.map((error) => `ERROR: ${error}`).join("\n"));
  process.exit(1);
}
const state = deriveState(authority, { runtimeRoot });
if (state.errors.length) {
  console.error(state.errors.map((error) => `ERROR: ${error}`).join("\n"));
  process.exit(1);
}
const counts = {};
for (const row of state.states.values()) counts[row.status] = (counts[row.status] || 0) + 1;
const ready = [...state.states].filter(([, row]) => row.status === "READY").map(([id]) => id);
if (!state.active && ready.some((id) => id !== "ORC0")) {
  console.error(`ERROR: premature activation exposed READY nodes: ${ready.join(",")}`);
  process.exit(1);
}
console.log(`validate-orchestration-state: PASS phase=${state.phase} ${Object.entries(counts).sort().map(([key, value]) => `${key}=${value}`).join(" ")}`);
