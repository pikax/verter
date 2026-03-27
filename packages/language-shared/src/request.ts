import type { StatisticsRequestParams, StatisticsSnapshot } from "./statistics";
import type {
  ComponentParentsResponse,
  FileAnalysisSnapshot,
  ProjectOverview,
  VirtualFilesResponse,
} from "./analysis";
import type { RouteAnalysisSnapshot } from "./routes";

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
  GetVirtualFiles = "$/verter/getVirtualFiles",
  GetAnalysis = "$/verter/getAnalysis",
  GetProjectOverview = "$/verter/getProjectOverview",
  GetBindingTypes = "$/verter/getBindingTypes",
  GetComponentParents = "$/verter/getComponentParents",
  ApplyStyleOverrides = "$/verter/applyStyleOverrides",
  GetRouteTree = "$/verter/getRouteTree",
}

/** Server → client request method for forwarding TypeScript queries to the
 * extension's in-process `ts.createLanguageService()` (Experiment E). */
export const TsQueryMethod = "$/verter/tsQuery" as const;

export interface TsQueryParams {
  command: string;
  arguments: Record<string, unknown>;
}

export interface StyleOverrideParam {
  index: number;
  code: string;
  sourceMap?: string;
}

export type RequestParams = {
  [RequestType.GetCompiledCode]: { uri: string };
  [RequestType.GetStatistics]: StatisticsRequestParams | undefined;
  [RequestType.GetVirtualFiles]: { uri: string };
  [RequestType.GetAnalysis]: { uri: string };
  [RequestType.GetProjectOverview]: Record<string, never>;
  [RequestType.GetBindingTypes]: { uri: string };
  [RequestType.GetComponentParents]: { uri: string };
  [RequestType.ApplyStyleOverrides]: {
    uri: string;
    overrides: StyleOverrideParam[];
  };
  [RequestType.GetRouteTree]: Record<string, never>;
};

export type RequestResponse = {
  [RequestType.GetCompiledCode]: {
    js: { code: string; map: any | undefined };
    css: { code: string; map: any | undefined };
    wasm: { code: string; map: any | undefined };
  };
  [RequestType.GetStatistics]: StatisticsSnapshot;
  [RequestType.GetVirtualFiles]: VirtualFilesResponse;
  [RequestType.GetAnalysis]: FileAnalysisSnapshot;
  [RequestType.GetProjectOverview]: ProjectOverview;
  [RequestType.GetBindingTypes]: Record<string, string | null>;
  [RequestType.GetComponentParents]: ComponentParentsResponse;
  [RequestType.ApplyStyleOverrides]: { success: boolean };
  [RequestType.GetRouteTree]: RouteAnalysisSnapshot;
};
