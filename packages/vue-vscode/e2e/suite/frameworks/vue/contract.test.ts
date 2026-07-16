import { FIXTURE_NAME } from "../../../helpers";
import { vueContract } from "../../../frameworks/vue/descriptor";
import { registerFrameworkContract } from "../../../lib/frameworkContract";

if (FIXTURE_NAME === vueContract.fixture) registerFrameworkContract(vueContract);
