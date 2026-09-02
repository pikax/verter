/**
 * @ai-generated - This test file was generated with AI assistance.
 * Branch-protection ruleset check/apply: pure diffs, fake adapter
 * mutation recording, and injected GitHubAdapter REST path shapes.
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubAdapterError,
  GitHubDoctor,
  UnstructuredGitHubOutputError,
  diffProtection,
  loadExpectedProtection,
  protectionApply,
  protectionCheck,
} from "../index.mjs";

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter) {
  const report = new GitHubDoctor(adapter).check({ require: ["admin"] });
  assert.equal(report.ok, true, report.errors?.join("; ") ?? "doctor failed");
  return report.clearance;
}

function expectedProtection() {
  return loadExpectedProtection();
}

function matchingActual(overrides = {}) {
  const expected = expectedProtection();
  const ruleset = {
    id: 1,
    ...expected.ruleset,
    ...(overrides.ruleset ?? {}),
    rules: overrides.rules ?? expected.ruleset.rules,
  };
  return {
    rulesets: [ruleset, ...(overrides.otherRulesets ?? [])],
    repository: { ...expected.repositorySettings, ...(overrides.repository ?? {}) },
  };
}

function findingKinds(report) {
  return report.findings.map((row) => row.kind);
}

function transportMap(routes) {
  const calls = [];
  return {
    calls,
    request(req) {
      calls.push(req);
      const key = `${req.method} ${req.path}`;
      if (!Object.hasOwn(routes, key)) throw new Error(`unexpected ${key}`);
      const hit = routes[key];
      if (typeof hit === "function") return hit(req);
      if (hit instanceof Error) throw hit;
      return hit;
    },
  };
}

test("loadExpectedProtection reads the versioned data file", () => {
  const expected = expectedProtection();
  assert.equal(expected.ruleset.name, "tama-main-protection");
  assert.equal(expected.ruleset.target, "branch");
  assert.equal(expected.ruleset.enforcement, "active");
  assert.equal(expected.repositorySettings.allow_squash_merge, true);
  assert.equal(expected.repositorySettings.allow_merge_commit, false);
});

test("loadExpectedProtection throws typed errors on garbage", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-protection-"));
  const invalidJson = path.join(dir, "invalid.json");
  fs.writeFileSync(invalidJson, "{");
  assert.throws(() => loadExpectedProtection(invalidJson), UnstructuredGitHubOutputError);

  const missingCheck = path.join(dir, "missing-check.json");
  fs.writeFileSync(
    missingCheck,
    JSON.stringify({
      ruleset: {
        name: "x",
        target: "branch",
        enforcement: "active",
        rules: [{ type: "deletion" }],
      },
      repositorySettings: {},
    }),
  );
  assert.throws(() => loadExpectedProtection(missingCheck), GitHubAdapterError);
});

test("matching protection state is ok", () => {
  const expected = expectedProtection();
  const actual = matchingActual({
    rules: expected.ruleset.rules.map((rule) => {
      if (rule.type === "pull_request") {
        return {
          ...rule,
          parameters: { ...rule.parameters, github_added_field: true },
        };
      }
      if (rule.type === "required_status_checks") {
        return {
          ...rule,
          parameters: {
            ...rule.parameters,
            required_status_checks: [{ context: "CI Required", integration_id: 99 }],
          },
        };
      }
      return rule;
    }),
  });
  const report = diffProtection(expected, actual);
  assert.equal(report.ok, true);
  assert.deepEqual(report.findings, []);
});

test("missing ruleset is reported", () => {
  const expected = expectedProtection();
  const report = diffProtection(expected, {
    rulesets: [],
    repository: expected.repositorySettings,
  });
  assert.equal(report.ok, false);
  assert.equal(report.findings.length, 1);
  assert.equal(report.findings[0].kind, "missing-ruleset");
  assert.equal(report.findings[0].expected, expected.ruleset.name);
  assert.equal(report.findings[0].action, "create");
});

test("wrong enforcement is reported", () => {
  const expected = expectedProtection();
  const report = diffProtection(expected, matchingActual({ ruleset: { enforcement: "disabled" } }));
  assert.equal(report.ok, false);
  assert.equal(findingKinds(report).includes("wrong-enforcement"), true);
  const row = report.findings.find((finding) => finding.kind === "wrong-enforcement");
  assert.equal(row.expected, "active");
  assert.equal(row.actual, "disabled");
  assert.equal(row.action, "update");
});

test("missing required check context is reported", () => {
  const expected = expectedProtection();
  const rules = expected.ruleset.rules.map((rule) => {
    if (rule.type !== "required_status_checks") return rule;
    return {
      ...rule,
      parameters: {
        ...rule.parameters,
        required_status_checks: [{ context: "lint" }],
      },
    };
  });
  const report = diffProtection(expected, matchingActual({ rules }));
  assert.equal(report.ok, false);
  const row = report.findings.find((finding) => finding.kind === "wrong-parameter");
  assert.ok(row);
  assert.match(row.path, /required_status_checks/u);
  assert.equal(row.action, "update");
});

test("extra unexpected blocking rule is reported and not deletable", () => {
  const expected = expectedProtection();
  const extra = { type: "required_signatures" };
  const report = diffProtection(
    expected,
    matchingActual({ rules: [...expected.ruleset.rules, extra] }),
  );
  assert.equal(report.ok, false);
  const row = report.findings.find((finding) => finding.kind === "extra-blocking-rule");
  assert.ok(row);
  assert.equal(row.actual.type, "required_signatures");
  assert.equal(row.action, "report");
  assert.notEqual(row.action, "delete");
});

test("repository merge commit enabled is repo-setting drift", () => {
  const expected = expectedProtection();
  const report = diffProtection(
    expected,
    matchingActual({ repository: { allow_merge_commit: true } }),
  );
  assert.equal(report.ok, false);
  const row = report.findings.find((finding) => finding.kind === "repo-setting");
  assert.ok(row);
  assert.equal(row.path, "repository.allow_merge_commit");
  assert.equal(row.expected, false);
  assert.equal(row.actual, true);
  assert.equal(row.action, "patch");
});

test("check reports drift without mutation", () => {
  const adapter = fake();
  const report = protectionCheck({ adapter, owner: "pikax", repo: "verter" });
  assert.equal(report.mode, "check");
  assert.equal(report.ok, false);
  assert.equal(findingKinds(report).includes("missing-ruleset"), true);
  assert.deepEqual(adapter.rulesetWrites, []);
  assert.deepEqual(adapter.repositorySettingWrites, []);
});

test("apply with clearance creates and updates and records writes", () => {
  const expected = expectedProtection();
  const created = fake();
  const check = protectionCheck({ adapter: created });
  assert.equal(check.ok, false);
  assert.deepEqual(created.rulesetWrites, []);
  assert.deepEqual(created.repositorySettingWrites, []);

  const createdReport = protectionApply({
    adapter: created,
    owner: "pikax",
    repo: "verter",
    clearance: clearanceFor(created),
  });
  assert.equal(createdReport.mode, "apply");
  assert.equal(createdReport.ok, true);
  assert.equal(created.rulesetWrites.length, 1);
  assert.equal(created.rulesetWrites[0].kind, "create");
  assert.equal(created.rulesetWrites[0].payload.name, expected.ruleset.name);
  assert.equal(created.repositorySettingWrites.length, 1);
  assert.equal(created.repositorySettingWrites[0].kind, "patch");
  assert.equal(created.getRepositorySettings().allow_merge_commit, false);
  assert.equal(created.listRulesets()[0].name, expected.ruleset.name);

  const drifted = fake({
    rulesets: [
      {
        id: 7,
        ...expected.ruleset,
        enforcement: "disabled",
      },
    ],
    repositorySettings: expected.repositorySettings,
  });
  const updatedReport = protectionApply({
    adapter: drifted,
    clearance: clearanceFor(drifted),
  });
  assert.equal(updatedReport.ok, true);
  assert.equal(drifted.rulesetWrites.length, 1);
  assert.equal(drifted.rulesetWrites[0].kind, "update");
  assert.equal(drifted.rulesetWrites[0].id, 7);
  assert.equal(drifted.getRuleset(7).enforcement, "active");
  assert.deepEqual(drifted.repositorySettingWrites, []);
});

test("apply without clearance throws DoctorRequiredError", () => {
  const adapter = fake();
  assert.throws(
    () => protectionApply({ adapter, owner: "pikax", repo: "verter" }),
    DoctorRequiredError,
  );
  assert.deepEqual(adapter.rulesetWrites, []);
  assert.deepEqual(adapter.repositorySettingWrites, []);
});

test("apply never touches a differently-named ruleset", () => {
  const expected = expectedProtection();
  const other = {
    id: 9,
    name: "legacy-branch-protection",
    target: "branch",
    enforcement: "active",
    conditions: { ref_name: { include: ["~DEFAULT_BRANCH"], exclude: [] } },
    rules: [{ type: "deletion" }, { type: "required_signatures" }],
  };
  const adapter = fake({
    rulesets: [
      {
        id: 3,
        ...expected.ruleset,
        enforcement: "disabled",
      },
      other,
    ],
    repositorySettings: expected.repositorySettings,
  });
  const before = adapter.getRuleset(9);
  const report = protectionApply({
    adapter,
    clearance: clearanceFor(adapter),
  });
  assert.equal(report.ok, true);
  assert.equal(adapter.rulesetWrites.length, 1);
  assert.equal(adapter.rulesetWrites[0].id, 3);
  assert.equal(
    adapter.rulesetWrites.some((row) => row.id === 9),
    false,
  );
  assert.deepEqual(adapter.getRuleset(9), before);
});

test("GitHubAdapter lists rulesets with pagination and POSTs the expected payload", () => {
  const expected = expectedProtection();
  let posted = null;
  const transport = transportMap({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": {
      full_name: "pikax/verter",
      has_issues: true,
      permissions: { admin: true, maintain: false, push: true, triage: false, pull: true },
      allow_squash_merge: true,
      allow_merge_commit: false,
      allow_rebase_merge: false,
      allow_auto_merge: false,
      delete_branch_on_merge: true,
    },
    "GET /repos/pikax/verter/rulesets?per_page=100": [],
    "POST /repos/pikax/verter/rulesets": (request) => {
      posted = request.body;
      return { id: 42, ...request.body };
    },
  });
  const adapter = new GitHubAdapter({ owner: "pikax", repo: "verter", transport });
  const doctor = new GitHubDoctor(adapter).check({ require: ["admin"] });
  assert.equal(doctor.ok, true);
  transport.calls.length = 0;
  const report = protectionApply({
    adapter,
    owner: "pikax",
    repo: "verter",
    clearance: doctor.clearance,
  });
  assert.equal(report.ok, true);
  assert.deepEqual(posted, {
    name: expected.ruleset.name,
    target: expected.ruleset.target,
    enforcement: expected.ruleset.enforcement,
    conditions: expected.ruleset.conditions,
    rules: expected.ruleset.rules,
  });
  assert.deepEqual(
    transport.calls.map((call) => `${call.method} ${call.path}`),
    [
      "GET /repos/pikax/verter/rulesets?per_page=100",
      "GET /repos/pikax/verter",
      "POST /repos/pikax/verter/rulesets",
    ],
  );
});
