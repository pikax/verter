import { spawnSync } from "node:child_process";

import {
  ClosingLinkError,
  DoctorRequiredError,
  DuplicateError,
  GitHubAdapterError,
  InvalidIssueNumberError,
  LiveGitHubForbiddenInTestsError,
  MappingMismatchError,
  MutationModeRequiredError,
  NotFoundError,
  PartialFailureError,
  PermissionDeniedError,
  ProtectedMappingError,
  UnstructuredGitHubOutputError,
  WrongRepositoryError,
} from "./errors.mjs";

const MINTED_CLEARANCES = new WeakMap();
const OWNER_REPO = /^[A-Za-z0-9_.-]+$/;

export function bindOwnerRepo(target, options, label) {
  const owner = options.owner;
  const repo = options.repo;
  if (typeof owner !== "string" || !owner || typeof repo !== "string" || !repo) {
    throw new GitHubAdapterError(`${label} requires owner and repo`);
  }
  if (!OWNER_REPO.test(owner) || !OWNER_REPO.test(repo)) {
    throw new GitHubAdapterError(`${label} owner and repo must match [A-Za-z0-9_.-]+`);
  }
  Object.defineProperty(target, "owner", { get: () => owner, enumerable: true });
  Object.defineProperty(target, "repo", { get: () => repo, enumerable: true });
}

export function capabilityRecord({ authenticated, login, repository, issues, pullRequests }) {
  const record = {
    authenticated: authenticated === true,
    repository: repository ?? null,
    issues: issues === true,
    pullRequests: pullRequests === true,
  };
  if (typeof login === "string" && login.length > 0) record.login = login;
  return record;
}

export function mintDoctorClearance(adapter, clearance) {
  MINTED_CLEARANCES.set(clearance, adapter);
  return clearance;
}

export function planCreateIssue(request) {
  return { kind: "create-issue", title: request.title, body: request.body, applied: false };
}

export function planUpdateIssue(request, number) {
  return {
    kind: "update-issue",
    number,
    title: request.title,
    body: request.body,
    applied: false,
  };
}

export function planCreatePullRequest(request, mappedIssue) {
  return {
    kind: "create-pull-request",
    title: request.title,
    body: request.body,
    head: request.head,
    base: request.base,
    closes: mappedIssue,
    applied: false,
  };
}

export function parseGitHubResourceNumber(payload) {
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub response is not a JSON object");
  }
  const number = payload.number;
  if (!Number.isSafeInteger(number) || number < 1) {
    throw new UnstructuredGitHubOutputError(
      "GitHub response did not include a numeric resource number",
    );
  }
  return number;
}

export function assertIssueNumber(value, label = "issue number") {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new InvalidIssueNumberError(`${label} must be a positive safe integer`);
  }
  return value;
}

export function mappedClosingLink(issueNumber) {
  return `Closes #${assertIssueNumber(issueNumber, "mapped issue")}`;
}

export function hasExactMappedClosingLink(body, issueNumber) {
  if (typeof body !== "string") return false;
  const link = mappedClosingLink(issueNumber);
  let from = 0;
  while (from <= body.length) {
    const index = body.indexOf(link, from);
    if (index === -1) return false;
    const after = body[index + link.length];
    if (after === undefined || after < "0" || after > "9") return true;
    from = index + 1;
  }
  return false;
}

export function assertMutationMode(mode) {
  if (mode === "check" || mode === "apply") return mode;
  throw new MutationModeRequiredError("mutation requires mode 'check' or 'apply'");
}

export function assertApplyClearance(mode, clearance, capability, adapter) {
  if (mode !== "apply") return;
  if (
    MINTED_CLEARANCES.get(clearance) !== adapter ||
    clearance[capability] !== true ||
    clearance.owner !== adapter.owner ||
    clearance.repo !== adapter.repo
  ) {
    throw new DoctorRequiredError(`apply requires GitHubDoctor ${capability} clearance`);
  }
}

export function assertRepository(adapter, request) {
  if (request.owner != null && request.owner !== adapter.owner) {
    throw new WrongRepositoryError(`GitHubAdapter is bound to ${adapter.owner}/${adapter.repo}`);
  }
  if (request.repo != null && request.repo !== adapter.repo) {
    throw new WrongRepositoryError(`GitHubAdapter is bound to ${adapter.owner}/${adapter.repo}`);
  }
}

export function assertRequiredText(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    throw new GitHubAdapterError(`${label} is required`);
  }
}

export function mappingPolicy(mapping, number) {
  if (!mapping || typeof mapping !== "object") {
    throw new MappingMismatchError("issue update requires a GitHubIssueMapping");
  }
  if (typeof mapping.node_id !== "string" || mapping.node_id.length === 0) {
    throw new MappingMismatchError("mapping.node_id is required");
  }
  assertIssueNumber(mapping.gh_issue, "mapping.gh_issue");
  if (typeof mapping.sync_to_github !== "boolean") {
    throw new MappingMismatchError("mapping.sync_to_github is required policy");
  }
  if (mapping.gh_issue !== number) {
    throw new MappingMismatchError(
      `mapping.gh_issue ${mapping.gh_issue} does not match issue ${number}`,
    );
  }
  return mapping.sync_to_github ? "opt-in" : "protected";
}

