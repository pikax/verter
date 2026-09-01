import { spawnSync } from "node:child_process";

import {
  AmbiguousAiLabelError,
  ClosingLinkError,
  DoctorRequiredError,
  DuplicateError,
  GitHubAdapterError,
  IgnoredIssueError,
  InvalidIssueNumberError,
  LiveGitHubForbiddenInTestsError,
  MappingMismatchError,
  MissingProjectIdentityError,
  MutationModeRequiredError,
  NotFoundError,
  PartialFailureError,
  PermissionDeniedError,
  ProtectedMappingError,
  UnstructuredGitHubOutputError,
  UnsupportedVerdictError,
  WrongRepositoryError,
} from "./errors.mjs";

export const PROJECT_NUMBER = 3;
export const PROJECT_VIEWS = Object.freeze([
  "execution",
  "READY",
  "triage",
  "review/gate",
  "train",
  "milestone",
  "roadmap",
]);

const PROJECT_LOOKUP_QUERY =
  "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){owner{... on Organization{projectV2(number:$number){id number viewerCanUpdate}}... on User{projectV2(number:$number){id number viewerCanUpdate}}}}}";
const ISSUE_ID_QUERY =
  "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){issue(number:$number){id projectItems(first:100){totalCount nodes{id project{id number}}}}}}";
const ADD_ITEM_MUTATION =
  "mutation($projectId:ID!,$contentId:ID!){addProjectV2ItemById(input:{projectId:$projectId,contentId:$contentId}){item{id}}}";
const PROJECT_STATUS_FIELD_QUERY =
  "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){owner{... on Organization{projectV2(number:$number){id number viewerCanUpdate fields(first:100){totalCount nodes{... on ProjectV2SingleSelectField{id name options{id name}}}}}}... on User{projectV2(number:$number){id number viewerCanUpdate fields(first:100){totalCount nodes{... on ProjectV2SingleSelectField{id name options{id name}}}}}}}}}";
const ISSUE_PROJECT_STATE_QUERY =
  'query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){issue(number:$number){id number parent{id number repository{name owner{login}}} projectItems(first:100){totalCount nodes{id project{id number} fieldValueByName(name:"Status"){... on ProjectV2ItemFieldSingleSelectValue{name optionId}}}} subIssues(first:100){totalCount nodes{id number repository{name owner{login}} projectItems(first:100){totalCount nodes{id project{id number} fieldValueByName(name:"Status"){... on ProjectV2ItemFieldSingleSelectValue{name optionId}}}}}}}}}';
const ADD_SUB_ISSUE_MUTATION =
  "mutation($issueId:ID!,$subIssueId:ID!,$replaceParent:Boolean!){addSubIssue(input:{issueId:$issueId,subIssueId:$subIssueId,replaceParent:$replaceParent}){issue{id number repository{name owner{login}}} subIssue{id number repository{name owner{login}} parent{id number repository{name owner{login}}}}}}";
const SET_PROJECT_STATUS_MUTATION =
  "mutation($projectId:ID!,$itemId:ID!,$fieldId:ID!,$optionId:String!){updateProjectV2ItemFieldValue(input:{projectId:$projectId,itemId:$itemId,fieldId:$fieldId,value:{singleSelectOptionId:$optionId}}){projectV2Item{id}}}";

const MINTED_CLEARANCES = new WeakMap();
const OWNER_REPO = /^[A-Za-z0-9_.-]+$/;

export const AI_ISSUE_VERDICTS = Object.freeze([
  "unchecked",
  "confirmed",
  "rejected",
  "fixed",
  "needs-human",
]);
export const AI_OWNED_LABELS = Object.freeze(AI_ISSUE_VERDICTS.map((verdict) => `ai:${verdict}`));
export const MAINTAINER_IGNORE_LABEL = "ai:ignore";

export function assertAiIssueVerdict(value) {
  if (typeof value !== "string" || !AI_ISSUE_VERDICTS.includes(value)) {
    throw new UnsupportedVerdictError(`unsupported AiIssueVerdict ${value}`);
  }
  return value;
}

export function aiOwnedLabel(verdict) {
  return `ai:${assertAiIssueVerdict(verdict)}`;
}

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

