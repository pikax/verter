import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapterError,
  GitHubDoctor,
  MissingProjectIdentityError,
  ProtectedMappingError,
  projectStatus,
} from "../index.mjs";

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter) {
  const report = new GitHubDoctor(adapter).check({ require: ["projects"] });
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function fixture(options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-project-status-"));
  const ledgerPath = path.join(dir, "implemented.toml");
  fs.writeFileSync(
    ledgerPath,
    `schema = 1

[[implemented]]
node_id = "BASE"
commit_message = "test locator"
commit_date = "2026-08-29T00:00:00+00:00"

[[github_issue]]
node_id = "WORK"
gh_issue = 10
sync_to_github = ${options.protected === true ? "false" : "true"}

[[github_issue]]
node_id = "SIBLING"
gh_issue = 11
sync_to_github = ${options.siblingProtected === true ? "false" : "true"}
${
  options.parentProtected === true
    ? `
[[github_issue]]
node_id = "PARENT"
gh_issue = 20
sync_to_github = false
`
    : ""
}
`,
  );
  const authority = {
    nodes: [
      { id: "BASE", predecessors: [], train: "test" },
      { id: "WORK", predecessors: ["BASE"], train: "test" },
      { id: "SIBLING", predecessors: ["BASE"], train: "test" },
    ],
    ledgerFile: ledgerPath,
  };
  const adapter = fake({
    issues: [
      { number: 10, title: "Work", body: "work", parent: options.parent === false ? null : 20 },
      { number: 11, title: "Sibling", body: "sibling", parent: 20 },
      { number: 20, title: "Parent", body: "parent", subIssues: [10, 11] },
    ],
    projectItems: options.childMissing === true ? [11, 20] : [10, 11, 20],
    projectStatuses: {
      10: options.childStatus ?? "Todo",
      11: options.siblingStatus ?? "Todo",
      20: options.parentStatus ?? "Todo",
    },
  });
  return { adapter, authority, ledgerPath };
}

function run(fx, extra = {}) {
  return projectStatus({
    adapter: fx.adapter,
    authority: fx.authority,
    ledgerPath: fx.ledgerPath,
    node: "WORK",
    ...extra,
  });
}

test("starting a child marks both the child and its parent in progress", () => {
  const fx = fixture();
  const report = run(fx, {
    mode: "apply",
    status: "in-progress",
    clearance: clearanceFor(fx.adapter),
  });

  assert.equal(report.item.status, "In Progress");
  assert.equal(report.parent.status, "In Progress");
  assert.equal(fx.adapter.getProjectStatus(10), "In Progress");
  assert.equal(fx.adapter.getProjectStatus(20), "In Progress");
});

test("starting a child does not require every mapped sibling to be attached yet", () => {
  const fx = fixture();
  const getState = fx.adapter.getIssueProjectState.bind(fx.adapter);
  fx.adapter.getIssueProjectState = (number) => {
    const snapshot = getState(number);
    if (number === 20) snapshot.subIssues = snapshot.subIssues.filter((row) => row.number === 10);
    return snapshot;
  };
  const report = run(fx, { mode: "check", status: "in-progress" });
  assert.equal(report.item.status, "In Progress");
  assert.equal(report.parent.status, "In Progress");
});

test("finishing still requires every mapped sibling to be attached", () => {
  const fx = fixture({ siblingStatus: "Done" });
  const getState = fx.adapter.getIssueProjectState.bind(fx.adapter);
  fx.adapter.getIssueProjectState = (number) => {
    const snapshot = getState(number);
    if (number === 20) snapshot.subIssues = snapshot.subIssues.filter((row) => row.number === 10);
    return snapshot;
  };
  assert.throws(() => run(fx, { mode: "check", status: "done" }), GitHubAdapterError);
});

test("finishing a child keeps its parent in progress while a sibling remains", () => {
  const fx = fixture({ siblingStatus: "In Progress", parentStatus: "In Progress" });
  const report = run(fx, {
    mode: "apply",
    status: "done",
    clearance: clearanceFor(fx.adapter),
  });

  assert.equal(report.item.status, "Done");
  assert.equal(report.parent.status, "In Progress");
  assert.equal(fx.adapter.getProjectStatus(10), "Done");
  assert.equal(fx.adapter.getProjectStatus(20), "In Progress");
});

test("finishing the last child marks its parent done", () => {
  const fx = fixture({ siblingStatus: "Done", parentStatus: "In Progress" });
  const report = run(fx, {
    mode: "apply",
    status: "done",
    clearance: clearanceFor(fx.adapter),
  });

  assert.equal(report.parent.status, "Done");
  assert.equal(fx.adapter.getProjectStatus(20), "Done");
});

test("check mode plans status changes without writing the project", () => {
  const fx = fixture();
  const report = run(fx, { mode: "check", status: "in-progress" });

  assert.equal(report.item.status, "In Progress");
  assert.equal(report.parent.status, "In Progress");
  assert.equal(fx.adapter.getProjectStatus(10), "Todo");
  assert.equal(fx.adapter.getProjectStatus(20), "Todo");
});

test("apply requires doctor clearance and refuses protected issue mappings", () => {
  const fx = fixture();
  assert.throws(() => run(fx, { mode: "apply", status: "in-progress" }), DoctorRequiredError);

  const protectedFx = fixture({ protected: true });
  assert.throws(
    () =>
      run(protectedFx, {
        mode: "apply",
        status: "in-progress",
        clearance: clearanceFor(protectedFx.adapter),
      }),
    ProtectedMappingError,
  );
});

test("protected train mappings suppress parent Project traffic", () => {
  for (const options of [{ siblingProtected: true }, { parentProtected: true }]) {
    const fx = fixture(options);
    const report = run(fx, {
      mode: "apply",
      status: "in-progress",
      clearance: clearanceFor(fx.adapter),
    });
    assert.equal(report.item.status, "In Progress");
    assert.equal(report.parent, null);
    assert.equal(report.parent_skipped.reason, "protected-mapping");
    assert.equal(
      fx.adapter.reads.some((row) => row.kind === "get-issue-project-state" && row.number === 20),
      false,
    );
    assert.equal(fx.adapter.getProjectStatus(20), "Todo");
  }
});

test("Project lifecycle refuses an issue that scheduling has not added", () => {
  const fx = fixture({ childMissing: true });
  assert.throws(
    () => run(fx, { mode: "check", status: "in-progress" }),
    MissingProjectIdentityError,
  );
});

test("Project lifecycle refuses a parent from another repository", () => {
  const fx = fixture();
  const getState = fx.adapter.getIssueProjectState.bind(fx.adapter);
  fx.adapter.getIssueProjectState = (number) => {
    const snapshot = getState(number);
    if (number === 10) {
      snapshot.parent = { id: "external-parent", number: 20, owner: "other", repo: "repo" };
    }
    return snapshot;
  };
  assert.throws(() => run(fx, { mode: "check", status: "in-progress" }), GitHubAdapterError);
});

test("Project lifecycle compares repository identities case-insensitively", () => {
  const fx = fixture();
  const getState = fx.adapter.getIssueProjectState.bind(fx.adapter);
  fx.adapter.getIssueProjectState = (number) => {
    const snapshot = getState(number);
    if (snapshot.parent) {
      snapshot.parent.owner = snapshot.parent.owner.toUpperCase();
      snapshot.parent.repo = snapshot.parent.repo.toUpperCase();
    }
    for (const child of snapshot.subIssues) {
      child.owner = child.owner.toUpperCase();
      child.repo = child.repo.toUpperCase();
    }
    return snapshot;
  };
  const report = run(fx, { mode: "check", status: "in-progress" });
  assert.equal(report.parent.status, "In Progress");
});
