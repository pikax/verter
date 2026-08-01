//! Machine-readable readiness records for host-spawned transports.
//!
//! A host that spawns `verter-mcp --transport http --port 0` (the VS Code
//! extension, another IDE, a test harness) must learn the OS-assigned port
//! from a STABLE record, not from human `tracing` output: tracing goes to
//! stderr, is env-filterable, and prints the REQUESTED port, none of which is
//! port identity. The HTTP transport therefore writes exactly one JSON line to
//! stdout, before anything else, once the listener is bound.
//!
//! The wire shape is a one-line JSON object keyed by [`HTTP_READY_RECORD_KEY`]:
//!
//! ```json
//! {"verterMcpHttpReady":{"port":54321,"url":"http://127.0.0.1:54321/mcp"}}
//! ```
//!
//! The TypeScript mirror parser lives in
//! `packages/vue-vscode/src/mcpServer.ts` (`parseMcpHttpReadyRecord`); both
//! sides pin the same sample literal in their unit tests so the encodings
//! cannot drift apart silently.

use serde::{Deserialize, Serialize};

/// The stdout record's single top-level key.
pub const HTTP_READY_RECORD_KEY: &str = "verterMcpHttpReady";

/// The payload of an HTTP readiness record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpReadyRecord {
    /// The OS-assigned (or requested, when non-zero) port the listener bound.
    pub port: u16,
    /// The full streamable-HTTP endpoint URL for that port.
    pub url: String,
}

#[derive(Serialize, Deserialize)]
struct HttpReadyEnvelope {
    #[serde(rename = "verterMcpHttpReady")]
    ready: HttpReadyRecord,
}

/// The canonical one-line readiness record for a bound HTTP port.
pub fn http_ready_record(port: u16) -> String {
    serde_json::to_string(&HttpReadyEnvelope {
        ready: HttpReadyRecord {
            port,
            url: format!("http://127.0.0.1:{port}/mcp"),
        },
    })
    .expect("readiness record serialization is infallible")
}

/// Parse one stdout line as a readiness record.
///
/// Returns `None` for anything that is not a well-formed record naming a
/// non-zero port — noise lines, other JSON, a record claiming port 0.
pub fn parse_http_ready_record(line: &str) -> Option<HttpReadyRecord> {
    let envelope: HttpReadyEnvelope = serde_json::from_str(line.trim()).ok()?;
    if envelope.ready.port == 0 {
        return None;
    }
    Some(envelope.ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact sample also pinned by the TypeScript mirror parser's unit
    /// test (`packages/vue-vscode/src/mcpServer.spec.ts`). Changing this
    /// encoding requires changing both pins in the same commit.
    const CROSS_LANGUAGE_SAMPLE: &str =
        r#"{"verterMcpHttpReady":{"port":54321,"url":"http://127.0.0.1:54321/mcp"}}"#;

    #[test]
    fn record_matches_the_cross_language_pinned_sample() {
        assert_eq!(http_ready_record(54321), CROSS_LANGUAGE_SAMPLE);
    }

    #[test]
    fn record_is_a_single_line() {
        assert!(!http_ready_record(6772).contains('\n'));
    }

    #[test]
    fn record_round_trips_through_the_parser() {
        let record = parse_http_ready_record(&http_ready_record(6772)).expect("round trip");
        assert_eq!(record.port, 6772);
        assert_eq!(record.url, "http://127.0.0.1:6772/mcp");
    }

    #[test]
    fn parser_accepts_surrounding_whitespace_only() {
        assert!(parse_http_ready_record(
            "  {\"verterMcpHttpReady\":{\"port\":1,\"url\":\"http://127.0.0.1:1/mcp\"}}\n"
        )
        .is_some());
    }

    #[test]
    fn parser_rejects_noise_and_malformed_records() {
        // Human tracing noise.
        assert!(
            parse_http_ready_record("2026-07-28T00:00:00Z INFO Starting Verter MCP server")
                .is_none()
        );
        // Other JSON.
        assert!(parse_http_ready_record(r#"{"jsonrpc":"2.0","id":1}"#).is_none());
        // Missing key.
        assert!(parse_http_ready_record(r#"{"ready":{"port":1,"url":"u"}}"#).is_none());
        // Port 0 is not a bound port.
        assert!(parse_http_ready_record(
            r#"{"verterMcpHttpReady":{"port":0,"url":"http://127.0.0.1:0/mcp"}}"#
        )
        .is_none());
        // Out-of-range port fails u16 deserialization.
        assert!(parse_http_ready_record(
            r#"{"verterMcpHttpReady":{"port":65536,"url":"http://127.0.0.1:65536/mcp"}}"#
        )
        .is_none());
        assert!(parse_http_ready_record("").is_none());
    }
}
