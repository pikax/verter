import { describe, expect, it } from "vitest";

import { parseTraceTimingsFromContent } from "./trace-component-corpus.mjs";

describe("parseTraceTimingsFromContent", () => {
  it("keeps the primary resolve duration and sums all root spans into trace_query_ms", () => {
    const content = [
      '[verter-meta-trace] event=start trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=/src/prose/Tabs.vue mode=Expanded"',
      '[verter-meta-trace] event=end trace=1 span=1 parent=- request=1 subrequest=1 caller=- depth=0 thread=ThreadId(1) name="resolve_component_meta" detail="owner=/src/prose/Tabs.vue mode=Expanded" dur_ms=194.667',
      '[verter-meta-trace] event=end trace=190 span=190 parent=- request=190 subrequest=190 caller=- depth=0 thread=ThreadId(1) name="extract_component_meta" detail="owner=/src/prose/Tabs.vue" dur_ms=0.564',
      '[verter-meta-trace] event=end trace=193 span=193 parent=- request=193 subrequest=193 caller=- depth=0 thread=ThreadId(1) name="build_fallthrough_eval_env_lightweight" detail="owner=/src/prose/Tabs.vue" dur_ms=9.939',
      '[verter-meta-trace] event=end trace=216 span=216 parent=- request=216 subrequest=216 caller=- depth=0 thread=ThreadId(1) name="base_eval_env" detail="owner=/src/Tabs.vue" dur_ms=0.290',
      '[verter-meta-trace] event=start trace=219 span=219 parent=- request=219 subrequest=219 caller=- depth=0 thread=ThreadId(1) name="resolve_fallthrough_surface" detail="owner=/src/Tabs.vue"',
      '[verter-meta-trace] event=end trace=219 span=220 parent=219 request=219 subrequest=220 caller=219 depth=1 thread=ThreadId(1) name="compute_component_meta_state" detail="owner=/src/Tabs.vue" dur_ms=338.871',
      '[verter-meta-trace] event=end trace=219 span=219 parent=- request=219 subrequest=219 caller=- depth=0 thread=ThreadId(1) name="resolve_fallthrough_surface" detail="owner=/src/Tabs.vue" dur_ms=355.521',
    ].join("\n");

    const timings = parseTraceTimingsFromContent(content);

    expect(timings.traceResolveMs).toBe(194.667);
    expect(timings.traceQueryMs).toBeCloseTo(560.981, 6);
  });

  it("returns nulls when the trace has no root span durations", () => {
    const timings = parseTraceTimingsFromContent(
      '[verter-meta-trace] event=point trace=1 span=2 parent=1 request=1 subrequest=2 caller=1 depth=1 thread=ThreadId(1) name="component_meta_parts" detail="owner=/src/App.vue"',
    );

    expect(timings.traceResolveMs).toBeNull();
    expect(timings.traceQueryMs).toBeNull();
  });
});
