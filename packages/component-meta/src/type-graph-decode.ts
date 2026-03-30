import type { NativeComponentMetaResult } from "./native-component-meta.js";
import { decodeTypedComponentMetaPayload } from "./type-graph-proto-decode.js";

export function decodeComponentMetaPayload(
  payload: ArrayBuffer | ArrayBufferView,
): NativeComponentMetaResult {
  return decodeTypedComponentMetaPayload(payload);
}
