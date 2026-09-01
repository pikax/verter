import {
  AI_OWNED_LABELS,
  applyOperations,
  assertIssueNumber,
  assertRequiredText,
  bindOwnerRepo,
  capabilityRecord,
  classifyIssueSubIssueState,
  planAddIssueSubIssue,
  planAddIssueToProject,
  planAddIssueLabels,
  planCreateIssue,
  planCreateRepositoryLabel,
  planCreateRepositoryMilestone,
  planCloseMilestone,
  planCreatePullRequest,
  planCreatePullRequestComment,
  planCreateReleasePullRequest,
  planMergePullRequest,
  planDispatchReleaseRehearsal,
  planSetAiResultLabel,
  planSetIssueProjectStatus,
  planSetIssueMilestone,
  planAddIssueDependency,
  planRemoveIssueDependency,
  planUpdateIssue,
  planUpdateRepositoryLabel,
  planUpdateRepositoryMilestone,
  planRemoveIssueLabel,
  prepareAddIssueToProject,
  prepareAddIssueLabels,
  prepareCloseMilestone,
  prepareCreateIssue,
  prepareCreateRepositoryLabel,
  prepareCreateRepositoryMilestone,
  prepareCreatePullRequest,
  prepareCreatePullRequestComment,
  prepareCreateReleasePullRequest,
  prepareDispatchReleaseRehearsal,
  prepareMergePullRequest,
  prepareSetAiResultLabel,
  prepareSetIssueProjectStatus,
  prepareSetIssueMilestone,
  prepareAddIssueDependency,
  prepareAddIssueSubIssue,
  prepareRemoveIssueDependency,
  prepareUpdateIssue,
  prepareUpdateRepositoryLabel,
  prepareUpdateRepositoryMilestone,
  prepareRemoveIssueLabel,
  PROJECT_NUMBER,
  selectAiResultLabel,
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

function cloneLabels(labels) {
  if (labels == null) return [];
  if (!Array.isArray(labels)) throw new GitHubAdapterError("issue labels must be an array");
  return labels.map((row) => {
    if (typeof row === "string" && row.length > 0) return row;
    if (row && typeof row === "object" && typeof row.name === "string" && row.name.length > 0) {
      return row.name;
    }
    throw new GitHubAdapterError("label name is required");
  });
}

function cloneRepositoryLabel(label) {
  if (label === null || typeof label !== "object" || Array.isArray(label)) {
    throw new GitHubAdapterError("repository label definition is required");
  }
  if (typeof label.name !== "string" || label.name.length === 0) {
    throw new GitHubAdapterError("repository label name is required");
  }
  if (typeof label.color !== "string" || !/^[0-9a-f]{6}$/iu.test(label.color)) {
    throw new GitHubAdapterError("repository label color must be six hexadecimal characters");
  }
  const description = label.description == null ? "" : label.description;
  if (typeof description !== "string") {
    throw new GitHubAdapterError("repository label description must be a string");
  }
  return { name: label.name, color: label.color.toLowerCase(), description };
}

function labelKey(name) {
  return name.toLocaleLowerCase("en-US");
}

function cloneIssue(issue) {
  return {
    number: issue.number,
    title: issue.title,
    body: issue.body,
    comments: cloneComments(issue.comments),
    milestone: issue.milestone ?? null,
    labels: cloneLabels(issue.labels),
    state: issue.state === "closed" ? "closed" : "open",
    parent: issue.parent ?? null,
    subIssues: [...(issue.subIssues ?? [])],
    dependencies: [...(issue.dependencies ?? [])],
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

function cloneCheckRun(row) {
  const conclusion = row?.conclusion == null ? "pending" : row.conclusion;
  return {
    name: row.name,
    conclusion,
    skipped: conclusion === "skipped",
  };
}

export class FakeGitHubAdapter {
  #issues;
  #repositoryLabels;
  #pulls;
  #heads;
  #applyAttempts;
  #projectMissing;
  #projectItems;
  #projectStatuses;
  #milestones;
  #checkRuns;
  #merges;

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
      projectRead: options.permissions?.projectRead !== false,
      actions: options.permissions?.actions !== false,
    };
    this.failOnApply = options.failOnApply;
    this.failOnApplyError = options.failOnApplyError;
    this.refusals = [];
    this.reads = [];
    this.milestoneWrites = [];
    this.milestoneCloses = [];
    this.repositoryMilestoneWrites = [];
    this.dependencyWrites = [];
    this.subIssueWrites = [];
    this.labelWrites = [];
    this.repositoryLabelWrites = [];
    this.projectStatusWrites = [];
    this.workflowDispatches = [];
    this.#issues = new Map();
    this.#repositoryLabels = new Map();
    this.#pulls = new Map();
    this.#heads = new Set();
    this.#applyAttempts = 0;
    this.#projectMissing = options.missing === true;
    this.#projectItems = new Set();
    this.#projectStatuses = new Map();
    this.#milestones = new Map();
    this.#checkRuns = new Map();
    this.#merges = [];
    for (const label of options.repositoryLabels ?? []) {
      const cloned = cloneRepositoryLabel(label);
      const key = labelKey(cloned.name);
      if (this.#repositoryLabels.has(key)) {
        throw new DuplicateError(`repository label ${cloned.name} already exists`);
      }
      this.#repositoryLabels.set(key, cloned);
    }
    for (const number of options.projectItems ?? []) {
      this.#projectItems.add(assertIssueNumber(number));
    }
    for (const [number, status] of Object.entries(options.projectStatuses ?? {})) {
      this.#projectStatuses.set(assertIssueNumber(Number(number)), status);
    }
    for (const [index, row] of (options.milestones ?? []).entries()) {
      this.#milestones.set(row.title, {
        number: row.number ?? index + 1,
        title: row.title,
        description: row.description ?? "",
        state: row.state ?? "open",
        ...(row.due_on == null ? {} : { due_on: row.due_on }),
      });
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
        labels: cloneLabels(issue.labels),
        state: issue.state === "closed" ? "closed" : "open",
        parent: issue.parent ?? null,
        subIssues: [...(issue.subIssues ?? [])],
        dependencies: [...(issue.dependencies ?? [])].map((candidate) =>
          assertIssueNumber(candidate, "blocking issue number"),
        ),
      });
    }
    for (const issue of this.#issues.values()) {
      if (issue.parent == null) continue;
      const parent = this.#issues.get(assertIssueNumber(issue.parent, "parent issue"));
      if (parent && !parent.subIssues.includes(issue.number)) parent.subIssues.push(issue.number);
    }
    for (const pull of options.pullRequests ?? []) {
      const number = assertIssueNumber(pull.number, "pull request number");
      if (this.#heads.has(pull.head)) throw new DuplicateError("pull request already exists");
      this.#claimNumber(number, "pull request already exists");
      const record = clonePull({ ...pull, number });
      record.merged = false;
      this.#pulls.set(number, record);
      this.#heads.add(pull.head);
      if (Array.isArray(pull.checkRuns)) {
        this.#checkRuns.set(number, pull.checkRuns.map(cloneCheckRun));
      }
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

  inspectCapabilities(options = {}) {
    const required = Array.isArray(options.require)
      ? new Set(options.require)
      : new Set(["issues", "pullRequests", "projects", "actions"]);
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
      projects:
        required.has("projects") &&
        this.permissions.projects &&
        this.permissions.projectRead &&
        !this.#projectMissing,
      actions: this.permissions.actions,
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
      labels: [],
      state: "open",
      parent: null,
      subIssues: [],
      dependencies: [],
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
      merged: false,
    };
    this.#pulls.set(number, record);
    this.#heads.add(request.head);
    return { kind: "create-pull-request", ...record, applied: true };
  }

  createReleasePullRequest(request) {
    const { mode } = prepareCreateReleasePullRequest(this, request);
    if (mode === "check") return planCreateReleasePullRequest(request);
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
      closes: null,
      comments: [],
      merged: false,
    };
    this.#pulls.set(number, record);
    this.#heads.add(request.head);
    return { kind: "create-release-pull-request", ...record, applied: true };
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

  listPullRequestCheckRuns(number) {
    const expected = assertIssueNumber(number, "pull request number");
    if (!this.#pulls.has(expected)) {
      throw new NotFoundError(`pull request #${expected} is missing`);
    }
    const rows = this.#checkRuns.get(expected) ?? [];
    return rows.map((row) => ({
      name: row.name,
      conclusion: row.conclusion,
      skipped: row.skipped,
    }));
  }

  setPullRequestCheckRuns(number, runs) {
    const expected = assertIssueNumber(number, "pull request number");
    if (!this.#pulls.has(expected)) {
      throw new NotFoundError(`pull request #${expected} is missing`);
    }
    if (!Array.isArray(runs)) throw new GitHubAdapterError("check runs must be an array");
    this.#checkRuns.set(expected, runs.map(cloneCheckRun));
  }

  mergePullRequest(request) {
    const { mode, number, mergeMethod, commitTitle } = prepareMergePullRequest(this, request);
    if (mode === "check") return planMergePullRequest(number, commitTitle);
    this.#beginApply();
    if (!this.permissions.pullRequests) {
      throw new PermissionDeniedError("pull request permission denied");
    }
    const pull = this.#pulls.get(number);
    if (!pull) throw new NotFoundError(`pull request #${number} is missing`);
    if (pull.merged === true) {
      throw new DuplicateError(`pull request #${number} is already merged`);
    }
    pull.merged = true;
    const record = { number, merge_method: mergeMethod };
    if (commitTitle != null) record.commit_title = commitTitle;
    this.#merges.push(record);
    return { kind: "squash-merge", ...record, applied: true };
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

  getIssueIdentity(number) {
    const expected = assertIssueNumber(number);
    this.reads.push({ kind: "get-issue-identity", number: expected });
    if (!this.#issues.has(expected)) {
      throw new NotFoundError(`issue #${expected} is not in the fake`);
    }
    return { id: expected * 1000, number: expected, owner: this.owner, repo: this.repo };
  }

  getIssueLabels(number) {
    const expected = assertIssueNumber(number);
    this.reads.push({ kind: "get-issue-labels", number: expected });
    const issue = this.#issues.get(expected);
    if (!issue) throw new NotFoundError(`issue #${expected} is not in the fake`);
    return [...issue.labels];
  }

  getRepositoryLabels() {
    this.reads.push({ kind: "get-repository-labels" });
    return [...this.#repositoryLabels.values()]
      .map(cloneRepositoryLabel)
      .sort((left, right) => left.name.localeCompare(right.name));
  }

  createRepositoryLabel(request) {
    const { mode, label } = prepareCreateRepositoryLabel(this, request);
    if (mode === "check") return planCreateRepositoryLabel(label);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const key = labelKey(label.name);
    if (this.#repositoryLabels.has(key)) {
      throw new DuplicateError(`repository label ${label.name} already exists`);
    }
    this.#repositoryLabels.set(key, cloneRepositoryLabel(label));
    this.repositoryLabelWrites.push({ kind: "create", label: cloneRepositoryLabel(label) });
    return { kind: "create-repository-label", label: cloneRepositoryLabel(label), applied: true };
  }

  updateRepositoryLabel(request) {
    const { mode, existing, label } = prepareUpdateRepositoryLabel(this, request);
    if (mode === "check") return planUpdateRepositoryLabel(existing, label);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const existingKey = labelKey(existing);
    if (!this.#repositoryLabels.has(existingKey)) {
      throw new NotFoundError(`repository label ${existing} is not in the fake`);
    }
    const nextKey = labelKey(label.name);
    if (existingKey !== nextKey && this.#repositoryLabels.has(nextKey)) {
      throw new DuplicateError(`repository label ${label.name} already exists`);
    }
    this.#repositoryLabels.delete(existingKey);
    this.#repositoryLabels.set(nextKey, cloneRepositoryLabel(label));
    for (const issue of this.#issues.values()) {
      issue.labels = issue.labels.map((name) =>
        labelKey(name) === existingKey ? label.name : name,
      );
    }
    this.repositoryLabelWrites.push({
      kind: "update",
      previous: existing,
      label: cloneRepositoryLabel(label),
    });
    return {
      kind: "update-repository-label",
      previous: existing,
      label: cloneRepositoryLabel(label),
      applied: true,
    };
  }

  getRepositoryMilestones() {
    this.reads.push({ kind: "get-repository-milestones" });
    return [...this.#milestones.values()]
      .map((row) => ({ ...row }))
      .sort((left, right) => left.number - right.number);
  }

  createRepositoryMilestone(request) {
    const { mode, milestone } = prepareCreateRepositoryMilestone(this, request);
    if (mode === "check") return planCreateRepositoryMilestone(milestone);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    if (this.#milestones.has(milestone.title)) {
      throw new DuplicateError(`repository milestone ${milestone.title} already exists`);
    }
    const number = Math.max(0, ...[...this.#milestones.values()].map((row) => row.number)) + 1;
    const created = { number, ...milestone, state: "open" };
    this.#milestones.set(created.title, created);
    this.repositoryMilestoneWrites.push({ kind: "create", title: created.title });
    return { kind: "create-repository-milestone", milestone: { ...created }, applied: true };
  }

  updateRepositoryMilestone(request) {
    const { mode, number, milestone } = prepareUpdateRepositoryMilestone(this, request);
    if (mode === "check") return planUpdateRepositoryMilestone(request.existing, milestone);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const existing = [...this.#milestones.values()].find((row) => row.number === number);
    if (!existing) throw new NotFoundError(`repository milestone #${number} is not in the fake`);
    this.#milestones.delete(existing.title);
    const updated = { ...existing, ...milestone, number };
    this.#milestones.set(updated.title, updated);
    for (const issue of this.#issues.values()) {
      if (issue.milestone === existing.title) issue.milestone = updated.title;
    }
    this.repositoryMilestoneWrites.push({
      kind: "update",
      number,
      previous: existing.title,
      title: updated.title,
    });
    return { kind: "update-repository-milestone", milestone: { ...updated }, applied: true };
  }

  addIssueLabels(request) {
    const { mode, number, labels } = prepareAddIssueLabels(this, request);
    if (mode === "check") return planAddIssueLabels(number, labels);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const issue = this.#issues.get(number);
    if (!issue) throw new NotFoundError(`issue #${number} is not in the fake`);
    for (const label of labels) {
      if (!this.#repositoryLabels.has(labelKey(label))) {
        throw new NotFoundError(`repository label ${label} is not in the fake`);
      }
      if (!issue.labels.some((name) => labelKey(name) === labelKey(label))) {
        issue.labels.push(label);
      }
    }
    this.labelWrites.push({ number, add: [...labels], remove: null });
    return { kind: "add-issue-labels", number, labels: [...labels], applied: true };
  }

  removeIssueLabel(request) {
    const { mode, number, label } = prepareRemoveIssueLabel(this, request);
    if (mode === "check") return planRemoveIssueLabel(number, label);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const issue = this.#issues.get(number);
    if (!issue) throw new NotFoundError(`issue #${number} is not in the fake`);
    const key = labelKey(label);
    const index = issue.labels.findIndex((name) => labelKey(name) === key);
    if (index === -1) throw new NotFoundError(`issue #${number} does not have label ${label}`);
    issue.labels.splice(index, 1);
    this.labelWrites.push({ number, add: null, remove: label });
    return { kind: "remove-issue-label", number, label, applied: true };
  }

  getIssueDependencies(number) {
    const expected = assertIssueNumber(number);
    this.reads.push({ kind: "get-issue-dependencies", number: expected });
    const issue = this.#issues.get(expected);
    if (!issue) throw new NotFoundError(`issue #${expected} is not in the fake`);
    return issue.dependencies
      .map((blockingNumber) => ({
        id: blockingNumber * 1000,
        number: blockingNumber,
        owner: this.owner,
        repo: this.repo,
      }))
      .sort((left, right) => left.number - right.number);
  }

  addIssueDependency(request) {
    const { mode, number, blockingNumber, blockingId } = prepareAddIssueDependency(this, request);
    if (mode === "check") return planAddIssueDependency(number, blockingNumber, blockingId);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const issue = this.#issues.get(number);
    if (!issue) throw new NotFoundError(`issue #${number} is not in the fake`);
    if (!this.#issues.has(blockingNumber)) {
      throw new NotFoundError(`blocking issue #${blockingNumber} is not in the fake`);
    }
    if (!issue.dependencies.includes(blockingNumber)) issue.dependencies.push(blockingNumber);
    issue.dependencies.sort((left, right) => left - right);
    this.dependencyWrites.push({ kind: "add", number, blockingNumber, blockingId });
    return {
      kind: "add-issue-dependency",
      number,
      blockingNumber,
      blockingId,
      applied: true,
    };
  }

  removeIssueDependency(request) {
    const { mode, number, blockingNumber, blockingId } = prepareRemoveIssueDependency(
      this,
      request,
    );
    if (mode === "check") {
      return planRemoveIssueDependency(number, blockingNumber, blockingId);
    }
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const issue = this.#issues.get(number);
    if (!issue) throw new NotFoundError(`issue #${number} is not in the fake`);
    const index = issue.dependencies.indexOf(blockingNumber);
    if (index === -1) {
      throw new NotFoundError(`issue #${number} is not blocked by #${blockingNumber}`);
    }
    issue.dependencies.splice(index, 1);
    this.dependencyWrites.push({ kind: "remove", number, blockingNumber, blockingId });
    return {
      kind: "remove-issue-dependency",
      number,
      blockingNumber,
      blockingId,
      applied: true,
    };
  }

  addIssueSubIssue(request) {
    const { mode, parentIssueNumber, subIssueNumber } = prepareAddIssueSubIssue(this, request);
    if (mode === "check") return planAddIssueSubIssue(parentIssueNumber, subIssueNumber);
    const parentSnapshot = this.getIssueProjectState(parentIssueNumber);
    const subIssueSnapshot = this.getIssueProjectState(subIssueNumber);
    const state = classifyIssueSubIssueState(
      parentSnapshot,
      subIssueSnapshot,
      parentIssueNumber,
      subIssueNumber,
      { owner: this.owner, repo: this.repo },
    );
    if (state === "unchanged") {
      return {
        kind: "add-issue-sub-issue",
        parentIssueNumber,
        subIssueNumber,
        applied: true,
        unchanged: true,
      };
    }
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const parent = this.#issues.get(parentIssueNumber);
    const subIssue = this.#issues.get(subIssueNumber);
    subIssue.parent = parentIssueNumber;
    parent.subIssues.push(subIssueNumber);
    parent.subIssues.sort((left, right) => left - right);
    this.subIssueWrites.push({ parentIssueNumber, subIssueNumber });
    return {
      kind: "add-issue-sub-issue",
      parentIssueNumber,
      subIssueNumber,
      applied: true,
    };
  }

  setAiResultLabel(request) {
    const prepared = prepareSetAiResultLabel(this, request);
    if (prepared.mode === "check") return planSetAiResultLabel(prepared.number, prepared.label);
    this.#beginApply();
    if (!this.permissions.issues) throw new PermissionDeniedError("issues permission denied");
    const issue = this.#issues.get(prepared.number);
    if (!issue) throw new NotFoundError(`issue #${prepared.number} is not in the fake`);
    const previous = selectAiResultLabel(issue.labels, prepared.number);
    if (previous === prepared.label) {
      return {
        kind: "set-ai-result-label",
        number: prepared.number,
        label: prepared.label,
        previous,
        applied: true,
        unchanged: true,
      };
    }
    const next = issue.labels.filter((name) => !AI_OWNED_LABELS.includes(name));
    next.push(prepared.label);
    issue.labels = next;
    this.labelWrites.push({
      number: prepared.number,
      add: prepared.label,
      remove: previous,
    });
    return {
      kind: "set-ai-result-label",
      number: prepared.number,
      label: prepared.label,
      previous,
      applied: true,
    };
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
      milestoneCloses: this.milestoneCloses.map((row) => ({ title: row.title })),
      repositoryMilestones: this.getRepositoryMilestones(),
      repositoryMilestoneWrites: this.repositoryMilestoneWrites.map((row) => ({ ...row })),
      dependencyWrites: this.dependencyWrites.map((row) => ({ ...row })),
      subIssueWrites: this.subIssueWrites.map((row) => ({ ...row })),
      labelWrites: this.labelWrites.map((row) => ({
        number: row.number,
        add: row.add,
        remove: row.remove,
      })),
      repositoryLabels: [...this.#repositoryLabels.values()]
        .map(cloneRepositoryLabel)
        .sort((left, right) => left.name.localeCompare(right.name)),
      repositoryLabelWrites: this.repositoryLabelWrites.map((row) => ({
        kind: row.kind,
        ...(row.previous == null ? {} : { previous: row.previous }),
        label: cloneRepositoryLabel(row.label),
      })),
      workflowDispatches: this.workflowDispatches.map((row) => ({
        workflow: row.workflow,
        uses: row.uses,
        dry_run: row.dry_run,
      })),
      merges: this.#merges.map((row) => {
        const merge = { number: row.number, merge_method: row.merge_method };
        if (row.commit_title != null) merge.commit_title = row.commit_title;
        return merge;
      }),
    };
  }

  getProject(number = PROJECT_NUMBER) {
    if (number !== PROJECT_NUMBER) {
      throw new GitHubAdapterError(`scheduling overlay uses GitHub Project ${PROJECT_NUMBER} only`);
    }
    if (this.#projectMissing) {
      throw new MissingProjectIdentityError(`GitHub Project ${PROJECT_NUMBER} is missing`);
    }
    if (!this.permissions.projectRead) {
      throw new PermissionDeniedError("project read permission denied");
    }
    return {
      id: "fake-project-3",
      number: PROJECT_NUMBER,
      viewerCanUpdate: this.permissions.projects,
    };
  }

  getProjectStatusField(number = PROJECT_NUMBER) {
    const project = this.getProject(number);
    return {
      ...project,
      fieldId: "fake-status-field",
      options: new Map([
        ["Todo", "fake-todo"],
        ["In Progress", "fake-in-progress"],
        ["Done", "fake-done"],
      ]),
    };
  }

  getProjectItems(number = PROJECT_NUMBER) {
    if (number !== PROJECT_NUMBER) {
      throw new GitHubAdapterError(`scheduling overlay uses GitHub Project ${PROJECT_NUMBER} only`);
    }
    return [...this.#projectItems].sort((left, right) => left - right);
  }

  getProjectStatus(issueNumber) {
    const number = assertIssueNumber(issueNumber);
    return this.#projectStatuses.get(number) ?? null;
  }

  getIssueProjectState(issueNumber) {
    const number = assertIssueNumber(issueNumber);
    this.reads.push({ kind: "get-issue-project-state", number });
    this.getProject(PROJECT_NUMBER);
    const issue = this.#issues.get(number);
    if (!issue) throw new NotFoundError(`issue #${number} is missing`);
    const itemFor = (candidate) =>
      this.#projectItems.has(candidate)
        ? {
            id: `fake-project-item-${candidate}`,
            status: this.#projectStatuses.get(candidate) ?? null,
            optionId: null,
          }
        : null;
    return {
      id: `fake-issue-${number}`,
      number,
      owner: this.owner,
      repo: this.repo,
      item: itemFor(number),
      parent:
        issue.parent == null
          ? null
          : {
              id: `fake-issue-${issue.parent}`,
              number: issue.parent,
              owner: this.owner,
              repo: this.repo,
            },
      subIssues: issue.subIssues.map((subIssue) => ({
        id: `fake-issue-${subIssue}`,
        number: subIssue,
        owner: this.owner,
        repo: this.repo,
        item: itemFor(subIssue),
      })),
    };
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
      item_id: `fake-project-item-${issueNumber}`,
    };
  }

  setIssueProjectStatus(request) {
    const { mode, issueNumber, status } = prepareSetIssueProjectStatus(this, request);
    this.getProjectStatusField(PROJECT_NUMBER);
    const snapshot = this.getIssueProjectState(issueNumber);
    if (!snapshot.item) {
      throw new MissingProjectIdentityError(
        `issue #${issueNumber} Project ${PROJECT_NUMBER} item is missing`,
      );
    }
    if (mode === "check") {
      return planSetIssueProjectStatus(issueNumber, status, snapshot.item?.status ?? null, false);
    }
    this.#beginApply();
    if (!this.permissions.projects) throw new PermissionDeniedError("projects permission denied");
    const current = this.#projectStatuses.get(issueNumber) ?? null;
    const unchanged = current === status;
    this.#projectStatuses.set(issueNumber, status);
    if (!unchanged) this.projectStatusWrites.push({ issueNumber, status, current });
    return {
      kind: "set-project-status",
      number: PROJECT_NUMBER,
      issueNumber,
      status,
      current,
      added: false,
      unchanged,
      applied: true,
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

  closeMilestone(request) {
    const { mode, title } = prepareCloseMilestone(this, request);
    if (mode === "check") return planCloseMilestone(title);
    this.#beginApply();
    if (!this.#milestones.has(title)) throw new NotFoundError(`milestone ${title} is missing`);
    this.#milestones.get(title).state = "closed";
    this.milestoneCloses.push({ title });
    return { kind: "close-milestone", title, applied: true };
  }

  listMilestoneIssues(title) {
    assertRequiredText(title, "milestone title");
    this.reads.push({ kind: "list-milestone-issues", title });
    if (!this.#milestones.has(title)) throw new NotFoundError(`milestone ${title} is missing`);
    return [...this.#issues.values()]
      .filter((issue) => issue.milestone === title)
      .map((issue) => ({
        number: issue.number,
        title: issue.title,
        state: issue.state === "closed" ? "closed" : "open",
        milestone: title,
      }))
      .sort((left, right) => left.number - right.number);
  }

  dispatchReleaseRehearsal(request = {}) {
    const { mode } = prepareDispatchReleaseRehearsal(this, request);
    if (mode === "check") return planDispatchReleaseRehearsal();
    this.#beginApply();
    this.workflowDispatches.push({
      workflow: "release-check.yml",
      uses: "release.yml",
      dry_run: true,
    });
    return {
      kind: "dispatch-release-check",
      workflow: "release-check.yml",
      applied: true,
    };
  }
}
