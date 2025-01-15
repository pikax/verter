import { handleHelpers } from "../../../utils";
import _BundlerHelper from "./bundler.ts?raw";
// import _BundlerHelper from "./src/v5/process/template/helperBundlerHelper.ts?raw";

export const BundlerHelper = handleHelpers(_BundlerHelper);
