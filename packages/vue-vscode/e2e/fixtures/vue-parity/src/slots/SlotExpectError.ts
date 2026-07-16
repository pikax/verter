/**
 * @ts-expect-error negatives for Vue slot prop shapes.
 * Unused @ts-expect-error (TS2578) means slot types collapsed to any.
 */
import SlotTypedHost from "./SlotTypedHost.vue";

/** Structural slot prop bags matching SlotTypedHost.defineSlots. */
type HeaderSlot = { title: string; count: number };
type DefaultSlot = { body: string; flag: boolean };
type FooterSlot = { ok: boolean };

// @ts-expect-error title is string, not number
export const badHeaderTitle: HeaderSlot = { title: 1, count: 2 };

// @ts-expect-error count is number, not string
export const badHeaderCount: HeaderSlot = { title: "t", count: "n" };

// @ts-expect-error body is string
export const badDefaultBody: DefaultSlot = { body: 1, flag: true };

// @ts-expect-error flag is boolean
export const badDefaultFlag: DefaultSlot = { body: "b", flag: "yes" };

// @ts-expect-error ok is boolean
export const badFooter: FooterSlot = { ok: "no" };

// Function shapes that consume slot props (scoped slot render functions)
type HeaderRender = (props: HeaderSlot) => unknown;

// @ts-expect-error render must accept HeaderSlot; wrong param shape
export const badHeaderRender: HeaderRender = (p: { title: number; count: string }) => p;

void SlotTypedHost;
void badHeaderTitle;
void badHeaderCount;
void badDefaultBody;
void badDefaultFlag;
void badFooter;
void badHeaderRender;
