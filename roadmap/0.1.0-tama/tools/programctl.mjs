#!/usr/bin/env node
import { deriveState, explainNode, loadAuthority, packetFor, validateAuthority } from "./lib.mjs";

const args = process.argv.slice(2);
const command = args[0] || "frontier";
const authority = loadAuthority();

function positional(index, label) {
  const value = args[index];
  if (!value || value.startsWith("--")) throw new Error(`${label} is required`);
  return value;
}

try {
  const errors = validateAuthority(authority);
  if (errors.length)
    throw new Error(`authority invalid:\n${errors.map((error) => `- ${error}`).join("\n")}`);
  const state = deriveState(authority);

  if (command === "frontier") {
    const ready = [...state.states]
      .filter(([, row]) => row.status === "READY")
      .map(([nodeId]) => nodeId)
      .sort();
    console.log(`ready=${ready.length}`);
    for (const nodeId of ready) console.log(nodeId);
  } else if (command === "explain") {
    console.log(JSON.stringify(explainNode(authority, state, positional(1, "node ID")), null, 2));
  } else if (command === "packet") {
    process.stdout.write(packetFor(authority, state, positional(1, "node ID")));
  } else if (command === "implemented") {
    const rows = authority.ledger.implemented
      .map((row) => ({
        node_id: row.node_id,
        commit_message: row.commit_message,
        commit_date: row.commit_date,
        ...(row.pull_request === undefined ? {} : { pull_request: row.pull_request }),
      }))
      .sort((left, right) => left.node_id.localeCompare(right.node_id));
    console.log(JSON.stringify(rows, null, 2));
  } else if (command === "github-issues") {
    const rows = [...(authority.ledger.github_issue || [])].sort((left, right) =>
      left.node_id.localeCompare(right.node_id),
    );
    console.log(JSON.stringify(rows, null, 2));
  } else if (command === "github-issue") {
    const issue = Number(positional(1, "GitHub issue number"));
    if (!Number.isSafeInteger(issue) || issue < 1)
      throw new Error("GitHub issue number must be positive");
    const row = (authority.ledger.github_issue || []).find(
      (candidate) => candidate.gh_issue === issue,
    );
    if (!row) throw new Error(`GitHub issue #${issue} is not mapped`);
    console.log(JSON.stringify(row, null, 2));
  } else {
    throw new Error(
      `unknown command ${command}; supported commands: frontier, explain, packet, implemented, github-issues, github-issue`,
    );
  }
} catch (error) {
  console.error(`ERROR: ${error.message}`);
  process.exitCode = 1;
}
