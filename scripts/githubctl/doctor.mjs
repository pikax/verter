import { mintDoctorClearance } from "./adapter.mjs";
import { GitHubAdapterError } from "./errors.mjs";

export const DOCTOR_ALL_CAPABILITIES = Object.freeze(["issues", "pullRequests", "projects"]);
export const SYNC_ISSUES_CAPABILITIES = Object.freeze(["issues"]);
export const CREATE_PR_CAPABILITIES = Object.freeze(["issues", "pullRequests"]);
export const REVIEW_SUMMARY_CAPABILITIES = Object.freeze(["issues", "pullRequests"]);
export const SQUASH_LAND_CAPABILITIES = Object.freeze(["pullRequests"]);
export const SCHEDULE_CAPABILITIES = Object.freeze(["issues", "projects"]);

const CAPABILITY_ERRORS = Object.freeze({
  issues: "issues",
  pullRequests: "pull-requests",
  projects: "projects",
});

function requiredCapabilities(require) {
  if (require == null) return DOCTOR_ALL_CAPABILITIES;
  if (!Array.isArray(require) || require.length === 0) {
    throw new GitHubAdapterError("doctor require must be a non-empty capability list");
  }
  for (const name of require) {
    if (!(name in CAPABILITY_ERRORS)) {
      throw new GitHubAdapterError(`unknown doctor capability ${name}`);
    }
  }
  return require;
}

export class GitHubDoctor {
  constructor(adapter) {
    this.adapter = adapter;
  }

  check(options = {}) {
    const capabilities = this.adapter.inspectCapabilities();
    const required = requiredCapabilities(options.require);
    const errors = [];
    if (!capabilities.authenticated) errors.push("unauthenticated");
    if (!capabilities.repository) errors.push("repository");
    for (const name of required) {
      if (capabilities[name] !== true) errors.push(CAPABILITY_ERRORS[name]);
    }
    const ok = errors.length === 0;
    return {
      ok,
      errors,
      capabilities,
      clearance: ok
        ? mintDoctorClearance(
            this.adapter,
            Object.freeze({
              kind: "github-doctor-clearance",
              owner: this.adapter.owner,
              repo: this.adapter.repo,
              issues: capabilities.issues === true,
              pullRequests: capabilities.pullRequests === true,
              projects: capabilities.projects === true,
            }),
          )
        : null,
    };
  }
}
