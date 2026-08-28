export {
  GitHubAdapter,
  applyOperations,
  hasExactMappedClosingLink,
  mappedClosingLink,
  parseGitHubResourceNumber,
} from "./adapter.mjs";
export { FakeGitHubAdapter } from "./fake.mjs";
export { GitHubDoctor } from "./doctor.mjs";
export {
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