export function capabilityRecord({
  authenticated,
  login,
  repository,
  issues,
  pullRequests,
  projects,
  actions,
}) {
  const record = {
    authenticated: authenticated === true,
    repository: repository ?? null,
    issues: issues === true,
    pullRequests: pullRequests === true,
    projects: projects === true,
    actions: actions === true,
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

export const RELEASE_SUBJECT_PATTERN = /^release: v.+$/u;
const RELEASE_PR_SUFFIX = /\(#\d+\)/u;
export const RELEASE_VERSION_PATTERN = /^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$/u;
const GH_NAME = String.raw`[A-Za-z0-9_.-]+`;
const ISSUE_DIGITS = String.raw`[0-9]+`;
const CLOSING_REFERENCE = new RegExp(
  String.raw`\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s+(?:#${ISSUE_DIGITS}|${GH_NAME}/${GH_NAME}#${ISSUE_DIGITS}|https://github\.com/${GH_NAME}/${GH_NAME}/issues/${ISSUE_DIGITS})\b`,
  "giu",
);

export function findClosingReferences(text) {
  if (typeof text !== "string" || text.length === 0) return [];
  const pattern = new RegExp(CLOSING_REFERENCE.source, CLOSING_REFERENCE.flags);
  return Array.from(text.matchAll(pattern), (row) => row[0]);
}

export function assertReleasePullRequestTitle(title) {
  assertRequiredText(title, "pull request title");
  if (!RELEASE_SUBJECT_PATTERN.test(title) || RELEASE_PR_SUFFIX.test(title)) {
    throw new GitHubAdapterError("release title must match /^release: v.+$/ without a PR suffix");
  }
  return title;
}

export function releasePullRequestTitle(version) {
  assertRequiredText(version, "release version");
  if (!RELEASE_VERSION_PATTERN.test(version)) {
    throw new GitHubAdapterError("release title must match /^release: v.+$/ without a PR suffix");
  }
  return assertReleasePullRequestTitle(`release: v${version}`);
}

export function assertNoClosingLink(body) {
  if (typeof body !== "string") {
    throw new GitHubAdapterError("pull request body must be a string");
  }
  if (findClosingReferences(body).length > 0) {
    throw new ClosingLinkError("release pull request body must not contain a closing link");
  }
  return body;
}

export function planCreateReleasePullRequest(request) {
  return {
    kind: "create-release-pull-request",
    title: request.title,
    body: request.body,
    head: request.head,
    base: request.base,
    closes: null,
    applied: false,
  };
}

export function prepareCreateReleasePullRequest(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  if (request.mappedIssue != null) {
    throw new GitHubAdapterError("release pull request must not name a mapped issue");
  }
  assertReleasePullRequestTitle(request.title);
  assertRequiredText(request.head, "pull request head");
  assertRequiredText(request.base, "pull request base");
  assertNoClosingLink(request.body);
  assertApplyClearance(mode, request.clearance, "pullRequests", adapter);
  return { mode };
}

export function planCloseMilestone(title) {
  return { kind: "close-milestone", title, applied: false };
}

export function prepareCloseMilestone(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  assertRequiredText(request.title, "milestone title");
  assertApplyClearance(mode, request.clearance, "pullRequests", adapter);
  return { mode, title: request.title };
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

function parsePullHead(head) {
  if (typeof head === "string" && head.length > 0) return head;
  if (
    head !== null &&
    typeof head === "object" &&
    !Array.isArray(head) &&
    typeof head.ref === "string" &&
    head.ref.length > 0
  ) {
    return head.ref;
  }
  throw new UnstructuredGitHubOutputError("GitHub pull request head is missing a ref");
}

function parsePullsPayload(payload) {
  if (!Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub pull request list is not a JSON array");
  }
  return payload.map((row) => {
    if (row === null || typeof row !== "object" || Array.isArray(row)) {
      throw new UnstructuredGitHubOutputError(
        "GitHub pull request list entry is not a JSON object",
      );
    }
    return {
      number: parseGitHubResourceNumber(row),
      head: parsePullHead(row.head),
    };
  });
}

export function parseLabelsPayload(payload) {
  if (!Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub labels response is not a JSON array");
  }
  return payload.map((row, index) => {
    if (row === null || typeof row !== "object" || Array.isArray(row)) {
      throw new UnstructuredGitHubOutputError(`GitHub label ${index} is not a JSON object`);
    }
    if (typeof row.name !== "string" || row.name.length === 0) {
      throw new UnstructuredGitHubOutputError("GitHub label name is not a string");
    }
    return row.name;
  });
}

function parseRepositoryLabel(row, index = 0) {
  if (row === null || typeof row !== "object" || Array.isArray(row)) {
    throw new UnstructuredGitHubOutputError(`GitHub label ${index} is not a JSON object`);
  }
  if (typeof row.name !== "string" || row.name.length === 0) {
    throw new UnstructuredGitHubOutputError("GitHub label name is not a string");
  }
  if (typeof row.color !== "string" || !/^[0-9a-f]{6}$/iu.test(row.color)) {
    throw new UnstructuredGitHubOutputError(`GitHub label ${row.name} color is invalid`);
  }
  const description = row.description == null ? "" : row.description;
  if (typeof description !== "string") {
    throw new UnstructuredGitHubOutputError(`GitHub label ${row.name} description is invalid`);
  }
  return { name: row.name, color: row.color.toLowerCase(), description };
}

export function parseRepositoryLabelsPayload(payload) {
  if (!Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub repository labels response is not an array");
  }
  return payload.map(parseRepositoryLabel);
}

function assertLabelDefinition(label) {
  if (label === null || typeof label !== "object" || Array.isArray(label)) {
    throw new GitHubAdapterError("repository label definition is required");
  }
  assertRequiredText(label.name, "repository label name");
  if (typeof label.color !== "string" || !/^[0-9a-f]{6}$/iu.test(label.color)) {
    throw new GitHubAdapterError("repository label color must be six hexadecimal characters");
  }
  if (typeof label.description !== "string" || label.description.length > 100) {
    throw new GitHubAdapterError("repository label description must be at most 100 characters");
  }
  return {
    name: label.name,
    color: label.color.toLowerCase(),
    description: label.description,
  };
}

export function planCreateRepositoryLabel(label) {
  return { kind: "create-repository-label", label, applied: false };
}

export function prepareCreateRepositoryLabel(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const label = assertLabelDefinition(request.label);
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, label };
}

export function planUpdateRepositoryLabel(existing, label) {
  return { kind: "update-repository-label", existing, label, applied: false };
}

function assertMilestoneDefinition(milestone) {
  if (milestone === null || typeof milestone !== "object" || Array.isArray(milestone)) {
    throw new GitHubAdapterError("repository milestone definition is required");
  }
  assertRequiredText(milestone.title, "repository milestone title");
  assertRequiredText(milestone.description, "repository milestone description");
  return { title: milestone.title, description: milestone.description };
}

export function planCreateRepositoryMilestone(milestone) {
  return { kind: "create-repository-milestone", milestone, applied: false };
}

export function prepareCreateRepositoryMilestone(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const milestone = assertMilestoneDefinition(request.milestone);
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, milestone };
}

export function planUpdateRepositoryMilestone(existing, milestone) {
  return { kind: "update-repository-milestone", existing, milestone, applied: false };
}

export function prepareUpdateRepositoryMilestone(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const existing = request.existing;
  if (existing === null || typeof existing !== "object" || Array.isArray(existing)) {
    throw new GitHubAdapterError("existing repository milestone is required");
  }
  const number = assertIssueNumber(existing.number, "milestone number");
  const milestone = assertMilestoneDefinition(request.milestone);
  assertRequiredText(existing.title, "existing milestone title");
  if (existing.title !== milestone.title) {
    throw new GitHubAdapterError("repository milestone title is immutable catalog identity");
  }
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, number, milestone, patch: { description: milestone.description } };
}

export function prepareUpdateRepositoryLabel(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const existing = requiredLabelName(request.existing, "existing repository label name");
  const label = assertLabelDefinition(request.label);
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, existing, label };
}

function requiredLabelName(value, description = "label name") {
  assertRequiredText(value, description);
  return value;
}

function prepareIssueLabelMutation(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const number = assertIssueNumber(request.number);
  if (mappingPolicy(request.mapping, number) === "protected") {
    throw new ProtectedMappingError(
      `refusing label update of protected issue #${number} (${request.mapping.node_id})`,
    );
  }
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, number };
}

export function prepareAddIssueLabels(adapter, request) {
  const prepared = prepareIssueLabelMutation(adapter, request);
  if (!Array.isArray(request.labels) || request.labels.length === 0) {
    throw new GitHubAdapterError("issue labels to add must be a non-empty array");
  }
  const labels = request.labels.map((label) => requiredLabelName(label));
  return { ...prepared, labels };
}

export function planAddIssueLabels(number, labels) {
  return { kind: "add-issue-labels", number, labels, applied: false };
}

export function prepareRemoveIssueLabel(adapter, request) {
  return {
    ...prepareIssueLabelMutation(adapter, request),
    label: requiredLabelName(request.label),
  };
}

export function planRemoveIssueLabel(number, label) {
  return { kind: "remove-issue-label", number, label, applied: false };
}

export function planSetAiResultLabel(number, label) {
  return { kind: "set-ai-result-label", number, label, applied: false };
}

export function prepareSetAiResultLabel(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const number = assertIssueNumber(request.number);
  const label = aiOwnedLabel(request.verdict);
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, number, label };
}

export function selectAiResultLabel(names, number) {
  if (names.includes(MAINTAINER_IGNORE_LABEL)) {
    throw new IgnoredIssueError(`issue #${number} is labeled ${MAINTAINER_IGNORE_LABEL}`);
  }
  const currentAi = names.filter((name) => AI_OWNED_LABELS.includes(name));
  if (currentAi.length > 1) {
    throw new AmbiguousAiLabelError(`issue #${number} has multiple AI-result labels`);
  }
  return currentAi[0] ?? null;
}

export function parseIssuePayload(payload, expectedNumber) {
  const number = parseGitHubResourceNumber(payload);
  if (number !== expectedNumber) {
    throw new UnstructuredGitHubOutputError(
      `GitHub issue read returned number ${number}, expected ${expectedNumber}`,
    );
  }
  if (payload.pull_request != null) {
    throw new UnstructuredGitHubOutputError(`mapped issue #${number} cannot be read unambiguously`);
  }
  if (typeof payload.title !== "string") {
    throw new UnstructuredGitHubOutputError("GitHub issue title is not a string");
  }
  const body = payload.body == null ? "" : payload.body;
  if (typeof body !== "string") {
    throw new UnstructuredGitHubOutputError("GitHub issue body is not a string");
  }
  const result = { number, title: payload.title, body };
  if (payload.state != null) {
    if (payload.state !== "open" && payload.state !== "closed") {
      throw new UnstructuredGitHubOutputError(`GitHub issue #${number} state is invalid`);
    }
    result.state = payload.state;
  }
  const milestone = payload.milestone;
  if (typeof milestone === "string" && milestone.length > 0) {
    result.milestone = milestone;
  } else if (milestone && typeof milestone === "object" && typeof milestone.title === "string") {
    result.milestone = milestone.title;
  }
  return result;
}

export function parseMilestoneListPayload(payload) {
  if (!Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub milestone list is not a JSON array");
  }
  return payload.map((row, index) => {
    if (row === null || typeof row !== "object" || Array.isArray(row)) {
      throw new UnstructuredGitHubOutputError(`GitHub milestone ${index} is not a JSON object`);
    }
    const number = parseGitHubResourceNumber(row);
    if (typeof row.title !== "string" || row.title.length === 0) {
      throw new UnstructuredGitHubOutputError("GitHub milestone title is not a string");
    }
    const description = row.description == null ? "" : row.description;
    if (typeof description !== "string") {
      throw new UnstructuredGitHubOutputError(
        `GitHub milestone ${row.title} description is invalid`,
      );
    }
    if (row.state != null && row.state !== "open" && row.state !== "closed") {
      throw new UnstructuredGitHubOutputError(`GitHub milestone ${row.title} state is invalid`);
    }
    return {
      number,
      title: row.title,
      description,
      ...(row.state == null ? {} : { state: row.state }),
      ...(row.due_on == null ? {} : { due_on: row.due_on }),
    };
  });
}

