# Probe-state manifests

One TOML file per framework, named `<framework>.toml` (`vue.toml`, `svelte.toml`). These files are the only
manifest input any validator reads: `ProbeStateManifest::load` / `ProbeStateManifest::validate` check the
contract, and the authority validator (`validate-probe-authorities.mjs`) checks every citation against
the validation-authority catalog and the implementation ledger.

A manifest is added by the lane that pins its corpus. Its shape:

```toml
framework = "vue"                  # vue | svelte
comparison = "structural"          # structural (comparator bound) | none
external_revision = "<40-hex SHA>" # the full pinned corpus commit

[comparator]                       # exactly when comparison = structural; equals the catalog identity
crate = "verter_vue_conformance"
path = "src/compare.rs"
function = "compare_modules"
atom = "product-identity"

[applicability]                    # an inapplicable dimension holds only owned skips
runtime = "inapplicable"
map = "inapplicable"

[[strata]]                         # a representative family: `*`-glob over the case file name;
id = "app"                         # each case belongs to the first matching stratum
pattern = "App*"
min_cases = 2

[[inventory]]                      # the complete ratified case list
case_id = "vue/<relative-path-in-corpus>"
# digest = "<64-hex SHA-256>"      # for a generated corpus

[[entries]]                        # exactly one cell per { probe_id, dimension } of every case
probe_id = "vue/<relative-path-in-corpus>"
framework = "vue"
case = "<relative-path-in-corpus>"
dimension = "Route"                # Route | Compile | Structural | Runtime | Map | Performance
expected_state = "gate"            # gate | canary | known-fail | skip
expected_class = "pass"            # exact terminal class; required except for skip, forbidden for skip
authority = "compiler.public-request-route"
atom = "route-callable"            # durable atom id of that authority
# reason = "..."                   # required for skip
```

A committed manifest never contains a classless canary, an uncited cell, or a gate whose cited atom does not
list its exact class. Observation never implies acceptance: expected classes record Verter behaviour, never
output derived from the external corpus.
