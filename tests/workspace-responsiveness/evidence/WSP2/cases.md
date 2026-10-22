# WSP2 evidence index

Selected case IDs: `WSP2-AC1`, `WSP2-AC2`, `WSP2-AC3`, `WSP2-AC-OWNER`, `WSP2-AC-BASIS`, `WSP2-AC-RESOURCE`, `WSP2-AC-EXPOSURE`.

Commands (run on the candidate; this file is not an execution transcript):

- `cargo nextest run -p verter_lsp --lib -E "test(demand_plan) | test(heartbeat) | test(carrier_store_pings) | test(a_carrier_publication_never_announces_provider_sync_completion) | test(statistics)"`
- `cargo nextest run -p verter_lsp --lib -E "test(optional_semantic_analysis_is_isolated_and_published_asynchronously) | test(disabled_records_nothing)"`

Retired routes: the heartbeat every 5s to every client, and the carrier-store ping after every carrier publication to every client. Real Lapce captures (WSP1L, WSP1B) log each heartbeat as an unhandled host notification. Both are now subscriptions declared in `initializationOptions.subscriptions`; the VS Code extension declares both.

AC1: the workspace scan starts only on demand (the references/rename frontier or a workspace-wide enumeration), never at initialize, didOpen or didChange; workspace-wide answers are pending until the scan settles, never complete-empty (`the_workspace_scan_starts_only_on_frontier_demand`, `workspace_wide_requests_demand_the_scan_and_answer_pending_until_it_settles`). Opening and publishing documents for a client that declared nothing puts only those open documents' diagnostics on the wire. Workspace-scale answers (project overview, component parents, route tree, statistics, audit) exist only as request handlers. The 50k-source corpus run is unavailable here.

AC2: with analysis, trace and subscriptions off: no heartbeat task or message, no carrier-store ping, no semantic task or semantic host, no statistics allocation, no trace records. Analysis-only provider requests (`getBindingTypes`) occur only on client request.

AC3: diagnostics and hover suites run without any analysis option or sidebar; both open documents receive diagnostics without a subscription.

AC-OWNER: `LspDemandPlan::from_initialize` negotiates all client demand once; `ClientSubscriptions` is the one gate for recurring notifications. No historical issue-93 repair is claimed.

AC-BASIS: no cache, mapping, snapshot or publication change.

AC-RESOURCE: work counts only; real-client paint, provider process cost, corpus outbound bytes and retained memory are unavailable.

AC-EXPOSURE: no new protocol operation.
