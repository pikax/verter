import { buildGitHubActionsMatrix } from "./lib/routeInventory";

process.stdout.write(JSON.stringify(buildGitHubActionsMatrix()));