export function prepareCreateIssue(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  assertRequiredText(request.title, "issue title");
  if (typeof request.body !== "string") throw new GitHubAdapterError("issue body must be a string");
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return mode;
}

export function prepareUpdateIssue(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const number = assertIssueNumber(request.number);
  assertRequiredText(request.title, "issue title");
  if (typeof request.body !== "string") throw new GitHubAdapterError("issue body must be a string");
  const policy = mappingPolicy(request.mapping, number);
  if (policy === "protected") {
    throw new ProtectedMappingError(
      `refusing update of protected issue #${number} (${request.mapping.node_id})`,
    );
  }
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, number };
}

export function prepareCreatePullRequest(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const mappedIssue = assertIssueNumber(request.mappedIssue, "mapped issue");
  assertRequiredText(request.title, "pull request title");
  assertRequiredText(request.head, "pull request head");
  assertRequiredText(request.base, "pull request base");
  if (typeof request.body !== "string") {
    throw new GitHubAdapterError("pull request body must be a string");
  }
  if (!hasExactMappedClosingLink(request.body, mappedIssue)) {
    throw new ClosingLinkError(
      `pull request body must contain exact ${mappedClosingLink(mappedIssue)}`,
    );
  }
  assertApplyClearance(mode, request.clearance, "pullRequests", adapter);
  return { mode, mappedIssue };
}

function dispatchOperation(adapter, operation) {
  const op = operation?.op;
  if (op === "createIssue") return adapter.createIssue(operation);
  if (op === "updateIssue") return adapter.updateIssue(operation);
  if (op === "createPullRequest") return adapter.createPullRequest(operation);
  throw new GitHubAdapterError(`unknown operation ${op}`);
}

export function applyOperations(adapter, operations) {
  if (!Array.isArray(operations)) throw new GitHubAdapterError("operations must be an array");
  const succeeded = [];
  for (const operation of operations) {
    try {
      succeeded.push(dispatchOperation(adapter, operation));
    } catch (error) {
      if (succeeded.length === 0) throw error;
      throw new PartialFailureError({ succeeded, failed: { operation, error } });
    }
  }
  return succeeded;
}

function duplicateMessage(payload) {
  if (
    typeof payload?.message === "string" &&
    payload.message.toLowerCase().includes("already exists")
  ) {
    return payload.message;
  }
  if (!Array.isArray(payload?.errors)) return null;
  for (const row of payload.errors) {
    if (!row || typeof row !== "object") continue;
    if (row.code === "already_exists") {
      return typeof row.message === "string" && row.message ? row.message : payload.message;
    }
    if (typeof row.message === "string" && row.message.toLowerCase().includes("already exists")) {
      return row.message;
    }
  }
  return null;
}

function errorFromHttpStatus(status, payload, fallbackMessage) {
  const message =
    typeof payload.message === "string" && payload.message ? payload.message : fallbackMessage;
  if (status === 401 || status === 403) return new PermissionDeniedError(message);
  if (status === 404) return new NotFoundError(message);
  const duplicate = duplicateMessage(payload);
  return duplicate ? new DuplicateError(duplicate) : new GitHubAdapterError(message);
}

const HTTP_STATUS_LINE = /^HTTP\/\d[\d.]*\s+(\d{3})\b/;

function parseGhApiIncludeStdout(stdout) {
  const text = typeof stdout === "string" ? stdout : "";
  const split = text.search(/\r?\n\r?\n/);
  const headerBlock = split === -1 ? text : text.slice(0, split);
  const bodyText = split === -1 ? "" : text.slice(split).replace(/^\r?\n\r?\n/, "");
  const matched = HTTP_STATUS_LINE.exec(headerBlock.split(/\r?\n/, 1)[0] ?? "");
  let payload;
  try {
    payload = JSON.parse(bodyText);
  } catch {
    payload = undefined;
  }
  if (!matched || payload === null || typeof payload !== "object" || Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError(
      matched ? "gh api JSON was not an object" : "gh api output is missing an HTTP status line",
    );
  }
  return { status: Number.parseInt(matched[1], 10), payload };
}

function expectedCapabilityMiss(error) {
  return error instanceof PermissionDeniedError || error instanceof NotFoundError;
}