export function parseIssueDependenciesPayload(payload) {
  if (!Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub issue dependency list is not an array");
  }
  return payload.map((row, index) => {
    if (row === null || typeof row !== "object" || Array.isArray(row)) {
      throw new UnstructuredGitHubOutputError(`GitHub issue dependency ${index} is not an object`);
    }
    if (row.pull_request != null) {
      throw new UnstructuredGitHubOutputError("GitHub issue dependency cannot be a pull request");
    }
    const number = parseGitHubResourceNumber(row);
    if (!Number.isSafeInteger(row.id) || row.id < 1) {
      throw new UnstructuredGitHubOutputError(
        `GitHub issue dependency #${number} is missing its database id`,
      );
    }
    let repository;
    try {
      repository = new URL(row.repository_url);
    } catch {
      throw new UnstructuredGitHubOutputError(
        `GitHub issue dependency #${number} repository is missing`,
      );
    }
    const match = /^\/repos\/([^/]+)\/([^/]+)\/?$/u.exec(repository.pathname);
    if (!match) {
      throw new UnstructuredGitHubOutputError(
        `GitHub issue dependency #${number} repository is invalid`,
      );
    }
    return { id: row.id, number, owner: match[1], repo: match[2] };
  });
}

export function parseMilestoneIssuesPayload(payload, milestoneTitle) {
  if (!Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub milestone issue list is not a JSON array");
  }
  assertRequiredText(milestoneTitle, "milestone title");
  const issues = [];
  for (const row of payload) {
    if (row === null || typeof row !== "object" || Array.isArray(row)) {
      throw new UnstructuredGitHubOutputError("GitHub milestone issue is not a JSON object");
    }
    if (row.pull_request != null) continue;
    const number = parseGitHubResourceNumber(row);
    if (typeof row.title !== "string") {
      throw new UnstructuredGitHubOutputError("GitHub issue title is not a string");
    }
    if (row.state !== "open" && row.state !== "closed") {
      throw new UnstructuredGitHubOutputError("GitHub issue state is not open or closed");
    }
    issues.push({
      number,
      title: row.title,
      state: row.state,
      milestone: milestoneTitle,
    });
  }
  return issues;
}

export function planDispatchReleaseRehearsal() {
  return {
    kind: "dispatch-release-check",
    workflow: "release-check.yml",
    applied: false,
  };
}

export function prepareDispatchReleaseRehearsal(adapter, request = {}) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  assertApplyClearance(mode, request.clearance, "actions", adapter);
  const ref = typeof request.ref === "string" && request.ref.length > 0 ? request.ref : "main";
  return { mode, ref };
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
  const hasNodeId = typeof mapping.node_id === "string" && mapping.node_id.length > 0;
  const hasTrain = typeof mapping.train === "string" && mapping.train.length > 0;
  if (!hasNodeId && !hasTrain) {
    throw new MappingMismatchError("mapping.node_id or mapping.train is required");
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

function mappingIdentity(mapping) {
  if (typeof mapping?.node_id === "string" && mapping.node_id.length > 0) {
    return mapping.node_id;
  }
  if (typeof mapping?.train === "string" && mapping.train.length > 0) return mapping.train;
  return "unknown mapping";
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

export function assertProjectNumber(number) {
  if (number !== PROJECT_NUMBER) {
    throw new GitHubAdapterError(`scheduling overlay uses GitHub Project ${PROJECT_NUMBER} only`);
  }
  return number;
}

export function parseGraphqlResult(payload, createError) {
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub GraphQL response is not a JSON object");
  }
  const fail = () => {
    const error =
      typeof createError === "function"
        ? createError(payload)
        : new GitHubAdapterError("GitHub GraphQL request failed");
    throw error;
  };
  if (Array.isArray(payload.errors) && payload.errors.length > 0) fail();
  const data = payload.data;
  if (data === null || typeof data !== "object" || Array.isArray(data)) fail();
  return data;
}

function missingProjectError(number) {
  return () => new MissingProjectIdentityError(`GitHub Project ${number} is missing`);
}

function projectFromGraphqlData(data, number) {
  const project =
    data.repository?.owner?.projectV2 ??
    data.organization?.projectV2 ??
    data.user?.projectV2 ??
    null;
  if (
    project === null ||
    typeof project !== "object" ||
    Array.isArray(project) ||
    project.number !== number ||
    typeof project.id !== "string" ||
    project.id.length === 0
  ) {
    throw new MissingProjectIdentityError(`GitHub Project ${number} is missing`);
  }
  return { id: project.id, number, viewerCanUpdate: project.viewerCanUpdate === true };
}

function projectStatusFieldFromGraphqlData(data, number) {
  const project =
    data.repository?.owner?.projectV2 ??
    data.organization?.projectV2 ??
    data.user?.projectV2 ??
    null;
  const base = projectFromGraphqlData(data, number);
  const fieldConnection = project?.fields;
  const fields = fieldConnection?.nodes;
  if (
    !Array.isArray(fields) ||
    !Number.isSafeInteger(fieldConnection?.totalCount) ||
    fieldConnection.totalCount !== fields.length
  ) {
    throw new MissingProjectIdentityError(`GitHub Project ${number} Status field is missing`);
  }
  const matches = fields.filter(
    (row) => row && typeof row === "object" && !Array.isArray(row) && row.name === "Status",
  );
  if (matches.length !== 1 || typeof matches[0].id !== "string") {
    throw new MissingProjectIdentityError(`GitHub Project ${number} Status field is missing`);
  }
  const options = matches[0].options;
  if (!Array.isArray(options)) {
    throw new MissingProjectIdentityError(`GitHub Project ${number} Status options are missing`);
  }
  const byName = new Map();
  for (const row of options) {
    if (
      !row ||
      typeof row !== "object" ||
      Array.isArray(row) ||
      typeof row.id !== "string" ||
      typeof row.name !== "string"
    ) {
      throw new UnstructuredGitHubOutputError("GitHub Project Status option is invalid");
    }
    if (byName.has(row.name)) {
      throw new UnstructuredGitHubOutputError(
        `GitHub Project Status option ${row.name} is duplicate`,
      );
    }
    byName.set(row.name, row.id);
  }
  for (const status of PROJECT_STATUSES) {
    if (!byName.has(status)) {
      throw new MissingProjectIdentityError(
        `GitHub Project ${number} Status option ${status} is missing`,
      );
    }
  }
  return { ...base, fieldId: matches[0].id, options: byName };
}

function projectItemFromConnection(connection, issueNumber, projectId) {
  const nodes = connection?.nodes;
  if (
    !Array.isArray(nodes) ||
    !Number.isSafeInteger(connection?.totalCount) ||
    connection.totalCount !== nodes.length
  ) {
    throw new UnstructuredGitHubOutputError(`issue #${issueNumber} project items are missing`);
  }
  const matches = nodes.filter((row) => row?.project?.id === projectId);
  if (matches.length > 1) {
    throw new UnstructuredGitHubOutputError(
      `issue #${issueNumber} has duplicate Project ${PROJECT_NUMBER} items`,
    );
  }
  if (matches.length === 0) return null;
  const row = matches[0];
  if (typeof row.id !== "string" || row.id.length === 0) {
    throw new UnstructuredGitHubOutputError(`issue #${issueNumber} project item id is missing`);
  }
  const value = row.fieldValueByName;
  if (value == null) return { id: row.id, status: null, optionId: null };
  if (
    typeof value !== "object" ||
    Array.isArray(value) ||
    typeof value.name !== "string" ||
    typeof value.optionId !== "string"
  ) {
    throw new UnstructuredGitHubOutputError(`issue #${issueNumber} Project Status is invalid`);
  }
  return { id: row.id, status: value.name, optionId: value.optionId };
}

function issueRelationIdentity(row, label) {
  const number = assertIssueNumber(row?.number, `${label} number`);
  if (typeof row?.id !== "string" || row.id.length === 0) {
    throw new UnstructuredGitHubOutputError(`${label} #${number} id is missing`);
  }
  const owner = row.repository?.owner?.login;
  const repo = row.repository?.name;
  if (typeof owner !== "string" || !owner || typeof repo !== "string" || !repo) {
    throw new UnstructuredGitHubOutputError(`${label} #${number} repository is missing`);
  }
  return { id: row.id, number, owner, repo };
}

export function parseIssueProjectState(payload, issueNumber, projectId, repository) {
  assertRequiredText(projectId, "Project id");
  const data = parseGraphqlResult(
    payload,
    () => new NotFoundError(`issue #${issueNumber} is missing`),
  );
  const issue = data.repository?.issue;
  if (!issue || issue.number !== issueNumber || typeof issue.id !== "string") {
    throw new NotFoundError(`issue #${issueNumber} is missing`);
  }
  const parent = issue.parent == null ? null : issueRelationIdentity(issue.parent, "parent issue");
  const connection = issue.subIssues;
  if (
    !connection ||
    !Array.isArray(connection.nodes) ||
    connection.totalCount !== connection.nodes.length
  ) {
    throw new UnstructuredGitHubOutputError(`issue #${issueNumber} sub-issue list is incomplete`);
  }
  const subIssues = connection.nodes.map((row) => {
    const identity = issueRelationIdentity(row, "sub-issue");
    return {
      ...identity,
      item: projectItemFromConnection(row.projectItems, identity.number, projectId),
    };
  });
  return {
    id: issue.id,
    number: issueNumber,
    ...(repository == null ? {} : { owner: repository.owner, repo: repository.repo }),
    item: projectItemFromConnection(issue.projectItems, issueNumber, projectId),
    parent,
    subIssues,
  };
}

export function parseGraphqlProject(payload, number = PROJECT_NUMBER) {
  return projectFromGraphqlData(parseGraphqlResult(payload, missingProjectError(number)), number);
}

function issueIdFromGraphql(data, issueNumber) {
  const contentId = data?.repository?.issue?.id;
  if (typeof contentId !== "string" || contentId.length === 0) {
    throw new NotFoundError(`issue #${issueNumber} is missing`);
  }
  return contentId;
}

function projectMembership(issue, project) {
  const connection = issue?.projectItems;
  const nodes = connection?.nodes;
  if (
    !Array.isArray(nodes) ||
    !Number.isSafeInteger(connection?.totalCount) ||
    connection.totalCount !== nodes.length
  ) {
    throw new UnstructuredGitHubOutputError("GitHub issue Project membership is incomplete");
  }
  return nodes.some(
    (row) =>
      row &&
      typeof row === "object" &&
      !Array.isArray(row) &&
      row.project &&
      typeof row.project === "object" &&
      !Array.isArray(row.project) &&
      row.project.id === project.id,
  );
}

export function planAddIssueToProject(issueNumber) {
  return {
    kind: "add-project-item",
    number: PROJECT_NUMBER,
    issueNumber,
    applied: false,
  };
}

export const PROJECT_STATUSES = Object.freeze(["Todo", "In Progress", "Done"]);

function requiredProjectStatus(value) {
  if (!PROJECT_STATUSES.includes(value)) {
    throw new GitHubAdapterError(`project status must be one of ${PROJECT_STATUSES.join(", ")}`);
  }
  return value;
}

export function planSetIssueProjectStatus(issueNumber, status, current = null, add = false) {
  return {
    kind: "set-project-status",
    number: PROJECT_NUMBER,
    issueNumber: assertIssueNumber(issueNumber),
    status: requiredProjectStatus(status),
    current,
    add,
    applied: false,
  };
}

export function prepareSetIssueProjectStatus(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const issueNumber = assertIssueNumber(request.issueNumber);
  const status = requiredProjectStatus(request.status);
  assertApplyClearance(mode, request.clearance, "projects", adapter);
  return { mode, issueNumber, status };
}

export function planSetIssueMilestone(issueNumber, title) {
  return { kind: "set-milestone", issueNumber, title, applied: false };
}

function prepareIssueDependencyMutation(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const number = assertIssueNumber(request.number);
  const blockingNumber = assertIssueNumber(request.blockingNumber, "blocking issue number");
  if (number === blockingNumber) {
    throw new GitHubAdapterError("an issue cannot block itself");
  }
  if (mappingPolicy(request.mapping, number) === "protected") {
    throw new ProtectedMappingError(`refusing dependency update of protected issue #${number}`);
  }
  if (mappingPolicy(request.blockingMapping, blockingNumber) === "protected") {
    throw new ProtectedMappingError(
      `refusing dependency update involving protected issue #${blockingNumber}`,
    );
  }
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, number, blockingNumber };
}

