import {
  applyOperations,
  assertIssueNumber,
  bindOwnerRepo,
  capabilityRecord,
  planCreateIssue,
  planCreatePullRequest,
  planUpdateIssue,
  prepareCreateIssue,
  prepareCreatePullRequest,
  prepareUpdateIssue,
} from "./adapter.mjs";
import {
  DuplicateError,
  GitHubAdapterError,
  NotFoundError,
  PermissionDeniedError,
  ProtectedMappingError,
} from "./errors.mjs";

function cloneComments(comments) {
  return Array.isArray(comments) ? comments.map((row) => ({ id: row.id, body: row.body })) : [];
}

function cloneIssue(issue) {
  return {
    number: issue.number,
    title: issue.title,
    body: issue.body,
    comments: cloneComments(issue.comments),
  };
}

function clonePull(pull) {
  return {
    number: pull.number,
    title: pull.title,
    body: pull.body,
    head: pull.head,
    base: pull.base,
    closes: pull.closes,
  };
}

export class FakeGitHubAdapter {
  #issues;
  #pulls;
  #heads;
  #applyAttempts;

  constructor(options = {}) {
    bindOwnerRepo(this, options, "FakeGitHubAdapter");
    this.authenticated = options.authenticated !== false;
    this.login = typeof options.login === "string" && options.login ? options.login : "fake-user";
    this.repositoryAccess = options.repositoryAccess !== false;
    this.permissions = {
      issues: options.permissions?.issues !== false,
      pullRequests: options.permissions?.pullRequests !== false,
    };
    this.failOnApply = options.failOnApply;
    this.failOnApplyError = options.failOnApplyError;
    this.refusals = [];
    this.reads = [];
    this.#issues = new Map();
    this.#pulls = new Map();
    this.#heads = new Set();
    this.#applyAttempts = 0;
    for (const issue of options.issues ?? []) {
      const number = assertIssueNumber(issue.number);
      this.#claimNumber(number, `issue #${number} already exists`);
      this.#issues.set(number, {
        number,
        title: issue.title,
        body: issue.body,
        comments: cloneComments(issue.comments),
      });
    }
    for (const pull of options.pullRequests ?? []) {
      const number = assertIssueNumber(pull.number, "pull request number");
      if (this.#heads.has(pull.head)) throw new DuplicateError("pull request already exists");
      this.#claimNumber(number, "pull request already exists");
      this.#pulls.set(number, clonePull({ ...pull, number }));
      this.#heads.add(pull.head);
    }
    const used = [...this.#issues.keys(), ...this.#pulls.keys()];
    const maxUsed = used.length ? Math.max(...used) : 0;
    const nextIssue = options.nextNumber ?? options.nextIssueNumber;
    const nextPull = options.nextPullNumber;
    if (nextIssue != null && nextPull != null && nextIssue !== nextPull) {
      throw new GitHubAdapterError("fake issue and pull request numbers share one sequence");
    }
    this.nextNumber = nextIssue ?? nextPull ?? maxUsed + 1;
  }

  inspectCapabilities() {
    if (!this.authenticated) {
      return capabilityRecord({
        authenticated: false,
        repository: null,
        issues: false,
        pullRequests: false,
      });
    }
    if (!this.repositoryAccess) {
      return capabilityRecord({
        authenticated: true,
        login: this.login,
        repository: null,
        issues: false,
        pullRequests: false,
      });
    }
    return capabilityRecord({
      authenticated: true,
      login: this.login,
      repository: { owner: this.owner, repo: this.repo },
      issues: this.permissions.issues,
      pullRequests: this.permissions.pullRequests,
    });
  }

  #claimNumber(number, message) {
    if (this.#issues.has(number) || this.#pulls.has(number)) throw new DuplicateError(message);
  }

  #allocateNumber() {
    const number = this.nextNumber;
    this.#claimNumber(number, `resource #${number} already exists`);
    this.nextNumber += 1;
    return number;
  }

  #beginApply() {
    const index = this.#applyAttempts;
    this.#applyAttempts += 1;
    if (this.failOnApply === index) {
      throw this.failOnApplyError ?? new PermissionDeniedError("configured apply failure");
    }
  }

  createIssue(request) {
    const mode = prepareCreateIssue(this, request);
    if (mode === "check") return planCreateIssue(request);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const number = this.#allocateNumber();
    this.#issues.set(number, {
      number,
      title: request.title,
      body: request.body,
      comments: [],
    });
    return {
      kind: "create-issue",
      number,
      title: request.title,
      body: request.body,
      applied: true,
    };
  }

  updateIssue(request) {
    try {
      const prepared = prepareUpdateIssue(this, request);
      if (prepared.mode === "check") return planUpdateIssue(request, prepared.number);
      this.#beginApply();
      if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
      const issue = this.#issues.get(prepared.number);
      if (!issue) throw new NotFoundError(`issue #${prepared.number} is not in the fake`);
      issue.title = request.title;
      issue.body = request.body;
      return {
        kind: "update-issue",
        number: prepared.number,
        title: request.title,
        body: request.body,
        applied: true,
      };
    } catch (error) {
      if (error instanceof ProtectedMappingError) {
        this.refusals.push({
          kind: "protected-mapping",
          number: request.number,
          mapping: {
            node_id: request.mapping.node_id,
            gh_issue: request.mapping.gh_issue,
            sync_to_github: request.mapping.sync_to_github,
          },
        });
      }
      throw error;
    }
  }

  createPullRequest(request) {
    const { mode, mappedIssue } = prepareCreatePullRequest(this, request);
    if (mode === "check") return planCreatePullRequest(request, mappedIssue);
    this.#beginApply();
    if (!this.permissions.pullRequests)
      throw new PermissionDeniedError("pull request permission denied");
    if (this.#heads.has(request.head)) {
      throw new DuplicateError(`pull request already exists for head ${request.head}`);
    }
    const number = this.#allocateNumber();
    const record = {
      number,
      title: request.title,
      body: request.body,
      head: request.head,
      base: request.base,
      closes: mappedIssue,
    };
    this.#pulls.set(number, record);
    this.#heads.add(request.head);
    return { kind: "create-pull-request", ...record, applied: true };
  }

  applyOperations(operations) {
    return applyOperations(this, operations);
  }

  #cloneIssue(number) {
    const issue = this.#issues.get(number);
    return issue ? cloneIssue(issue) : null;
  }

  getIssue(number) {
    const expected = assertIssueNumber(number);
    this.reads.push({ kind: "get-issue", number: expected });
    return this.#cloneIssue(expected);
  }

  getIssues() {
    return [...this.#issues.keys()]
      .sort((left, right) => left - right)
      .map((number) => this.#cloneIssue(number));
  }

  getPullRequest(number) {
    const pull = this.#pulls.get(number);
    return pull ? clonePull(pull) : null;
  }

  getPullRequests() {
    return [...this.#pulls.keys()]
      .sort((left, right) => left - right)
      .map((number) => this.getPullRequest(number));
  }

  inspectState() {
    return {
      nextNumber: this.nextNumber,
      issues: this.getIssues(),
      pullRequests: this.getPullRequests(),
      refusals: this.refusals.map((row) => ({
        kind: row.kind,
        number: row.number,
        mapping: { ...row.mapping },
      })),
    };
  }
}
