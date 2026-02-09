import type { StatisticsRequestParams, StatisticsSnapshot } from "./statistics";

// fluff
export type RequestTyped = {
  onRequest: <T extends RequestType>(
    type: T,
    cb: (e: RequestParams[T]) => RequestResponse[T] | null,
  ) => Promise<void>;

  sendRequest: <T extends RequestType>(
    type: T,
    params: RequestParams[T],
  ) => Promise<RequestResponse[T] | null>;
};

export type OmitRequest<T> = Omit<T, keyof RequestTyped>;
export type PatchRequest<T> = OmitRequest<T> & RequestTyped;
// /fluff

export enum RequestType {
  GetCompiledCode = "$/getCompiledCode",
  GetStatistics = "$/verter/getStatistics",
}
export type RequestParams = {
  [RequestType.GetCompiledCode]: string;
  [RequestType.GetStatistics]: StatisticsRequestParams | undefined;
};

export type RequestResponse = {
  [RequestType.GetCompiledCode]: {
    js: { code: string; map: any | undefined };
    css: { code: string; map: any | undefined };
    wasm: { code: string; map: any | undefined };
  };
  [RequestType.GetStatistics]: StatisticsSnapshot;
};
