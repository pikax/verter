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
  identity.mapping_written = row.mapping_written === true;
  if (identity.node_id == null && identity.number == null) return null;
  return identity;
}

export class PartialFailureError extends GitHubAdapterError {
  constructor({ succeeded, failed }) {
    const identities = (Array.isArray(succeeded) ? succeeded : [])
      .map(mutationIdentity)
      .filter(Boolean);
    const summary = identities
      .map((row) => {
        const parts = [];
        if (row.node_id) parts.push(row.node_id);
        if (row.number != null) parts.push(`#${row.number}`);
        if (row.node_id) parts.push(`mapping_written=${row.mapping_written}`);
        return parts.join(" ");
      })
      .join("; ");
    super(
      `partial GitHub mutation failure after ${succeeded.length} succeeded operation(s)` +
        (summary ? ` (${summary})` : ""),
    );
    this.succeeded = succeeded;
    this.failed = failed;
  }
}
