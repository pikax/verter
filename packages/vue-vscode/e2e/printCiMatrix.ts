import { E2E_CI_TIERS, type E2eCiTier, buildGitHubActionsMatrix } from "./lib/routeInventory";

// `--tier=pr` (the default) is the pull request matrix; `--tier=tsserver` is
// the tsserver routes the scheduled `tsserver nightly` workflow runs.
const tierArg = process.argv.slice(2).find((arg) => arg.startsWith("--tier="));
const tier = (tierArg ? tierArg.slice("--tier=".length) : "pr") as E2eCiTier;
if (!E2E_CI_TIERS.includes(tier)) {
  throw new Error(
    `unknown VS Code E2E CI tier ${JSON.stringify(tier)}; expected one of ${E2E_CI_TIERS.join(", ")}`,
  );
}
process.stdout.write(JSON.stringify(buildGitHubActionsMatrix(tier)));
