import {
  applyOperations,
  assertIssueNumber,
  assertRequiredText,
  bindOwnerRepo,
  capabilityRecord,
  planAddIssueToProject,
  planCreateIssue,
  planCreatePullRequest,
  planCreatePullRequestComment,
  planSetIssueMilestone,
  planUpdateIssue,
  prepareAddIssueToProject,
  prepareCreateIssue,
  prepareCreatePullRequest,
  prepareCreatePullRequestComment,
  prepareSetIssueMilestone,
  prepareUpdateIssue,
  PROJECT_NUMBER,
} from "./adapter.mjs";
import {
  DuplicateError,
  GitHubAdapterError,
  MissingProjectIdentityError,
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
    milestone: issue.milestone ?? null,
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
    comments: cloneComments(pull.comments),
  };
}

export class FakeGitHubAdapter {
  #issues;
  #pulls;
  #heads;
  #applyAttempts;
  #projectMissing;
  #projectItems;
  #milestones;

  constructor(options = {}) {
    bindOwnerRepo(this, options, "FakeGitHubAdapter");
    if (options.projectNumber != null && options.projectNumber !== PROJECT_NUMBER) {
      throw new GitHubAdapterError("refusing to create a project other than Project 3");
    }
    this.authenticated = options.authenticated !== false;
    this.login = typeof options.login === "string" && options.login ? options.login : "fake-user";
    this.repositoryAccess = options.repositoryAccess !== false;
    this.permissions = {
      issues: options.permissions?.issues !== false,
      pullRequests: options.permissions?.pullRequests !== false,
      projects: options.permissions?.projects !== false,
    };
    this.failOnApply = options.failOnApply;
    this.failOnApplyError = options.failOnApplyError;
    this.refusals = [];
    this.reads = [];
    this.milestoneWrites = [];
    this.#issues = new Map();
    this.#pulls = new Map();
    this.#heads = new Set();
    this.#applyAttempts = 0;
    this.#projectMissing = options.missing === true;
    this.#projectItems = new Set();
    this.#milestones = new Map();
    for (const number of options.projectItems ?? []) {
      this.#projectItems.add(assertIssueNumber(number));
    }
    for (const row of options.milestones ?? []) {
      this.#milestones.set(row.title, row);
    }
    for (const issue of options.issues ?? []) {
      const number = assertIssueNumber(issue.number);
      this.#claimNumber(number, `issue #${number} already exists`);
      this.#issues.set(number, {
        number,
        title: issue.title,
        body: issue.body,
        comments: cloneComments(issue.comments),
        milestone: issue.milestone ?? null,
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
      projects: this.permissions.projects && !this.#projectMissing,
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
      milestone: null,
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
      comments: [],
    };
    this.#pulls.set(number, record);
    this.#heads.add(request.head);
    return { kind: "create-pull-request", ...record, applied: true };
  }

  createPullRequestComment(request) {
    const { mode, number } = prepareCreatePullRequestComment(this, request);
    if (mode === "check") return planCreatePullRequestComment(request, number);
    this.#beginApply();
    if (!this.permissions.pullRequests) {
      throw new PermissionDeniedError("pull request permission denied");
    }
    const pull = this.#pulls.get(number);
    if (!pull) throw new NotFoundError(`pull request #${number} is missing`);
    if (!Array.isArray(pull.comments)) pull.comments = [];
    const id = pull.comments.length === 0 ? 1 : Math.max(...pull.comments.map((row) => row.id)) + 1;
    pull.comments.push({ id, body: request.body });
    return {
      kind: "create-pull-request-comment",
      number,
      body: request.body,
      applied: true,
    };
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

  pullsForHead(head) {
    assertRequiredText(head, "pull request head");
    return this.getPullRequests().filter((row) => row.head === head);
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
      projectItems: this.getProjectItems(PROJECT_NUMBER),
      milestoneWrites: this.milestoneWrites.map((row) => ({
        issueNumber: row.issueNumber,
        title: row.title,
      })),
    };
  }

  getProject(number = PROJECT_NUMBER) {
    if (number !== PROJECT_NUMBER) {
      throw new GitHubAdapterError(`scheduling overlay uses GitHub Project ${PROJECT_NUMBER} only`);
    }
    if (this.#projectMissing) {
      throw new MissingProjectIdentityError(`GitHub Project ${PROJECT_NUMBER} is missing`);
    }
    return { id: "fake-project-3", number: PROJECT_NUMBER };
  }

  getProjectItems(number = PROJECT_NUMBER) {
    if (number !== PROJECT_NUMBER) {
      throw new GitHubAdapterError(`scheduling overlay uses GitHub Project ${PROJECT_NUMBER} only`);
    }
    return [...this.#projectItems].sort((left, right) => left - right);
  }

  addIssueToProject(request) {
    const { mode, issueNumber } = prepareAddIssueToProject(this, request);
    this.getProject(PROJECT_NUMBER);
    if (mode === "check") return planAddIssueToProject(issueNumber);
    this.#beginApply();
    if (!this.permissions.projects) throw new PermissionDeniedError("projects permission denied");
    if (!this.#issues.has(issueNumber)) {
      throw new NotFoundError(`issue #${issueNumber} is missing`);
    }
    const alreadyMember = this.#projectItems.has(issueNumber);
    this.#projectItems.add(issueNumber);
    return {
      kind: "add-project-item",
      number: PROJECT_NUMBER,
      issueNumber,
      applied: true,
      already_member: alreadyMember,
    };
  }

  setIssueMilestone(request) {
    const { mode, issueNumber, title } = prepareSetIssueMilestone(this, request);
    if (this.#pulls.has(issueNumber)) {
      throw new GitHubAdapterError("ReleaseTarget is set on the issue, never on a PR");
    }
    if (mode === "check") return planSetIssueMilestone(issueNumber, title);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    if (!this.#milestones.has(title)) throw new NotFoundError(`milestone ${title} is missing`);
    const issue = this.#issues.get(issueNumber);
    if (!issue) throw new NotFoundError(`issue #${issueNumber} is not in the fake`);
    issue.milestone = title;
    this.milestoneWrites.push({ issueNumber, title });
    return { kind: "set-milestone", issueNumber, title, applied: true };
  }
}