export function planAddIssueDependency(number, blockingNumber, blockingId) {
  return { kind: "add-issue-dependency", number, blockingNumber, blockingId, applied: false };
}

export function planAddIssueSubIssue(parentIssueNumber, subIssueNumber) {
  return {
    kind: "add-issue-sub-issue",
    parentIssueNumber,
    subIssueNumber,
    applied: false,
  };
}

export function prepareAddIssueSubIssue(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const parentIssueNumber = assertIssueNumber(request.parentIssueNumber, "parent issue number");
  const subIssueNumber = assertIssueNumber(request.subIssueNumber, "sub-issue number");
  if (parentIssueNumber === subIssueNumber) {
    throw new GitHubAdapterError("an issue cannot be its own sub-issue");
  }
  if (mappingPolicy(request.parentMapping, parentIssueNumber) === "protected") {
    throw new ProtectedMappingError(
      `refusing sub-issue update of protected parent issue #${parentIssueNumber} (${mappingIdentity(request.parentMapping)})`,
    );
  }
  if (mappingPolicy(request.subIssueMapping, subIssueNumber) === "protected") {
    throw new ProtectedMappingError(
      `refusing sub-issue update of protected issue #${subIssueNumber} (${mappingIdentity(request.subIssueMapping)})`,
    );
  }
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, parentIssueNumber, subIssueNumber };
}

function assertBoundIssueState(snapshot, expectedNumber, repository, label) {
  if (
    snapshot == null ||
    typeof snapshot !== "object" ||
    snapshot.number !== expectedNumber ||
    typeof snapshot.id !== "string" ||
    snapshot.id.length === 0
  ) {
    throw new UnstructuredGitHubOutputError(`${label} #${expectedNumber} identity is invalid`);
  }
  if (!sameRepository(snapshot, repository)) {
    throw new WrongRepositoryError(
      `${label} #${expectedNumber} is outside ${repository.owner}/${repository.repo}`,
    );
  }
  return snapshot;
}

function sameRepository(left, right) {
  return (
    typeof left?.owner === "string" &&
    typeof left?.repo === "string" &&
    typeof right?.owner === "string" &&
    typeof right?.repo === "string" &&
    left.owner.toLowerCase() === right.owner.toLowerCase() &&
    left.repo.toLowerCase() === right.repo.toLowerCase()
  );
}

function assertRelationRepository(relation, repository, label) {
  if (!sameRepository(relation, repository)) {
    throw new WrongRepositoryError(`${label} is outside ${repository.owner}/${repository.repo}`);
  }
}

