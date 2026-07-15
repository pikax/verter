// @ai-generated - Synthetic package subpath declarations for flow-return edge tests.

export declare function edgeGetMap(): Map<string, { id: string }>;
export declare function edgeAssertReady(
  input: unknown,
): asserts input is { ready: true; payload: string };
export declare function edgeMaybe<T>(value: T): T | undefined;
export declare function edgePick(kind: "left"): { side: "left"; value: string };
export declare function edgePick(kind: "right"): { side: "right"; value: number };
export declare function edgeUnused(): { skipped: true };
