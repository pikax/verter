# Dependency rationale

The predecessor graph is semantic, not resource-based. No edge represents machine capacity, staffing, or a desire to serialize unrelated work.

| Node | Direct predecessors | Rationale |
|---|---|---|
| `NCF-AT-CYCLE` | NCK4, NCK6, D8, G2 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-AT-QUERY` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-AT-REDUCE` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-BD-DUP` | NCK4, NCK6, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-BD-INIT` | NCK4, NCK6, D8, IDX0 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-BD-SCOPE` | NCK4, NCK6, IDX0 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-CF-CONTEXT` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-CF-THIS` | NCK4, NCK6, D8, IDX0 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-CF-VAR` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-CO-CALL` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-CO-INFER` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-CO-OVER` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-FD-CFLOW` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-FD-DEF` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-FD-NARROW` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-JD-DEC` | NCK4, NCK6, PAR0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-JD-JS` | NCK4, NCK6, PAR0, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-JD-JSDOC` | NCK4, NCK6, PAR0, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-JF-JSX` | NCK4, NCK6, NCK5, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-JF-SVELTE` | NCK4, NCK6, NCK5, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-JF-VUE` | NCK4, NCK6, NCK5, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-MP-AUG` | NCK4, NCK6, IDX0, TCM4, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-MP-MODULE` | NCK4, NCK6, IDX0, TCM4 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-MP-PROJECT` | NCK4, NCK6, IDX0, TCM4, PUB0 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-OC-HERIT` | NCK4, NCK6, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-OC-MEM` | NCK4, NCK6, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-OC-MERGE` | NCK4, NCK6, IDX0, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-RO-ASSIGN` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-RO-EXCESS` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCF-RO-OPER` | NCK4, NCK6, D8 | Generated feature slice waits on NCK4 certification/generation, NCK6 promotion infrastructure, and only the exact semantic/framework/index facts declared by its manifest row. |
| `NCK0` | UAK1, D8, E4, G2, TCM3, TIF1, LRA0, PUB0 | Waits on accepted shared flow/query/storage, certified TypeScript observation, TypeInfo, diagnostic/action, and public result contracts before defining diagnostic authority. |
| `NCK1` | NCK0, UAI0, PAR0, IDX0 | Builds region/contribution contracts only after the checker constitution and universal parser/index identity contracts. |
| `NCK2` | NCK1, G2, H3, PUB0 | Requires region identity, same-key query production, stale-safe publication basis, and public typed outcomes. |
| `NCK3` | NCK2, D8, LRA0 | Consumes live diagnostic queries plus complete shared flow/relation/call facts and LRA action/provenance law. |
| `NCK4` | NCK3, TCM4, VIM1, PER0 | Requires a functioning rule kernel, activated external observation plane, manifest generator, and performance methodology before generating/certifying slices. |
| `NCK5` | NCK1, NCK3, TIF1, IDX0, VIM1 | Requires region/contribution contracts and the shared rule kernel before framework contributions can participate without a second checker. |
| `NCK6` | NCK4, NCK5, H2, H3, COX0, PUB0 | Arbitration is meaningful only after certification infrastructure, framework isolation, provider epochs, stale-safe publication, coexistence, and public outcomes exist. |
| `NCK7` | NCK6, PUB0, CLI2:when-opened, CLI4:when-opened | Consumer integration must consume the final shared authority/publication plan rather than reimplementing it; CLI consumers are conditional to avoid reverse dependencies. |
| `NCK8` | NCK7, NCKF0, PER0, UAO0, UAP0, BR0 | Terminal deletion/promotion waits on consumer closure, generated required-family convergence, performance, universal contracts, and successor promotion authority. |
| `NCKF0` | NCF-BD-SCOPE, NCF-BD-DUP, NCF-BD-INIT, NCF-RO-ASSIGN, NCF-RO-OPER, NCF-RO-EXCESS, NCF-CO-CALL, NCF-CO-OVER, NCF-CO-INFER, NCF-CF-CONTEXT, NCF-CF-VAR, NCF-CF-THIS, NCF-FD-NARROW, NCF-FD-DEF, NCF-FD-CFLOW, NCF-OC-MEM, NCF-OC-HERIT, NCF-OC-MERGE, NCF-MP-MODULE, NCF-MP-AUG, NCF-MP-PROJECT, NCF-AT-QUERY, NCF-AT-REDUCE, NCF-AT-CYCLE, NCF-JF-JSX, NCF-JF-VUE, NCF-JF-SVELTE, NCF-JD-JS, NCF-JD-JSDOC, NCF-JD-DEC | Generated convergence waits on every manifest-required feature slice and replaces an external “all slices complete” assertion. |
| `LSO0` | UAI0, UAP0, TCM4, H3 | The operation constitution consumes final identity/coordinate, public/capability, mapper/provider, and stale-publication laws. |
| `LSO1` | LSO0, PAR0, EMB0, B2, LRA0 | Recovery needs the authored operation law plus parser, embedded mapping, accepted B2 recovery, and diagnostic provenance contracts. |
| `LSO10` | LSO9, PER0, UAI0, UAP0, BR0 | Terminal deletion/promotion waits on exact conformance, performance, identity/public contract locks, and successor promotion. |
| `LSO2` | LSO0, IDX0, ENCL0, TIF1 | The target graph requires canonical authored operation identity, bounded index candidates, strict coordinate cutover, and TypeInfo component identity. |
| `LSO3` | LSO2 | Navigation is a pure consumer of the canonical target graph; no additional broad predecessor is needed. |
| `LSO4` | LSO2, IDX0 | Occurrences require the target graph and bounded workspace candidates, but remain independently useful before rename/edit work. |
| `LSO5` | LSO4, LRA0 | Rename policy depends on complete role-typed occurrences and action safety/provenance, but not final edit materialization. |
| `LSO6` | LSO0, LSO2, H2, TCM4, PUB0 | Completion composition needs targets, provider binding/mapper activation, and public capability/outcome contracts. |
| `LSO7` | LSO0, LSO2, H2, TCM4, PUB0 | Presentation composition uses the same target/provider/public contracts but remains independent of completion and edits. |
| `LSO8` | LSO1, LSO5, LSO6, LRA0, ENCL0 | Edit materialization waits on recovery stability, semantic rename plans, completion intents, action safety, and exact coordinate conversion. |
| `LSO9` | LSO1, LSO3, LSO4, LSO5, LSO6, LSO7, LSO8, VIM1, COX0, NCK7:when-opened | Conformance waits on every operation implementation plus VIM/COX; Native Checker consumer conformance is conditional when that product is opened. |
| `EPR0` | UAK1, CFG0, H2, PUB0, TCM4 | Policy must consume universal configuration, ProviderHub, public outcomes, and certified engine binding before any acquisition channel opens. |
| `EPR1` | EPR0, VID0 | Artifact identity/trust contract is downstream only of explicit policy and exact release identity. |
| `EPR2` | EPR1, G5 | Managed acquisition is optional, explicitly authorized, and requires the artifact/install contract plus bounded scheduler pools. |
| `EPR3` | EPR1 | Bundled shipping is optional and depends only on policy/artifact contract; it is a release channel, not a runtime lifecycle block. |
| `EPR4` | EPR1, H2, EPR2:when-opened, EPR3:when-opened | Resolution consumes validated artifacts and ProviderHub requirements; optional acquisition/bundle channels become conditional inputs only when opened. |
| `EPR5` | EPR4, H3, PUB0, COX0 | Activation waits on deterministic selection and then composes stale-safe publication, public capability truth, and coexistence. |
| `EPR6` | EPR5, VIM1, PER0, BR0, CLI4:when-opened | Terminal closure waits on activation, VIM/PER0 evidence, successor promotion, and optional CLI consumer integration. |
