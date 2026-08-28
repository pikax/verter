#!/usr/bin/env node
import path from "node:path";
import {
  PACKAGE_ROOT, admitNode, assertDispatchable, createAmendment, createDirectiveAuthorization, defaultRuntimeRoot,
  activateProgram, activateTrustedProgram, deriveState, dispatchNode, explainNode, finalizeCandidate, importAcceptanceReceipt, importRuntimeArtifact,
  loadAuthority, packetFor, releaseLease, renewLease, runGateEvidence, runReviewEvidence,
  acceptTrustedRound, admitTrustedNode, closeTrustedRound, dispatchTrustedNode, finalizeTrustedCandidate,
  recordTrustedArchitectDecision, recordTrustedLanding, recordTrustedReview, recordTrustedReviewCleanup, recordTrustedRole, reinitializeTrustedLocal, renewTrustedLease,
  validateAmendments, validateAuthority,
} from "./lib.mjs";

const args = process.argv.slice(2);
const command = args[0] || "frontier";

function values(name) {
  const result = [];
  for (let index = 1; index < args.length; index += 1) {
    if (args[index] === name) {
      if (!args[index + 1] || args[index + 1].startsWith("--")) throw new Error(`${name} requires a value`);
      result.push(args[index + 1]); index += 1;
    } else if (args[index].startsWith(`${name}=`)) result.push(args[index].slice(name.length + 1));
  }
  return result;
}

function value(name, { required = false, fallback } = {}) {
  const found = values(name);
  if (found.length > 1) throw new Error(`${name} may be supplied only once`);
  if (required && !found.length) throw new Error(`${name} is required`);
  return found[0] ?? fallback;
}

function positional(index, label) {
  const result = args[index];
  if (!result || result.startsWith("--")) throw new Error(`${label} is required`);
  return result;
}

const authority = loadAuthority();
const runtimeRoot = path.resolve(value("--runtime-root", { fallback: defaultRuntimeRoot(PACKAGE_ROOT) }));
const openedOptional = values("--opened");

function effortOverrides() {
  return Object.fromEntries(values("--effort").map((entry) => {
    const split = entry.indexOf("=");
    if (split < 1) throw new Error("--effort requires role=low|medium|high");
    return [entry.slice(0, split), entry.slice(split + 1)];
  }));
}

function assertStatic({ amendments = true } = {}) {
  const errors = validateAuthority(authority, { strict: true, checkGenerated: true, checkAmendments: amendments, runtimeRoot });
  if (errors.length) throw new Error(`static authority invalid:\n${errors.map((error) => `- ${error}`).join("\n")}`);
}

function state() {
  const result = deriveState(authority, { runtimeRoot, openedOptional });
  if (result.errors.length) throw new Error(`orchestration state invalid:\n${result.errors.map((error) => `- ${error}`).join("\n")}`);
  return result;
}