export function classifyIssueSubIssueState(
  parentSnapshot,
  subIssueSnapshot,
  parentIssueNumber,
  subIssueNumber,
  repository,
) {
  const parent = assertBoundIssueState(
    parentSnapshot,
    parentIssueNumber,
    repository,
    "parent issue",
  );
  const subIssue = assertBoundIssueState(subIssueSnapshot, subIssueNumber, repository, "sub-issue");
  if (!Array.isArray(parent.subIssues)) {
    throw new UnstructuredGitHubOutputError(
      `parent issue #${parentIssueNumber} sub-issue list is invalid`,
    );
  }
  const listed = parent.subIssues.filter((row) => row?.number === subIssueNumber);
  for (const row of listed) {
    assertRelationRepository(row, repository, `sub-issue #${subIssueNumber}`);
    if (row.id !== subIssue.id) {
      throw new UnstructuredGitHubOutputError(
        `sub-issue #${subIssueNumber} node id does not match its issue identity`,
      );
    }
  }
  if (listed.length > 1) {
    throw new UnstructuredGitHubOutputError(
      `parent issue #${parentIssueNumber} contains duplicate sub-issue #${subIssueNumber}`,
    );
  }
  if (subIssue.parent == null) {
    if (listed.length !== 0) {
      throw new UnstructuredGitHubOutputError(
        `sub-issue #${subIssueNumber} has no parent but is listed by issue #${parentIssueNumber}`,
      );
    }
    return "attach";
  }
  assertRelationRepository(subIssue.parent, repository, `parent of sub-issue #${subIssueNumber}`);
  if (subIssue.parent.number !== parentIssueNumber || subIssue.parent.id !== parent.id) {
    throw new GitHubAdapterError(
      `refusing to replace parent of sub-issue #${subIssueNumber}; it already has parent issue #${subIssue.parent.number}`,
    );
  }
  if (listed.length !== 1) {
    throw new UnstructuredGitHubOutputError(
      `sub-issue #${subIssueNumber} names parent issue #${parentIssueNumber} but is absent from its sub-issue list`,
    );
  }
  return "unchanged";
}

function parseAddedIssueIdentity(row, expected, expectedId, repository, label) {
  if (row == null || typeof row !== "object" || Array.isArray(row)) {
    throw new UnstructuredGitHubOutputError(`${label} #${expected} identity is missing`);
  }
  if (!Number.isSafeInteger(row.number) || row.number < 1) {
    throw new UnstructuredGitHubOutputError(`${label} #${expected} number is invalid`);
  }
  if (typeof row.id !== "string" || row.id.length === 0) {
    throw new UnstructuredGitHubOutputError(`${label} #${expected} id is missing`);
  }
  const identity = {
    id: row.id,
    number: row.number,
    owner: row.repository?.owner?.login,
    repo: row.repository?.name,
  };
  if (
    identity.number !== expected ||
    identity.id !== expectedId ||
    !sameRepository(identity, repository)
  ) {
    throw new UnstructuredGitHubOutputError(`${label} #${expected} identity did not match`);
  }
  return identity;
}

export function parseAddIssueSubIssueResult(payload, parentSnapshot, subIssueSnapshot, repository) {
  const data = parseGraphqlResult(payload, () => new GitHubAdapterError("addSubIssue failed"));
  const result = data.addSubIssue;
  if (!result || typeof result !== "object" || Array.isArray(result)) {
    throw new UnstructuredGitHubOutputError("addSubIssue did not return issue identities");
  }
  const parent = parseAddedIssueIdentity(
    result.issue,
    parentSnapshot.number,
    parentSnapshot.id,
    repository,
    "parent issue",
  );
  const subIssue = parseAddedIssueIdentity(
    result.subIssue,
    subIssueSnapshot.number,
    subIssueSnapshot.id,
    repository,
    "sub-issue",
  );
  const returnedParent = parseAddedIssueIdentity(
    result.subIssue?.parent,
    parent.number,
    parent.id,
    repository,
    "returned sub-issue parent",
  );
  return { parent, subIssue: { ...subIssue, parent: returnedParent } };
}

export function prepareAddIssueDependency(adapter, request) {
  const prepared = prepareIssueDependencyMutation(adapter, request);
  const blockingId = assertIssueNumber(request.blockingId, "blocking issue database id");
  return { ...prepared, blockingId };
}

export function planRemoveIssueDependency(number, blockingNumber, blockingId) {
  return {
    kind: "remove-issue-dependency",
    number,
    blockingNumber,
    blockingId,
    applied: false,
  };
}

export function prepareRemoveIssueDependency(adapter, request) {
  const prepared = prepareIssueDependencyMutation(adapter, request);
  const blockingId = assertIssueNumber(request.blockingId, "blocking issue database id");
  return { ...prepared, blockingId };
}

export function prepareAddIssueToProject(adapter, request) {
  assertRepository(adapter, request);
  assertProjectNumber(request.number);
  const mode = assertMutationMode(request.mode);
  const issueNumber = assertIssueNumber(request.issueNumber);
  assertApplyClearance(mode, request.clearance, "projects", adapter);
  return { mode, issueNumber };
}

export function prepareSetIssueMilestone(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const issueNumber = assertIssueNumber(request.issueNumber);
  assertRequiredText(request.title, "milestone title");
  if (request.mapping != null && mappingPolicy(request.mapping, issueNumber) === "protected") {
    throw new ProtectedMappingError(`refusing milestone update of protected issue #${issueNumber}`);
  }
  assertApplyClearance(mode, request.clearance, "issues", adapter);
  return { mode, issueNumber, title: request.title };
}

export function prepareCreatePullRequest(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  assertRequiredText(request.title, "pull request title");
  if (RELEASE_SUBJECT_PATTERN.test(request.title)) {
    throw new GitHubAdapterError("release pull requests must use createReleasePullRequest");
  }
  const mappedIssue = assertIssueNumber(request.mappedIssue, "mapped issue");
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

export function parsePullRequestPayload(payload, expectedNumber) {
  const number = parseGitHubResourceNumber(payload);
  if (number !== expectedNumber) {
    throw new UnstructuredGitHubOutputError(
      `GitHub pull request read returned number ${number}, expected ${expectedNumber}`,
    );
  }
  if (typeof payload.title !== "string") {
    throw new UnstructuredGitHubOutputError("GitHub pull request title is not a string");
  }
  const body = payload.body == null ? "" : payload.body;
  if (typeof body !== "string") {
    throw new UnstructuredGitHubOutputError("GitHub pull request body is not a string");
  }
  return {
    number,
    title: payload.title,
    body,
    head: parsePullHead(payload.head),
    base: parsePullHead(payload.base),
  };
}

export function planCreatePullRequestComment(request, number) {
  return {
    kind: "create-pull-request-comment",
    number,
    body: request.body,
    applied: false,
  };
}

export function prepareCreatePullRequestComment(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const number = assertIssueNumber(request.number, "pull request number");
  assertRequiredText(request.body, "comment body");
  assertApplyClearance(mode, request.clearance, "pullRequests", adapter);
  return { mode, number };
}

export function parseCheckRunsPayload(payload) {
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub check-runs response is not a JSON object");
  }
  const rows = payload.check_runs;
  if (!Array.isArray(rows)) {
    throw new UnstructuredGitHubOutputError("GitHub check-runs list is not a JSON array");
  }
  const total = payload.total_count;
  if (total != null && (!Number.isSafeInteger(total) || total < 0 || total < rows.length)) {
    throw new UnstructuredGitHubOutputError("GitHub check-runs total_count is inconsistent");
  }
  if (Number.isSafeInteger(total) && total > rows.length) {
    throw new UnstructuredGitHubOutputError("GitHub check-runs list is incomplete");
  }
  return rows.map((row, index) => {
    if (row === null || typeof row !== "object" || Array.isArray(row)) {
      throw new UnstructuredGitHubOutputError(`GitHub check-run ${index} is not a JSON object`);
    }
    if (typeof row.name !== "string" || row.name.length === 0) {
      throw new UnstructuredGitHubOutputError("GitHub check-run name is not a string");
    }
    const conclusion = row.conclusion == null ? "pending" : row.conclusion;
    if (typeof conclusion !== "string" || conclusion.length === 0) {
      throw new UnstructuredGitHubOutputError("GitHub check-run conclusion is not a string");
    }
    return { name: row.name, conclusion, skipped: conclusion === "skipped" };
  });
}

function pullHeadSha(payload) {
  const sha = payload?.head?.sha;
  if (typeof sha !== "string" || !/^[0-9a-f]{40}$/i.test(sha)) {
    throw new UnstructuredGitHubOutputError("GitHub pull request head is missing a sha");
  }
  return sha;
}

export function planMergePullRequest(number, commitTitle) {
  const planned = { kind: "squash-merge", number, merge_method: "squash", applied: false };
  if (commitTitle != null) planned.commit_title = commitTitle;
  return planned;
}