export function createGhApiTransport({ spawn = spawnSync } = {}) {
  if (process.env.NODE_TEST_CONTEXT && spawn === spawnSync) {
    throw new LiveGitHubForbiddenInTestsError(
      "Live GitHub is not a test substrate; use FakeGitHubAdapter",
    );
  }
  return {
    request({ method, path, body }) {
      const args = ["api", "--include", "-X", method, path];
      const input = body === undefined ? undefined : JSON.stringify(body);
      if (input !== undefined) args.push("--input", "-");
      const result = spawn("gh", args, {
        encoding: "utf8",
        input,
        maxBuffer: 4 * 1024 * 1024,
      });
      if (result.error) throw new GitHubAdapterError(result.error.message);
      const { status, payload } = parseGhApiIncludeStdout(result.stdout ?? "");
      if (status < 200 || status >= 300) {
        throw errorFromHttpStatus(status, payload, "gh api request failed");
      }
      return payload;
    },
  };
}

export class GitHubAdapter {
  #transport;

  constructor(options = {}) {
    bindOwnerRepo(this, options, "GitHubAdapter");
    if (options.transport) this.#transport = options.transport;
    else if (process.env.NODE_TEST_CONTEXT) {
      throw new LiveGitHubForbiddenInTestsError(
        "Live GitHub is not a test substrate; use FakeGitHubAdapter",
      );
    } else this.#transport = createGhApiTransport();
  }

  inspectCapabilities() {
    let user;
    try {
      user = this.#transport.request({ method: "GET", path: "/user" });
    } catch (error) {
      if (!expectedCapabilityMiss(error)) throw error;
      return capabilityRecord({
        authenticated: false,
        repository: null,
        issues: false,
        pullRequests: false,
      });
    }
    if (
      user === null ||
      typeof user !== "object" ||
      Array.isArray(user) ||
      typeof user.login !== "string" ||
      user.login.length === 0
    ) {
      throw new UnstructuredGitHubOutputError("GET /user did not return a JSON login");
    }
    let repo;
    try {
      repo = this.#transport.request({
        method: "GET",
        path: `/repos/${this.owner}/${this.repo}`,
      });
    } catch (error) {
      if (!expectedCapabilityMiss(error)) throw error;
      return capabilityRecord({
        authenticated: true,
        login: user.login,
        repository: null,
        issues: false,
        pullRequests: false,
      });
    }
    if (repo === null || typeof repo !== "object" || Array.isArray(repo)) {
      throw new UnstructuredGitHubOutputError("GET /repos JSON was not an object");
    }
    if (typeof repo.full_name !== "string" || repo.full_name.length === 0) {
      throw new UnstructuredGitHubOutputError("GET /repos did not return a JSON full_name");
    }
    if (repo.full_name !== `${this.owner}/${this.repo}`) {
      return capabilityRecord({
        authenticated: true,
        login: user.login,
        repository: null,
        issues: false,
        pullRequests: false,
      });
    }
    const permissions =
      repo.permissions && typeof repo.permissions === "object" && !Array.isArray(repo.permissions)
        ? repo.permissions
        : {};
    const issueWrite =
      permissions.admin === true ||
      permissions.maintain === true ||
      permissions.push === true ||
      permissions.triage === true;
    const pullWrite =
      permissions.admin === true || permissions.maintain === true || permissions.push === true;
    return capabilityRecord({
      authenticated: true,
      login: user.login,
      repository: { owner: this.owner, repo: this.repo },
      issues: repo.has_issues === true && issueWrite,
      pullRequests: pullWrite,
    });
  }

  createIssue(request) {
    const mode = prepareCreateIssue(this, request);
    if (mode === "check") return planCreateIssue(request);
    const payload = this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/issues`,
      body: { title: request.title, body: request.body },
    });
    return {
      kind: "create-issue",
      number: parseGitHubResourceNumber(payload),
      title: request.title,
      body: request.body,
      applied: true,
    };
  }

  updateIssue(request) {
    const { mode, number } = prepareUpdateIssue(this, request);
    if (mode === "check") return planUpdateIssue(request, number);
    const payload = this.#transport.request({
      method: "PATCH",
      path: `/repos/${this.owner}/${this.repo}/issues/${number}`,
      body: { title: request.title, body: request.body },
    });
    const returned = parseGitHubResourceNumber(payload);
    if (returned !== number) {
      throw new UnstructuredGitHubOutputError(
        `GitHub issue update returned number ${returned}, expected ${number}`,
      );
    }
    return {
      kind: "update-issue",
      number,
      title: request.title,
      body: request.body,
      applied: true,
    };
  }

  createPullRequest(request) {
    const { mode, mappedIssue } = prepareCreatePullRequest(this, request);
    if (mode === "check") return planCreatePullRequest(request, mappedIssue);
    const payload = this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/pulls`,
      body: {
        title: request.title,
        body: request.body,
        head: request.head,
        base: request.base,
      },
    });
    return {
      kind: "create-pull-request",
      number: parseGitHubResourceNumber(payload),
      title: request.title,
      body: request.body,
      head: request.head,
      base: request.base,
      closes: mappedIssue,
      applied: true,
    };
  }

  applyOperations(operations) {
    return applyOperations(this, operations);
  }
}