try {
  if (command === "amendment-create") {
    assertStatic({ amendments: false });
    const result = createAmendment(authority, {
      id: positional(1, "amendment ID"), beforeRoot: path.resolve(value("--before-root", { required: true })),
      ratifiedBy: value("--ratified-by", { required: true }), ratificationReceiptSha256: value("--ratification-receipt", { required: true }),
      runtimeRoot,
    });
    console.log(JSON.stringify({ amendment: result.amendment.amendment_id, digest: result.amendmentSha256, lock: result.lock }, null, 2));
  } else {
    assertStatic();
    if (command === "amendment-check") {
      const errors = validateAmendments(authority, { runtimeRoot });
      if (errors.length) throw new Error(errors.join("\n"));
      console.log("amendment-check: PASS");
    } else if (command === "receipt-import") {
      const result = importAcceptanceReceipt(authority, { runtimeRoot, file: path.resolve(positional(1, "receipt file")) });
      console.log(JSON.stringify({ node_id: result.artifact.node_id, receipt: `${result.artifact.node_id}:${result.artifact.digest}`, phase: result.state.phase }, null, 2));
    } else if (command === "landed-receipt-import") {
      const result = importRuntimeArtifact(authority, { runtimeRoot, kind: "landed", file: path.resolve(positional(1, "landed receipt file")) });
      console.log(JSON.stringify({ node_id: result.artifact.node_id, receipt: `${result.artifact.receipt_id}:${result.artifact.digest}`, state: result.artifact.state, phase: result.state.phase }, null, 2));
    } else if (command === "authorization-create") {
      const result = createDirectiveAuthorization(authority, { runtimeRoot, id: positional(1, "node ID"), holder: value("--holder", { required: true }), leaseId: value("--lease-id", { required: true }) });
      console.log(JSON.stringify({ node_id: result.artifact.node_id, authorization: `${result.artifact.authorization}:${result.artifact.digest}`, candidate_sha: result.artifact.candidate_sha, candidate_tree: result.artifact.candidate_tree }, null, 2));
    } else if (command === "authorization-import") {
      const result = importRuntimeArtifact(authority, { runtimeRoot, kind: "authorization", file: path.resolve(positional(1, "authorization file")) });
      console.log(JSON.stringify({ node_id: result.artifact.node_id, authorization: `${result.artifact.authorization}:${result.artifact.digest}` }, null, 2));
    } else if (command === "evidence-import") {
      const kind = positional(1, "evidence kind");
      if (!['gate', 'review'].includes(kind)) throw new Error("evidence kind must be gate or review");
      const result = importRuntimeArtifact(authority, { runtimeRoot, kind, file: path.resolve(positional(2, "evidence file")) });
      console.log(JSON.stringify({ evidence_id: result.artifact.evidence_id, digest: result.artifact.digest }, null, 2));
    } else if (command === "gate-run") {
      const result = runGateEvidence(authority, {
        runtimeRoot, id: positional(1, "node ID"), scope: value("--scope", { required: true }), holder: value("--holder", { required: true }),
        leaseId: value("--lease-id", { required: true }), integrationSha: value("--integration-sha", { required: true }),
      });
      console.log(JSON.stringify({ evidence_id: result.evidence.evidence_id, digest: result.evidence.digest, scope: result.evidence.scope }, null, 2));
    } else if (command === "harness-record") {
      const role = value("--role", { required: true });
      const common = { runtimeRoot, roundId: value("--round-id", { required: true }), leaseId: value("--lease-id", { required: true }), holder: value("--holder", { required: true }), taskIdentity: value("--task", { required: true }), agentIdentity: value("--agent", { required: true }), provider: value("--provider", { required: true }), model: value("--model", { required: true }), effort: value("--effort", { required: true }), promptFile: path.resolve(value("--prompt", { required: true })), reportFile: path.resolve(value("--report", { required: true })) };
      const result = role === "review" ? recordTrustedReview(authority, { ...common, lens: value("--lens", { required: true }), worktreeMode: value("--worktree-mode", { fallback: "read-only" }), disposableWorktree: value("--disposable-worktree", { fallback: "" }) }) : recordTrustedRole(authority, { ...common, role });
      console.log(JSON.stringify(result, null, 2));
    } else if (command === "review-cleanup-record") {
      console.log(JSON.stringify(recordTrustedReviewCleanup(authority, { runtimeRoot, evidenceId: value("--evidence-id", { required: true }), holder: value("--holder", { required: true }), worktree: path.resolve(value("--worktree", { required: true })) }), null, 2));
    } else if (command === "trusted-local-reinitialize") {
      console.log(JSON.stringify(reinitializeTrustedLocal(authority, { operator: value("--operator", { required: true }), reason: value("--reason", { required: true }) }), null, 2));
    } else if (command === "architect-decision-record") {
      console.log(JSON.stringify(recordTrustedArchitectDecision(authority, { runtimeRoot, roundId: value("--round-id", { required: true }), operator: value("--operator", { required: true }), reportFile: path.resolve(value("--report", { required: true })) }), null, 2));
    } else if (command === "activate") {
      const result = activateTrustedProgram(authority, { runtimeRoot, activatedBy: value("--activated-by", { required: true }) });
      console.log(JSON.stringify({ phase: result.state.phase, transition: `${result.transition.transition_id}:${result.transition.digest}`, authority_sha256: result.state.authorityDigest }, null, 2));
    } else if (command === "landing-record") {
      console.log(JSON.stringify(recordTrustedLanding(authority, { runtimeRoot, roundId: value("--round-id", { required: true }), holder: value("--holder", { required: true }) }), null, 2));
    } else if (command === "phase") {
      const current = state();
      console.log(`phase=${current.phase} authority_sha256=${current.authorityDigest} runtime=${current.runtimeRoot}`);
    } else if (command === "frontier") {
      const current = state();
      const ready = [...current.states].filter(([, row]) => row.status === "READY").map(([nodeId]) => nodeId).sort();
      console.log(`phase=${current.phase} ready=${ready.length}`);
      for (const nodeId of ready) console.log(nodeId);
    } else if (command === "explain") console.log(JSON.stringify(explainNode(authority, state(), positional(1, "node ID")), null, 2));
    else if (command === "packet") process.stdout.write(packetFor(authority, state(), positional(1, "node ID"), { holder: value("--holder", { required: true }), leaseId: value("--lease-id", { required: true }) }));
    else if (command === "admit") {
      const result = admitTrustedNode(authority, {
        runtimeRoot, id: positional(1, "node ID"), holder: value("--holder", { required: true }),
        candidateRef: value("--candidate-ref", { required: true }), effortOverrides: effortOverrides(),
      });
      console.log(JSON.stringify({ lease_id: result.lease.lease_id, round_id: result.lease.round_id, packet_path: result.packetFile, brief_paths: result.briefPaths, effort_policy: result.lease.effort_policy }, null, 2));
    } else if (command === "dispatch") {
      const result = dispatchTrustedNode(authority, { runtimeRoot, id: positional(1, "node ID"), holder: value("--holder", { required: true }), leaseId: value("--lease-id", { required: true }) });
      process.stdout.write(result.packet);
    } else if (command === "candidate-finalize") {
      const result = finalizeTrustedCandidate(authority, { runtimeRoot, holder: value("--holder", { required: true }), leaseId: value("--lease-id", { required: true }) });
      console.log(JSON.stringify(result, null, 2));
    }
    else if (command === "lease-renew") {
      const result = renewTrustedLease(authority, { runtimeRoot, leaseId: positional(1, "lease ID"), holder: value("--holder", { required: true }) });
      console.log(JSON.stringify(result, null, 2));
    } else if (command === "lease-release") {
      console.log(JSON.stringify(closeTrustedRound(authority, { runtimeRoot, leaseId: positional(1, "lease ID"), holder: value("--holder", { required: true }), outcome: value("--outcome", { fallback: "RELEASED" }) }), null, 2));
    } else if (command === "round-accept") {
      console.log(JSON.stringify(acceptTrustedRound(authority, { runtimeRoot, roundId: positional(1, "round ID"), holder: value("--holder", { required: true }) }), null, 2));
    } else throw new Error(`unknown command ${command}`);
  }
} catch (error) {
  console.error(`ERROR: ${error.message}`);
  process.exitCode = 1;
}
