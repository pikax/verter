import type { FactResult as SourceFact } from "../src/facts.js";
import type { FactResult as PublicFact } from "../dist/index.js";
import type { NativeEventMeta, NativeTypeExprLike } from "../src/native-component-meta.js";

const complete: SourceFact<boolean> = { kind: "complete", value: false };
const approximate: PublicFact<boolean> = { kind: "approximate", value: false, causes: [9001] };
const unavailable: SourceFact<boolean> = { kind: "unavailable", causes: [100] };
const publicComplete: PublicFact<boolean> = complete;
const sourceApproximate: SourceFact<boolean> = approximate;
// @ts-expect-error unavailable facts carry no value
unavailable.value;
// @ts-expect-error approximate facts require a nonempty cause set
const emptyCauses: PublicFact<boolean> = { kind: "approximate", value: false, causes: [] };
void publicComplete;
void sourceApproximate;
void emptyCauses;

declare const unavailableEvent: NativeEventMeta;
// @ts-expect-error an unavailable payload is not a value the caller can read
const missingPayload: NativeTypeExprLike = unavailableEvent.payload;
declare const healthyEvent: NativeEventMeta & { payload: NativeTypeExprLike };
const presentPayload: NativeTypeExprLike = healthyEvent.payload;
void missingPayload;
void presentPayload;
