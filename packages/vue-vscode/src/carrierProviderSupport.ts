/**
 * Which `verter.typeProvider` values can serve framework carriers, and what the
 * user is told when the selected one cannot.
 *
 * The extension-hosted service (`verter.typeProvider: "extension"`) serves plain
 * `.ts`/`.js` correctly — each project from the TypeScript that project
 * installed. It does NOT serve `.vue`/`.svelte`: carrier publication is
 * suppressed for the provider kind it registers under, so no generated companion
 * ever reaches it and every carrier query arrives for a file it has no binding
 * for. The setting advertised a TypeScript provider for Vue files while
 * delivering, for carriers, nothing at all.
 *
 * The containment here is deliberately NOT a refusal of the whole provider.
 * Refusing selection would delete a capability that works (per-project plain
 * TypeScript, which is what the provider's acceptance route exists to prove) in
 * order to contain one that is missing. Instead the CARRIER half fails closed and
 * says so: a notice when a carrier opens under it, plus a persistent status-bar
 * warning for as long as one is open (`computeStatusBarState`).
 */

import { isFrameworkCarrierLanguageId } from "./frameworkWiring";

/**
 * `verter.typeProvider` values that cannot serve `.vue`/`.svelte`.
 *
 * Membership is the ONE authority for the containment: the notice, the status
 * bar, and the setting copy all key on it.
 */
const CARRIER_UNSUPPORTED_TYPE_PROVIDERS: ReadonlySet<string> = new Set(["extension"]);

/** The providers the notice sends the user to. */
export const CARRIER_SERVING_REMEDY_PROVIDERS = ["auto", "tsserver", "tsgo"] as const;

/** Whether `typeProvider` serves framework carrier sources. */
export function providerServesFrameworkCarriers(typeProvider: string): boolean {
  return !CARRIER_UNSUPPORTED_TYPE_PROVIDERS.has(typeProvider);
}

export interface CarrierUnsupportedInput {
  /** The active `verter.typeProvider` value. */
  readonly typeProvider: string;
  /** The document's language id, or `undefined` when there is no document. */
  readonly languageId: string | undefined;
}

export interface CarrierUnsupportedNotice {
  readonly message: string;
}

/** The one sentence describing the gap, shared by the notice and the tooltip. */
const CARRIER_UNSUPPORTED_SENTENCE =
  `the "extension" type provider does not serve .vue or .svelte files — ` +
  `they get no diagnostics, no hover and no completion from it`;

/** The status bar's persistent tooltip while an unservable carrier is open. */
export const CARRIER_UNSUPPORTED_TOOLTIP =
  `Verter: ${CARRIER_UNSUPPORTED_SENTENCE}. Plain .ts/.js files in this project are still ` +
  `served. Set verter.typeProvider to ${CARRIER_SERVING_REMEDY_PROVIDERS.join(", ")} ` +
  `to type-check this file.`;

/**
 * The notice to raise when a document opens under the active provider, or
 * `undefined` when the pairing is served.
 *
 * Conditional in both directions by construction: a served provider and a
 * non-carrier document both return `undefined`, so the containment can never
 * degrade into an unconditional warning.
 */
export function computeCarrierUnsupportedNotice(
  input: CarrierUnsupportedInput,
): CarrierUnsupportedNotice | undefined {
  if (providerServesFrameworkCarriers(input.typeProvider)) return undefined;
  if (!isFrameworkCarrierLanguageId(input.languageId)) return undefined;
  return {
    message:
      `Verter: ${CARRIER_UNSUPPORTED_SENTENCE}. This file is open but unchecked. ` +
      `Switch verter.typeProvider to ${CARRIER_SERVING_REMEDY_PROVIDERS.join(", ")} for ` +
      `Vue and Svelte support; plain .ts/.js files stay served either way.`,
  };
}
