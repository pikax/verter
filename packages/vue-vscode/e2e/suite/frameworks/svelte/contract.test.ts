import { FIXTURE_NAME } from "../../../helpers";
import { svelteContract } from "../../../frameworks/svelte/descriptor";
import { registerFrameworkContract } from "../../../lib/frameworkContract";

if (FIXTURE_NAME === svelteContract.fixture) registerFrameworkContract(svelteContract);
