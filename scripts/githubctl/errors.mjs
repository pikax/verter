export class GitHubAdapterError extends Error {
  constructor(message, options) {
    super(message, options);
    this.name = this.constructor.name;
  }
}

export class ProtectedMappingError extends GitHubAdapterError {}
export class PermissionDeniedError extends GitHubAdapterError {}
export class WrongRepositoryError extends GitHubAdapterError {}
export class DuplicateError extends GitHubAdapterError {}
export class ClosingLinkError extends GitHubAdapterError {}
export class DoctorRequiredError extends GitHubAdapterError {}
export class MutationModeRequiredError extends GitHubAdapterError {}
export class NotFoundError extends GitHubAdapterError {}
export class UnstructuredGitHubOutputError extends GitHubAdapterError {}
export class LiveGitHubForbiddenInTestsError extends GitHubAdapterError {}
export class MappingMismatchError extends GitHubAdapterError {}
export class InvalidIssueNumberError extends GitHubAdapterError {}
export class IssueSyncError extends GitHubAdapterError {}
export class SelectionError extends IssueSyncError {}
export class MissingAncestorError extends IssueSyncError {}
export class MissingProjectIdentityError extends GitHubAdapterError {}
export class MissingIssueMappingError extends GitHubAdapterError {}
export class NonReadyNodeError extends GitHubAdapterError {}
export class BlockingFindingError extends GitHubAdapterError {}
export class CiFailedError extends GitHubAdapterError {}
export class UnsupportedVerdictError extends GitHubAdapterError {}
export class IgnoredIssueError extends GitHubAdapterError {}
export class AmbiguousAiLabelError extends GitHubAdapterError {}
export class AmbiguousWaiverError extends GitHubAdapterError {}
export class UnauthorizedReleaseError extends GitHubAdapterError {}

export function mutationIdentity(row) {
  if (!row || typeof row !== "object") return null;
  const number = Number.isSafeInteger(row.gh_issue)
    ? row.gh_issue
    : Number.isSafeInteger(row.number)
      ? row.number
      : undefined;
  const identity = {};
  if (typeof row.node_id === "string" && row.node_id.length > 0) identity.node_id = row.node_id;
  if (number != null) identity.number = number;
  if (typeof row.kind === "string" && row.kind.length > 0) identity.kind = row.kind;
  if (typeof row.label === "string" && row.label.length > 0) identity.label = row.label;
  if (typeof row.milestone === "string" && row.milestone.length > 0) {
    identity.milestone = row.milestone;
  }
  if (typeof row.mapping_written === "boolean") {
    identity.mapping_written = row.mapping_written;
  }
  if (
    identity.node_id == null &&
    identity.number == null &&
    identity.label == null &&
    identity.milestone == null
  ) {
    return null;
  }
  return identity;
}

export class PartialFailureError extends GitHubAdapterError {
  constructor({ succeeded, failed }) {
    const completed = Array.isArray(succeeded) ? succeeded : [];
    const identities = completed.map(mutationIdentity).filter(Boolean);
    const summary = identities
      .map((row) => {
        const parts = [];
        if (row.node_id) parts.push(row.node_id);
        if (row.number != null) parts.push(`#${row.number}`);
        if (row.label) parts.push(row.label);
        if (row.milestone) parts.push(row.milestone);
        if (row.mapping_written != null) parts.push(`mapping_written=${row.mapping_written}`);
        return parts.join(" ");
      })
      .join("; ");
    super(
      `partial GitHub mutation failure after ${completed.length} succeeded operation(s)` +
        (summary ? ` (${summary})` : ""),
    );
    this.succeeded = completed;
    this.failed = failed;
  }
}
