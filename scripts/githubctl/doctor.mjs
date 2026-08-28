import { mintDoctorClearance } from "./adapter.mjs";

export class GitHubDoctor {
  constructor(adapter) {
    this.adapter = adapter;
  }

  check() {
    const capabilities = this.adapter.inspectCapabilities();
    const errors = [];
    if (!capabilities.authenticated) errors.push("unauthenticated");
    if (!capabilities.repository) errors.push("repository");
    if (!capabilities.issues) errors.push("issues");
    if (!capabilities.pullRequests) errors.push("pull-requests");
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
              issues: true,
              pullRequests: true,
            }),
          )
        : null,
    };
  }
}
