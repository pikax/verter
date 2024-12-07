import type { MagicString } from "@vue/compiler-sfc";
import type { WalkResult } from "../../../types.js";

export type TranspileContext = {
  s: MagicString;

  // prevent the identifiers from getting the accessor prefixed
  // for example the v-for="item in items" has "item" added, because
  // is a blocked variable
  ignoredIdentifiers: string[];

  // expose declarations, in case we need to import something
  // or declare a specific variable
  declarations: WalkResult[];

  accessors: {
    /**
     * Override the accessor for the template ctx
     * @default "___VERTER___ctx"
     */
    ctx: string;

    /**
     * Override the accessor for the template component
     * @default "___VERTER___comp"
     */
    comp: string;

    /**
     * Override the accessor for slot
     * @default "___VERTER___ctx.$slots"
     */
    slot: string;

    /**
     * Override the accessor for template
     * @default "___VERTER___template"
     */
    template: string;

    /**
     * Override the accessor for the slotCallback,
     * converts $slots.default(props) into
     * ```ts
     * const slot = ___VERTER___SLOT_CALLBACK($slots.default)
     * slot((props)=> {})
     * ```
     */
    slotCallback: string;

    /**
     * Used to convert slot declaration into render Component
     *
     * @default "___VERTER___SLOT_TO_COMPONENT"
     */
    slotToComponent: string;

    normalizeClass: string;
    normalizeStyle: string;

    renderList: string;

    /**
     * helper to call events
     */
    eventCb: string;

    /**
     * Used as the accessor for v-slot usages
     */
    componentInstance: string;
  };

  conditions: {
    // current conditions
    ifs: string[];
    // other conditions
    elses: string[];
  };

  attributes: {
    /**
     * prevents camel if the prop starts with *
     */
    camelWhitelist: string[];
  };

  webComponents: string[];
};