export function prepareMergePullRequest(adapter, request) {
  assertRepository(adapter, request);
  const mode = assertMutationMode(request.mode);
  const number = assertIssueNumber(request.number, "pull request number");
  const mergeMethod = request.mergeMethod ?? "squash";
  if (mergeMethod !== "squash") {
    throw new GitHubAdapterError("squash-land merge_method must be squash");
  }
  const commitTitle =
    request.commitTitle == null ? undefined : assertReleasePullRequestTitle(request.commitTitle);
  assertApplyClearance(mode, request.clearance, "pullRequests", adapter);
  return { mode, number, mergeMethod, commitTitle };
}

export function parseMergePayload(payload, expectedNumber) {
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
    throw new UnstructuredGitHubOutputError("GitHub merge response is not a JSON object");
  }
  if (payload.merged !== true) {
    throw new GitHubAdapterError("GitHub squash merge did not report merged");
  }
  return {
    kind: "squash-merge",
    number: expectedNumber,
    merge_method: "squash",
    applied: true,
  };
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
const LIST_PAGE_SIZE = 100;
const MAX_LIST_PAGES = 50;
const LINK_NEXT = Symbol("githubctl.linkNext");

function parseResponseHeaders(headerBlock) {
  const headers = {};
  const lines = headerBlock.split(/\r?\n/);
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i];
    const colon = line.indexOf(":");
    if (colon <= 0) continue;
    const name = line.slice(0, colon).trim().toLowerCase();
    const value = line.slice(colon + 1).trim();
    if (Object.hasOwn(headers, name)) headers[name] = `${headers[name]}, ${value}`;
    else headers[name] = value;
  }
  return headers;
}

function parseLinkRelNext(linkHeader) {
  if (typeof linkHeader !== "string" || linkHeader.length === 0) return null;
  const parts = linkHeader.split(/,\s*(?=<)/);
  for (const part of parts) {
    const urlMatch = /<([^>]+)>/.exec(part);
    if (!urlMatch) continue;
    const relMatch = /(?:^|;)\s*rel\s*=\s*"?([^\s;,"]+)"?/i.exec(part);
    if (!relMatch) continue;
    const rels = relMatch[1].toLowerCase().split(/\s+/);
    if (rels.includes("next")) return urlMatch[1];
  }
  return null;
}

function attachLinkNext(payload, linkNext) {
  if (payload !== null && typeof payload === "object") {
    Object.defineProperty(payload, LINK_NEXT, {
      value: linkNext,
      enumerable: false,
      configurable: true,
    });
  }
  return payload;
}

function readLinkNext(payload) {
  if (payload === null || typeof payload !== "object") return null;
  const next = payload[LINK_NEXT];
  return typeof next === "string" && next.length > 0 ? next : null;
}

function withPerPage(path) {
  if (/[?&]per_page=/i.test(path)) return path;
  return path.includes("?")
    ? `${path}&per_page=${LIST_PAGE_SIZE}`
    : `${path}?per_page=${LIST_PAGE_SIZE}`;
}

function ghApiPathFromLink(url) {
  if (typeof url !== "string" || url.length === 0) {
    throw new UnstructuredGitHubOutputError("GitHub Link rel=next is missing");
  }
  if (url.startsWith("/")) return url;
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    throw new UnstructuredGitHubOutputError("GitHub Link rel=next is not a URL");
  }
  const path = `${parsed.pathname}${parsed.search}`;
  if (!path.startsWith("/")) {
    throw new UnstructuredGitHubOutputError("GitHub Link rel=next is not a URL");
  }
  return path;
}

function parseGhApiIncludeStdout(stdout) {
  const text = typeof stdout === "string" ? stdout : "";
  const split = text.search(/\r?\n\r?\n/);
  const headerBlock = split === -1 ? text : text.slice(0, split);
  const bodyText = split === -1 ? "" : text.slice(split).replace(/^\r?\n\r?\n/, "");
  const matched = HTTP_STATUS_LINE.exec(headerBlock.split(/\r?\n/, 1)[0] ?? "");
  if (!matched) {
    throw new UnstructuredGitHubOutputError("gh api output is missing an HTTP status line");
  }
  const status = Number.parseInt(matched[1], 10);
  const headers = parseResponseHeaders(headerBlock);
  if (status === 204) {
    if (bodyText.trim() !== "") {
      throw new UnstructuredGitHubOutputError("gh api 204 response is not empty");
    }
    return { status, payload: null, headers };
  }
  let payload;
  try {
    payload = JSON.parse(bodyText);
  } catch {
    payload = undefined;
  }
  if (payload === undefined || payload === null || typeof payload !== "object") {
    throw new UnstructuredGitHubOutputError("gh api JSON was not an object");
  }
  return { status, payload, headers };
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
      const { status, payload, headers } = parseGhApiIncludeStdout(result.stdout ?? "");
      if (status < 200 || status >= 300) {
        throw errorFromHttpStatus(status, payload, "gh api request failed");
      }
      return attachLinkNext(payload, parseLinkRelNext(headers.link));
    },
  };
}

export class GitHubAdapter {
  #transport;
  #project;
  #projectStatusField;
  #repositoryMilestones;

  constructor(options = {}) {
    bindOwnerRepo(this, options, "GitHubAdapter");
    if (options.transport) this.#transport = options.transport;
    else if (process.env.NODE_TEST_CONTEXT) {
      throw new LiveGitHubForbiddenInTestsError(
        "Live GitHub is not a test substrate; use FakeGitHubAdapter",
      );
    } else this.#transport = createGhApiTransport();
  }

