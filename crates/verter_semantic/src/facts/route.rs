//! Route and SSR readiness facts.

use serde::{Deserialize, Serialize};

/// Route reachability status for a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RouteReachabilityStatus {
    /// Component is reachable via known route configuration.
    Reachable,
    /// Component is reachable only under certain conditions (guards, auth).
    Conditional,
    /// Component is not reachable from any known route.
    Unreachable,
    /// Cannot determine route reachability.
    Unknown,
}

/// SSR readiness status for a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SsrReadinessStatus {
    /// Component is SSR-compatible (no browser-only APIs in setup).
    Compatible,
    /// Component uses browser-only APIs unconditionally.
    Incompatible,
    /// Component uses browser-only APIs conditionally (guarded by `import.meta.env.SSR` etc.).
    Conditional,
    /// Cannot determine SSR readiness.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_reachability_all_variants_round_trip() {
        for status in [
            RouteReachabilityStatus::Reachable,
            RouteReachabilityStatus::Conditional,
            RouteReachabilityStatus::Unreachable,
            RouteReachabilityStatus::Unknown,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: RouteReachabilityStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn ssr_readiness_all_variants_round_trip() {
        for status in [
            SsrReadinessStatus::Compatible,
            SsrReadinessStatus::Incompatible,
            SsrReadinessStatus::Conditional,
            SsrReadinessStatus::Unknown,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: SsrReadinessStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }
}
