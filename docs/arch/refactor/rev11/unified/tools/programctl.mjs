#!/usr/bin/env node
import path from "node:path";
import {
  PACKAGE_ROOT, admitNode, assertDispatchable, createAmendment, createDirectiveAuthorization, defaultRuntimeRoot,
  activateProgram, deriveState, dispatchNode, explainNode, finalizeCandidate, importAcceptanceReceipt, importRuntimeArtifact,
  loadAuthority, packetFor, releaseLease, renewLease, runGateEvidence, runReviewEvidence,
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
    } else if (command === "review-run") {
      const result = runReviewEvidence(authority, {
        runtimeRoot, id: positional(1, "node ID"), lens: value("--lens", { required: true }), holder: value("--holder", { required: true }),
        leaseId: value("--lease-id", { required: true }), custodyBinding: value("--custody-binding", { required: true }),
      });
      console.log(JSON.stringify({ evidence_id: result.evidence.evidence_id, digest: result.evidence.digest, lens: result.evidence.lens }, null, 2));
    } else if (command === "activate") {
      const result = activateProgram(authority, {
        runtimeRoot, orc0Receipt: value("--orc0-receipt", { required: true }),
        authorization: value("--authorization", { required: true }), activatedBy: value("--activated-by", { required: true }),
      });
      console.log(JSON.stringify({ phase: result.state.phase, transition: `${result.transition.transition_id}:${result.transition.digest}`, authority_sha256: result.state.authorityDigest }, null, 2));
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
      const result = admitNode(authority, {
        runtimeRoot, id: positional(1, "node ID"), holder: value("--holder", { required: true }),
        candidateRef: value("--candidate-ref", { required: true }), gateRunner: value("--gate-runner", { required: true }),
        reviewers: values("--reviewer"), ttlSeconds: Number(value("--ttl-seconds", { fallback: "3600" })),
      });
      process.stdout.write(result.packet);
    } else if (command === "dispatch") {
      const result = dispatchNode(authority, { runtimeRoot, id: positional(1, "node ID"), holder: value("--holder", { required: true }), leaseId: value("--lease-id", { required: true }) });
      process.stdout.write(result.packet);
      process.stderr.write(`dispatch_receipt=${result.dispatch.dispatch_id}:${result.dispatch.digest}\n`);
    } else if (command === "candidate-finalize") {
      const result = finalizeCandidate(authority, { runtimeRoot, id: positional(1, "node ID"), holder: value("--holder", { required: true }), leaseId: value("--lease-id", { required: true }) });
      console.log(JSON.stringify({ node_id: result.finalization.node_id, finalization: `${result.finalization.finalization_id}:${result.finalization.digest}`, candidate_sha: result.finalization.candidate_sha, candidate_tree: result.finalization.candidate_tree, changed_paths: result.finalization.changed_paths }, null, 2));
    }
    else if (command === "lease-renew") {
      const result = renewLease(authority, { runtimeRoot, leaseId: positional(1, "lease ID"), holder: value("--holder", { required: true }), ttlSeconds: Number(value("--ttl-seconds", { fallback: "3600" })) });
      console.log(JSON.stringify({ lease_id: result.lease.lease_id, receipt: `${result.lease.lease_id}:${result.lease.digest}`, expires_at: result.lease.expires_at }, null, 2));
    } else if (command === "lease-release") {
      const result = releaseLease(authority, { runtimeRoot, leaseId: positional(1, "lease ID"), holder: value("--holder", { required: true }) });
      console.log(JSON.stringify({ release_id: result.release.release_id, digest: result.release.digest }, null, 2));
    } else throw new Error(`unknown command ${command}`);
  }
} catch (error) {
  console.error(`ERROR: ${error.message}`);
  process.exitCode = 1;
}