  #getCompleteList(path, parsePage, incompleteMessage) {
    const rows = [];
    let current = withPerPage(path);
    const seen = new Set();
    for (;;) {
      if (seen.has(current) || seen.size >= MAX_LIST_PAGES) {
        throw new UnstructuredGitHubOutputError(incompleteMessage);
      }
      seen.add(current);
      const payload = this.#transport.request({ method: "GET", path: current });
      rows.push(...parsePage(payload));
      const next = readLinkNext(payload);
      if (!next) return rows;
      current = withPerPage(ghApiPathFromLink(next));
    }
  }

  inspectCapabilities(options = {}) {
    const required = Array.isArray(options.require)
      ? new Set(options.require)
      : new Set(["issues", "pullRequests", "projects", "actions"]);
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
    let projects = false;
    if (required.has("projects")) {
      try {
        const project = this.getProject(PROJECT_NUMBER);
        projects = project.viewerCanUpdate === true;
      } catch (error) {
        if (!(error instanceof MissingProjectIdentityError) && !expectedCapabilityMiss(error)) {
          throw error;
        }
      }
    }
    return capabilityRecord({
      authenticated: true,
      login: user.login,
      repository: { owner: this.owner, repo: this.repo },
      issues: repo.has_issues === true && issueWrite,
      pullRequests: pullWrite,
      projects,
      // GitHub's repository permission object has no distinct Actions bit.
      // workflow_dispatch requires the same write as pull-request mutation.
      actions: pullWrite,
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

  createReleasePullRequest(request) {
    const { mode } = prepareCreateReleasePullRequest(this, request);
    if (mode === "check") return planCreateReleasePullRequest(request);
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
      kind: "create-release-pull-request",
      number: parseGitHubResourceNumber(payload),
      title: request.title,
      body: request.body,
      head: request.head,
      base: request.base,
      closes: null,
      applied: true,
    };
  }

  pullsForHead(head) {
    assertRequiredText(head, "pull request head");
    return this.#getCompleteList(
      `/repos/${this.owner}/${this.repo}/pulls?head=${encodeURIComponent(`${this.owner}:${head}`)}`,
      parsePullsPayload,
      "GitHub pull request list is incomplete",
    );
  }

  getPullRequest(number) {
    const expected = assertIssueNumber(number, "pull request number");
    const payload = this.#transport.request({
      method: "GET",
      path: `/repos/${this.owner}/${this.repo}/pulls/${expected}`,
    });
    return parsePullRequestPayload(payload, expected);
  }

  createPullRequestComment(request) {
    const { mode, number } = prepareCreatePullRequestComment(this, request);
    if (mode === "check") return planCreatePullRequestComment(request, number);
    this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/issues/${number}/comments`,
      body: { body: request.body },
    });
    return {
      kind: "create-pull-request-comment",
      number,
      body: request.body,
      applied: true,
    };
  }

  listPullRequestCheckRuns(number) {
    const expected = assertIssueNumber(number, "pull request number");
    const payload = this.#transport.request({
      method: "GET",
      path: `/repos/${this.owner}/${this.repo}/pulls/${expected}`,
    });
    const returned = parseGitHubResourceNumber(payload);
    if (returned !== expected) {
      throw new UnstructuredGitHubOutputError(
        `GitHub pull request read returned number ${returned}, expected ${expected}`,
      );
    }
    const sha = pullHeadSha(payload);
    const checkRuns = this.#transport.request({
      method: "GET",
      path: `/repos/${this.owner}/${this.repo}/commits/${sha}/check-runs?per_page=100`,
    });
    if (readLinkNext(checkRuns)) {
      throw new UnstructuredGitHubOutputError("GitHub check-runs list is incomplete");
    }
    return parseCheckRunsPayload(checkRuns);
  }

  mergePullRequest(request) {
    const { mode, number, mergeMethod, commitTitle } = prepareMergePullRequest(this, request);
    if (mode === "check") return planMergePullRequest(number, commitTitle);
    const body = { merge_method: mergeMethod };
    if (commitTitle != null) body.commit_title = commitTitle;
    const payload = this.#transport.request({
      method: "PUT",
      path: `/repos/${this.owner}/${this.repo}/pulls/${number}/merge`,
      body,
    });
    return parseMergePayload(payload, number);
  }

  applyOperations(operations) {
    return applyOperations(this, operations);
  }

  getIssue(number) {
    const expected = assertIssueNumber(number);
    const payload = this.#transport.request({
      method: "GET",
      path: `/repos/${this.owner}/${this.repo}/issues/${expected}`,
    });
    return parseIssuePayload(payload, expected);
  }

  getIssueIdentity(number) {
    const expected = assertIssueNumber(number);
    const payload = this.#transport.request({
      method: "GET",
      path: `/repos/${this.owner}/${this.repo}/issues/${expected}`,
    });
    if (payload?.pull_request != null) {
      throw new UnstructuredGitHubOutputError(`issue #${expected} is a pull request`);
    }
    const returned = parseGitHubResourceNumber(payload);
    if (returned !== expected || !Number.isSafeInteger(payload.id) || payload.id < 1) {
      throw new UnstructuredGitHubOutputError(`issue #${expected} is missing its database id`);
    }
    return { id: payload.id, number: expected, owner: this.owner, repo: this.repo };
  }

  getIssueLabels(number) {
    const expected = assertIssueNumber(number);
    return this.#getCompleteList(
      `/repos/${this.owner}/${this.repo}/issues/${expected}/labels`,
      parseLabelsPayload,
      "GitHub labels list is incomplete",
    );
  }

  getRepositoryLabels() {
    return this.#getCompleteList(
      `/repos/${this.owner}/${this.repo}/labels`,
      parseRepositoryLabelsPayload,
      "GitHub repository labels list is incomplete",
    );
  }

  createRepositoryLabel(request) {
    const { mode, label } = prepareCreateRepositoryLabel(this, request);
    if (mode === "check") return planCreateRepositoryLabel(label);
    const payload = this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/labels`,
      body: label,
    });
    return {
      kind: "create-repository-label",
      label: parseRepositoryLabel(payload),
      applied: true,
    };
  }

  updateRepositoryLabel(request) {
    const { mode, existing, label } = prepareUpdateRepositoryLabel(this, request);
    if (mode === "check") return planUpdateRepositoryLabel(existing, label);
    const payload = this.#transport.request({
      method: "PATCH",
      path: `/repos/${this.owner}/${this.repo}/labels/${encodeURIComponent(existing)}`,
      body: { new_name: label.name, color: label.color, description: label.description },
    });
    return {
      kind: "update-repository-label",
      previous: existing,
      label: parseRepositoryLabel(payload),
      applied: true,
    };
  }

  getRepositoryMilestones() {
    if (this.#repositoryMilestones === undefined) {
      this.#repositoryMilestones = this.#getCompleteList(
        `/repos/${this.owner}/${this.repo}/milestones?state=all`,
        parseMilestoneListPayload,
        "GitHub repository milestone list is incomplete",
      );
    }
    return this.#repositoryMilestones;
  }

  createRepositoryMilestone(request) {
    const { mode, milestone } = prepareCreateRepositoryMilestone(this, request);
    if (mode === "check") return planCreateRepositoryMilestone(milestone);
    const payload = this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/milestones`,
      body: milestone,
    });
    const [created] = parseMilestoneListPayload([payload]);
    this.#repositoryMilestones = undefined;
    return { kind: "create-repository-milestone", milestone: created, applied: true };
  }

  updateRepositoryMilestone(request) {
    const { mode, number, milestone, patch } = prepareUpdateRepositoryMilestone(this, request);
    if (mode === "check") return planUpdateRepositoryMilestone(request.existing, milestone);
    const payload = this.#transport.request({
      method: "PATCH",
      path: `/repos/${this.owner}/${this.repo}/milestones/${number}`,
      body: patch,
    });
    const [updated] = parseMilestoneListPayload([payload]);
    if (updated.number !== number) {
      throw new UnstructuredGitHubOutputError(
        `GitHub milestone update returned number ${updated.number}, expected ${number}`,
      );
    }
    this.#repositoryMilestones = undefined;
    return { kind: "update-repository-milestone", milestone: updated, applied: true };
  }

  addIssueLabels(request) {
    const { mode, number, labels } = prepareAddIssueLabels(this, request);
    if (mode === "check") return planAddIssueLabels(number, labels);
    const payload = this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/issues/${number}/labels`,
      body: { labels },
    });
    parseLabelsPayload(payload);
    return { kind: "add-issue-labels", number, labels, applied: true };
  }

  removeIssueLabel(request) {
    const { mode, number, label } = prepareRemoveIssueLabel(this, request);
    if (mode === "check") return planRemoveIssueLabel(number, label);
    const payload = this.#transport.request({
      method: "DELETE",
      path: `/repos/${this.owner}/${this.repo}/issues/${number}/labels/${encodeURIComponent(label)}`,
    });
    parseLabelsPayload(payload);
    return { kind: "remove-issue-label", number, label, applied: true };
  }

  getIssueDependencies(number) {
    const expected = assertIssueNumber(number);
    return this.#getCompleteList(
      `/repos/${this.owner}/${this.repo}/issues/${expected}/dependencies/blocked_by`,
      parseIssueDependenciesPayload,
      "GitHub issue dependency list is incomplete",
    );
  }

  addIssueDependency(request) {
    const { mode, number, blockingNumber, blockingId } = prepareAddIssueDependency(this, request);
    if (mode === "check") return planAddIssueDependency(number, blockingNumber, blockingId);
    this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/issues/${number}/dependencies/blocked_by`,
      body: { issue_id: blockingId },
    });
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
    this.#transport.request({
      method: "DELETE",
      path: `/repos/${this.owner}/${this.repo}/issues/${number}/dependencies/blocked_by/${blockingId}`,
    });
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
    const repository = { owner: this.owner, repo: this.repo };
    const parentSnapshot = this.getIssueProjectState(parentIssueNumber);
    const subIssueSnapshot = this.getIssueProjectState(subIssueNumber);
    const state = classifyIssueSubIssueState(
      parentSnapshot,
      subIssueSnapshot,
      parentIssueNumber,
      subIssueNumber,
      repository,
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
    const mutation = this.#graphql(
      ADD_SUB_ISSUE_MUTATION,
      {
        issueId: parentSnapshot.id,
        subIssueId: subIssueSnapshot.id,
        replaceParent: false,
      },
      () => new GitHubAdapterError("addSubIssue failed"),
    );
    parseAddIssueSubIssueResult({ data: mutation }, parentSnapshot, subIssueSnapshot, repository);
    return {
      kind: "add-issue-sub-issue",
      parentIssueNumber,
      subIssueNumber,
      applied: true,
    };
  }

  setAiResultLabel(request) {
    const { mode, number, label } = prepareSetAiResultLabel(this, request);
    if (mode === "check") return planSetAiResultLabel(number, label);
    const names = this.getIssueLabels(number);
    const previous = selectAiResultLabel(names, number);
    if (previous === label) {
      return {
        kind: "set-ai-result-label",
        number,
        label,
        previous,
        applied: true,
        unchanged: true,
      };
    }
    this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/issues/${number}/labels`,
      body: { labels: [label] },
    });
    if (previous) {
      this.#transport.request({
        method: "DELETE",
        path: `/repos/${this.owner}/${this.repo}/issues/${number}/labels/${encodeURIComponent(previous)}`,
      });
    }
    return {
      kind: "set-ai-result-label",
      number,
      label,
      previous,
      applied: true,
    };
  }

  #graphql(query, variables, createError) {
    return parseGraphqlResult(
      this.#transport.request({
        method: "POST",
        path: "graphql",
        body: { query, variables },
      }),
      createError,
    );
  }

  getProject(number = PROJECT_NUMBER) {
    assertProjectNumber(number);
    if (this.#project === undefined) {
      this.#project = projectFromGraphqlData(
        this.#graphql(
          PROJECT_LOOKUP_QUERY,
          { owner: this.owner, name: this.repo, number },
          missingProjectError(number),
        ),
        number,
      );
    }
    return this.#project;
  }

  getProjectStatusField(number = PROJECT_NUMBER) {
    assertProjectNumber(number);
    if (this.#projectStatusField === undefined) {
      this.#projectStatusField = projectStatusFieldFromGraphqlData(
        this.#graphql(
          PROJECT_STATUS_FIELD_QUERY,
          { owner: this.owner, name: this.repo, number },
          missingProjectError(number),
        ),
        number,
      );
    }
    return this.#projectStatusField;
  }

  getIssueProjectState(issueNumber) {
    const number = assertIssueNumber(issueNumber);
    const project = this.getProject(PROJECT_NUMBER);
    const data = this.#graphql(
      ISSUE_PROJECT_STATE_QUERY,
      { owner: this.owner, name: this.repo, number },
      () => new NotFoundError(`issue #${number} is missing`),
    );
    return parseIssueProjectState({ data }, number, project.id, {
      owner: this.owner,
      repo: this.repo,
    });
  }

  addIssueToProject(request) {
    const { mode, issueNumber } = prepareAddIssueToProject(this, request);
    const project = this.getProject(PROJECT_NUMBER);
    if (mode === "check") return planAddIssueToProject(issueNumber);
    const missingIssue = () => new NotFoundError(`issue #${issueNumber} is missing`);
    const data = this.#graphql(
      ISSUE_ID_QUERY,
      { owner: this.owner, name: this.repo, number: issueNumber },
      missingIssue,
    );
    const contentId = issueIdFromGraphql(data, issueNumber);
    const alreadyMember = projectMembership(data.repository?.issue, project);
    if (alreadyMember) {
      return {
        kind: "add-project-item",
        number: PROJECT_NUMBER,
        issueNumber,
        applied: true,
        already_member: true,
        unchanged: true,
      };
    }
    const mutation = this.#graphql(
      ADD_ITEM_MUTATION,
      { projectId: project.id, contentId },
      () => new GitHubAdapterError("addProjectV2ItemById failed"),
    );
    const itemId = mutation.addProjectV2ItemById?.item?.id;
    if (typeof itemId !== "string" || itemId.length === 0) {
      if (alreadyMember !== true) {
        throw new GitHubAdapterError("addProjectV2ItemById did not return an item id");
      }
    }
    const added = {
      kind: "add-project-item",
      number: PROJECT_NUMBER,
      issueNumber,
      applied: true,
    };
    added.already_member = false;
    if (typeof itemId === "string" && itemId.length > 0) added.item_id = itemId;
    return added;
  }

  setIssueProjectStatus(request) {
    const { mode, issueNumber, status } = prepareSetIssueProjectStatus(this, request);
    const project = this.getProjectStatusField(PROJECT_NUMBER);
    const snapshot = this.getIssueProjectState(issueNumber);
    if (!snapshot.item) {
      throw new MissingProjectIdentityError(
        `issue #${issueNumber} Project ${PROJECT_NUMBER} item is missing`,
      );
    }
    if (mode === "check") {
      return planSetIssueProjectStatus(issueNumber, status, snapshot.item?.status ?? null, false);
    }
    const itemId = snapshot.item?.id ?? null;
    if (!itemId) {
      throw new MissingProjectIdentityError(
        `issue #${issueNumber} Project ${PROJECT_NUMBER} item is missing`,
      );
    }
    const current = snapshot.item?.status ?? null;
    if (current === status) {
      return {
        kind: "set-project-status",
        number: PROJECT_NUMBER,
        issueNumber,
        status,
        current,
        added: false,
        unchanged: true,
        applied: true,
      };
    }
    const mutation = this.#graphql(
      SET_PROJECT_STATUS_MUTATION,
      {
        projectId: project.id,
        itemId,
        fieldId: project.fieldId,
        optionId: project.options.get(status),
      },
      () => new GitHubAdapterError("updateProjectV2ItemFieldValue failed"),
    );
    if (mutation.updateProjectV2ItemFieldValue?.projectV2Item?.id !== itemId) {
      throw new GitHubAdapterError("updateProjectV2ItemFieldValue returned the wrong item");
    }
    return {
      kind: "set-project-status",
      number: PROJECT_NUMBER,
      issueNumber,
      status,
      current,
      added: false,
      applied: true,
    };
  }

  setIssueMilestone(request) {
    const { mode, issueNumber, title } = prepareSetIssueMilestone(this, request);
    if (mode === "check") return planSetIssueMilestone(issueNumber, title);
    this.getIssue(issueNumber);
    const found = this.getRepositoryMilestones().find((row) => row.title === title);
    if (!found) throw new NotFoundError(`milestone ${title} is missing`);
    const payload = this.#transport.request({
      method: "PATCH",
      path: `/repos/${this.owner}/${this.repo}/issues/${issueNumber}`,
      body: { milestone: found.number },
    });
    parseIssuePayload(payload, issueNumber);
    return { kind: "set-milestone", issueNumber, title, applied: true };
  }

  listMilestoneIssues(title) {
    assertRequiredText(title, "milestone title");
    const milestones = this.#getCompleteList(
      `/repos/${this.owner}/${this.repo}/milestones?state=all`,
      parseMilestoneListPayload,
      "GitHub milestone list is incomplete",
    );
    const found = milestones.find((row) => row.title === title);
    if (!found) throw new NotFoundError(`milestone ${title} is missing`);
    return this.#getCompleteList(
      `/repos/${this.owner}/${this.repo}/issues?milestone=${found.number}&state=all`,
      (payload) => parseMilestoneIssuesPayload(payload, title),
      "GitHub milestone issue list is incomplete",
    );
  }

  closeMilestone(request) {
    const { mode, title } = prepareCloseMilestone(this, request);
    if (mode === "check") return planCloseMilestone(title);
    const milestones = this.#getCompleteList(
      `/repos/${this.owner}/${this.repo}/milestones?state=all`,
      parseMilestoneListPayload,
      "GitHub milestone list is incomplete",
    );
    const found = milestones.find((row) => row.title === title);
    if (!found) throw new NotFoundError(`milestone ${title} is missing`);
    this.#transport.request({
      method: "PATCH",
      path: `/repos/${this.owner}/${this.repo}/milestones/${found.number}`,
      body: { state: "closed" },
    });
    this.#repositoryMilestones = undefined;
    return { kind: "close-milestone", title, applied: true };
  }

  dispatchReleaseRehearsal(request = {}) {
    const { mode, ref } = prepareDispatchReleaseRehearsal(this, request);
    if (mode === "check") return planDispatchReleaseRehearsal();
    this.#transport.request({
      method: "POST",
      path: `/repos/${this.owner}/${this.repo}/actions/workflows/release-check.yml/dispatches`,
      body: { ref },
    });
    return {
      kind: "dispatch-release-check",
      workflow: "release-check.yml",
      applied: true,
    };
  }
}
