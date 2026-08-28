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

export class PartialFailureError extends GitHubAdapterError {
  constructor({ succeeded, failed }) {
    const numbers = succeeded
      .map((row) => row.number)
      .filter((number) => Number.isSafeInteger(number));
    super(
      `partial GitHub mutation failure after ${succeeded.length} succeeded operation(s)` +
        (numbers.length ? ` (numbers ${numbers.join(", ")})` : ""),
    );
    this.succeeded = succeeded;
    this.failed = failed;
  }
}
