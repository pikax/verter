/**
 * @ts-expect-error suite for Vue: each directive MUST suppress a real error.
 * If the imported surface is `any`, TS2578 (unused @ts-expect-error) fails E2E.
 */
import TypeNegChild from "./TypeNegChild.vue";

type ChildProps = InstanceType<typeof TypeNegChild>["$props"];

// --- props (component instance surface) ---

// @ts-expect-error count is number, not string
export const badCount: ChildProps = { count: "not-a-number", title: "ok" };

// @ts-expect-error title is string, not number
export const badTitle: ChildProps = { count: 1, title: 99 };

// @ts-expect-error enabled is boolean, not string
export const badEnabled: ChildProps = { count: 1, title: "ok", enabled: "yes" };

// --- handler shapes (overload / assignability) ---

type PickHandler = (value: string) => void;
type ChangeHandler = (next: number) => void;
type ClickHandler = (ev: MouseEvent) => void;

// @ts-expect-error pick payload is string; number parameter is not assignable
export const badPickHandler: PickHandler = (n: number) => {
  void n;
};

// @ts-expect-error change payload is number; string parameter is wrong
export const badChangeHandler: ChangeHandler = (s: string) => {
  void s;
};

// @ts-expect-error MouseEvent handler rejects string-only parameter
export const badClickHandler: ClickHandler = (s: string) => {
  void s;
};

void TypeNegChild;
void badCount;
void badTitle;
void badEnabled;
void badPickHandler;
void badChangeHandler;
void badClickHandler;
