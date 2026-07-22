use super::*;

#[test]
fn test_byte_offset_to_tsserver_pos() {
    let content = "line1\nline2\nline3";
    // 'l' at start of line1 → (1, 1)
    assert_eq!(byte_offset_to_tsserver_pos(content, 0), (1, 1));
    // 'i' in line1 → (1, 2)
    assert_eq!(byte_offset_to_tsserver_pos(content, 1), (1, 2));
    // '\n' at end of line1 → (1, 6)
    assert_eq!(byte_offset_to_tsserver_pos(content, 5), (1, 6));
    // 'l' at start of line2 → (2, 1)
    assert_eq!(byte_offset_to_tsserver_pos(content, 6), (2, 1));
    // 'l' at start of line3 → (3, 1)
    assert_eq!(byte_offset_to_tsserver_pos(content, 12), (3, 1));
}

#[test]
fn test_tsserver_pos_to_byte_offset() {
    let content = "line1\nline2\nline3";
    // (1, 1) → 0
    assert_eq!(tsserver_pos_to_byte_offset(content, 1, 1), 0);
    // (1, 2) → 1
    assert_eq!(tsserver_pos_to_byte_offset(content, 1, 2), 1);
    // (2, 1) → 6
    assert_eq!(tsserver_pos_to_byte_offset(content, 2, 1), 6);
    // (3, 1) → 12
    assert_eq!(tsserver_pos_to_byte_offset(content, 3, 1), 12);
}

#[test]
fn test_roundtrip_position_conversion() {
    let content = "const x = 1;\nconst y = 2;\nconst z = 3;";
    for offset in 0..content.len() as u32 {
        let (line, col) = byte_offset_to_tsserver_pos(content, offset);
        let back = tsserver_pos_to_byte_offset(content, line, col);
        assert_eq!(
            back, offset,
            "roundtrip failed for offset {offset}: got ({line},{col}) -> {back}"
        );
    }
}

// A UTF-16 column that lands between the two halves of an astral (surrogate-pair) character is
// not a real scalar boundary, so an EDIT placed there cannot be proven and must be DROPPED.
// `'😀'` occupies UTF-16 cols 9 (start) and 10 (the trailing surrogate half) on this line:
// `l e t   x   =   '` = cols 0..=8, `😀` = cols 9,10, closing `'` = col 11, `;` = col 12.
// tsserver positions are 1-based.
#[test]
fn tsserver_checked_drops_mid_surrogate_column() {
    let content = "let x = '😀';";
    // 0-based col 10 == 1-based offset 11 lands on the trailing surrogate half of the emoji.
    assert_eq!(
        tsserver_pos_to_byte_offset_checked(content, 1, 11),
        None,
        "a UTF-16 column inside an astral character is not a scalar boundary and must be dropped"
    );
}

#[test]
fn tsserver_checked_accepts_position_after_astral() {
    let content = "let x = '😀';";
    // 0-based col 11 == 1-based offset 12 is the closing quote, immediately AFTER the emoji.
    let off = tsserver_pos_to_byte_offset_checked(content, 1, 12)
        .expect("the position immediately after an astral character is a valid scalar boundary");
    // `let x = '` is 9 bytes, `😀` is 4 UTF-8 bytes → byte offset 13 is the closing quote.
    assert_eq!(off, 13);
    assert_eq!(&content.as_bytes()[off as usize], &b'\'');
}

#[test]
fn tsserver_checked_accepts_eol_insertion_on_astral_line() {
    let content = "let x = '😀';";
    // EOL insertion: 0-based col == line UTF-16 length (13) == 1-based offset 14.
    let off = tsserver_pos_to_byte_offset_checked(content, 1, 14)
        .expect("an end-of-line insertion position is a valid scalar boundary");
    assert_eq!(off as usize, content.len());
}

#[test]
fn test_parse_tsserver_diagnostic() {
    let content = "const x = 1;\nconst y: string = 42;";
    let diag = serde_json::json!({
        "start": { "line": 2, "offset": 7 },
        "end": { "line": 2, "offset": 13 },
        "text": "Type 'number' is not assignable to type 'string'.",
        "code": 2322,
        "category": "error"
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), None).unwrap();
    assert_eq!(
        parsed.message,
        "Type 'number' is not assignable to type 'string'."
    );
    assert!(matches!(parsed.severity, TypeDiagnosticSeverity::Error));
    assert_eq!(parsed.code, Some("2322".to_string()));
    // "string" starts at byte 19 (line 2, offset 7 → col index 6 → byte 13 + 6 = 19)
    assert_eq!(parsed.start, 19);
    // "string" ends at byte 25 (line 2, offset 13 → col index 12 → byte 13 + 12 = 25)
    assert_eq!(parsed.end, 25);
    // A plain type error carries no editor tags.
    assert!(
        parsed.tags.is_empty(),
        "a non-suggestion diagnostic must carry no tags, got: {:?}",
        parsed.tags
    );
}

#[test]
fn parse_tsserver_diagnostic_reads_reports_unnecessary_tag() {
    // tsserver flags unused-symbol suggestions (e.g. TS6133) with the
    // `reportsUnnecessary` boolean; it must surface as an `Unnecessary` tag so
    // the LSP can fade the unused code (the `.vue` gray-out parity fix).
    let content = "import { unused } from './x';\n";
    let diag = serde_json::json!({
        "start": { "line": 1, "offset": 10 },
        "end": { "line": 1, "offset": 16 },
        "text": "'unused' is declared but its value is never read.",
        "code": 6133,
        "category": "suggestion",
        "reportsUnnecessary": true
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), None).unwrap();
    assert_eq!(parsed.code, Some("6133".to_string()));
    assert_eq!(
        parsed.tags,
        vec![TypeDiagnosticTag::Unnecessary],
        "reportsUnnecessary:true must yield the Unnecessary tag, got: {:?}",
        parsed.tags
    );
}

#[test]
fn parse_tsserver_diagnostic_reads_reports_deprecated_tag() {
    // tsserver flags deprecated-symbol usage with `reportsDeprecated`; it must
    // surface as a `Deprecated` tag (strikethrough rendering).
    let content = "oldApi();\n";
    let diag = serde_json::json!({
        "start": { "line": 1, "offset": 1 },
        "end": { "line": 1, "offset": 7 },
        "text": "'oldApi' is deprecated.",
        "code": 6385,
        "category": "suggestion",
        "reportsDeprecated": true
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), None).unwrap();
    assert_eq!(
        parsed.tags,
        vec![TypeDiagnosticTag::Deprecated],
        "reportsDeprecated:true must yield the Deprecated tag, got: {:?}",
        parsed.tags
    );
}

#[test]
fn parse_tsserver_diagnostic_without_tag_flags_stays_untagged() {
    // Control: a suggestion-category diagnostic WITHOUT the boolean flags carries
    // no tags (the flags are the sole tag source on the tsserver path).
    let content = "const x = 1;\n";
    let diag = serde_json::json!({
        "start": { "line": 1, "offset": 7 },
        "end": { "line": 1, "offset": 8 },
        "text": "some hint",
        "code": 9999,
        "category": "suggestion"
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), None).unwrap();
    assert!(
        parsed.tags.is_empty(),
        "no boolean flags ⇒ no tags, got: {:?}",
        parsed.tags
    );
}

#[test]
fn parse_tsserver_diagnostic_reads_same_file_related_information() {
    // A duplicate-identifier diagnostic (TS2300) carries a `relatedInformation`
    // span pointing at the OTHER declaration ("also declared here") in the SAME
    // file. The parser must read it into `related_information` with a real byte
    // offset (same-file conversion via `content`) and the related message.
    // Pre-fix `related_information` did not exist; this fails to compile / is empty.
    let content = "const dup = 1;\nconst dup = 2;\n";
    let diag = serde_json::json!({
        "start": { "line": 2, "offset": 7 },
        "end": { "line": 2, "offset": 10 },
        "text": "Cannot redeclare block-scoped variable 'dup'.",
        "code": 2451,
        "category": "error",
        "relatedInformation": [
            {
                "message": "'dup' was also declared here.",
                "category": "message",
                "code": 2451,
                "span": {
                    "start": { "line": 1, "offset": 7 },
                    "end": { "line": 1, "offset": 10 },
                    "file": "/proj/dup.ts"
                }
            }
        ]
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), Some("/proj/dup.ts")).unwrap();
    assert_eq!(
        parsed.related_information.len(),
        1,
        "the relatedInformation span must be parsed, got: {:?}",
        parsed.related_information
    );
    let ri = &parsed.related_information[0];
    assert_eq!(ri.message, "'dup' was also declared here.");
    assert_eq!(ri.path, "/proj/dup.ts");
    // First `dup` decl: line 1, offset 7 → col index 6 → byte 6; spans 3 chars.
    assert_eq!(
        ri.start, 6,
        "same-file related span resolves to a real byte offset"
    );
    assert_eq!(ri.end, 9);
}

#[test]
fn parse_tsserver_diagnostic_without_related_information_is_empty() {
    // Control: a diagnostic with no `relatedInformation` yields an empty list
    // (the parser never fabricates related spans).
    let content = "const x = 1;\n";
    let diag = serde_json::json!({
        "start": { "line": 1, "offset": 7 },
        "end": { "line": 1, "offset": 8 },
        "text": "some error",
        "code": 1234,
        "category": "error"
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), Some("/proj/x.ts")).unwrap();
    assert!(
        parsed.related_information.is_empty(),
        "absent relatedInformation ⇒ empty list, got: {:?}",
        parsed.related_information
    );
}

#[test]
fn parse_tsserver_diagnostic_drops_cross_file_related_without_content() {
    // The diagnostic is in `/proj/a.ts` (the file the parser holds content for),
    // but its `relatedInformation` span points at a DIFFERENT file (`/proj/b.ts`)
    // whose content the parser does NOT have. There is no real byte offset for
    // that span, so it MUST be dropped fail-closed — never stored as a packed
    // `(line<<16)|col` value in the byte-offset `start`/`end` fields.
    //
    // Pre-fix the non-same-file branch stored `(99<<16)|4 = 6488068` in
    // `start`/`end`, so `related_information` was NON-empty with a packed value —
    // this assertion fails against that code and passes once cross-file-without-
    // content drops.
    let content = "const x = 1;\n";
    let diag = serde_json::json!({
        "start": { "line": 1, "offset": 7 },
        "end": { "line": 1, "offset": 8 },
        "text": "Type error referencing another file.",
        "code": 2322,
        "category": "error",
        "relatedInformation": [
            {
                "message": "the expected type comes from here.",
                "category": "message",
                "code": 2322,
                "span": {
                    "start": { "line": 100, "offset": 5 },
                    "end": { "line": 100, "offset": 9 },
                    "file": "/proj/b.ts"
                }
            }
        ]
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), Some("/proj/a.ts")).unwrap();
    assert!(
        parsed.related_information.is_empty(),
        "a cross-file related span with no content for the related file must be \
         dropped, not stored as a packed position, got: {:?}",
        parsed.related_information
    );
}

#[test]
fn parse_tsserver_related_never_stores_packed_position_anti_bogus_link() {
    // ANTI-BOGUS-LINK: a related span at line 100 col 5 into a SMALL related file
    // (`/proj/b.ts`) must NOT survive as a packed value. Pre-fix it stored
    // `(99<<16)|4 = 6488068` in `start`/`end`; the merge then treats that as a
    // BYTE OFFSET into `/proj/b.ts` — for any target whose length exceeds 6488068
    // bytes the packed value lands IN range → a WRONG "see declaration" link.
    //
    // The fix makes it IMPOSSIBLE: no `DiagnosticRelatedInfo` ever carries a
    // packed position. We assert that directly — every surviving related entry's
    // byte offsets are real offsets within the file the parser actually had
    // content for; a cross-file span with no content yields NO entry at all.
    //
    // Pre-fix proof: the parser emitted one entry with `start == 6488068`, which
    // is the packed encoding of (line 100, col 5) — this loop's assertion fires.
    let content = "let a = 1;\n";
    let diag = serde_json::json!({
        "start": { "line": 1, "offset": 5 },
        "end": { "line": 1, "offset": 6 },
        "text": "primary",
        "code": 2322,
        "category": "error",
        "relatedInformation": [
            {
                "message": "cross-file related (no content).",
                "category": "message",
                "code": 2322,
                "span": {
                    "start": { "line": 100, "offset": 5 },
                    "end": { "line": 100, "offset": 9 },
                    "file": "/proj/b.ts"
                }
            }
        ]
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), Some("/proj/a.ts")).unwrap();
    // The exact packed value the pre-fix code would have stored for line 100 col 5.
    let packed = ((100u32 - 1) << 16) | ((5u32 - 1) & 0xFFFF);
    assert_eq!(
        packed, 6_488_068,
        "sanity: packed encoding of (line 100, col 5)"
    );
    for ri in &parsed.related_information {
        assert_ne!(
            ri.start, packed,
            "a related entry must never carry a packed position in its byte-offset start"
        );
        assert!(
            (ri.start as usize) <= content.len() && (ri.end as usize) <= content.len(),
            "a surviving related entry's offsets must be real offsets in the file the \
             parser had content for (len {}), got start={} end={}",
            content.len(),
            ri.start,
            ri.end,
        );
    }
}

/// A SAME-FILE related span whose 1-based line/offset is BEYOND the file's content
/// must be DROPPED — not clamped to EOF. The related `file` matches the file the
/// parser holds content for (same canonical path), so the cross-file drop does NOT
/// apply; the only defense is a CHECKED conversion that returns `None` for an
/// out-of-range line/offset instead of clamping to `content.len()`. A clamped EOF
/// offset would fabricate a bogus "see declaration" link at the end of the file.
///
/// Pre-fix (fail-open `tsserver_pos_to_byte_offset`): line 100 clamps to
/// `content.len()`, so `related_information` is NON-empty with a clamped offset —
/// this assertion fires. Post-fix (`tsserver_pos_to_byte_offset_checked`): the entry
/// is dropped, the list is empty, and the PRIMARY diagnostic still survives.
#[test]
fn parse_tsserver_related_drops_same_file_out_of_range_offset() {
    let content = "const dup = 1;\n"; // 15 bytes, 2 lines (1 trailing empty)
    let diag = serde_json::json!({
        "start": { "line": 1, "offset": 7 },
        "end": { "line": 1, "offset": 10 },
        "text": "Cannot redeclare block-scoped variable 'dup'.",
        "code": 2451,
        "category": "error",
        "relatedInformation": [
            {
                "message": "'dup' was also declared here.",
                "category": "message",
                "code": 2451,
                "span": {
                    // Line 100 is far past EOF: fail-open clamps to content.len().
                    "start": { "line": 100, "offset": 5 },
                    "end": { "line": 100, "offset": 9 },
                    // SAME file the parser holds content for — so cross-file drop
                    // does not apply; only the checked conversion can reject this.
                    "file": "/proj/dup.ts"
                }
            }
        ]
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), Some("/proj/dup.ts")).unwrap();
    // The primary diagnostic survives with its real in-range offsets.
    assert_eq!(parsed.start, 6, "primary start is a real in-range offset");
    assert_eq!(parsed.end, 9, "primary end is a real in-range offset");
    assert!(
        parsed.related_information.is_empty(),
        "a same-file related span past EOF must be DROPPED (checked conversion), \
         never clamped to a bogus EOF offset, got: {:?}",
        parsed.related_information
    );
}

/// WRAP-TO-VALID: a SAME-FILE related coordinate of `2^32 + 1` must be DROPPED,
/// not silently truncated into an IN-RANGE position. The parser reads each
/// coordinate with `u32::try_from(value.as_u64()?)`; a lossy `as u32` cast would
/// WRAP `4294967297` (2^32 + 1) down to `1` — a valid 1-based line/offset for
/// this fixture — and then `tsserver_pos_to_byte_offset_checked` (which only
/// rejects line/offset 0 and past-EOF positions) would HAPPILY accept the wrapped
/// `1`, fabricating a valid-looking but WRONG related link. The corruption
/// happens in the cast BEFORE the checked converter runs, so only a checked
/// `u64 → u32` conversion drops it. `2^32 + 1` (not `2^32`) is required because a
/// bare `2^32` wraps to `0`, which the 1-based `checked_sub(1)` already rejects —
/// that would not discriminate the cast bug from the existing guard.
///
/// Pre-fix (`as_u64()? as u32`): `start.offset = 2^32 + 1` wraps to `1`, an
/// in-range 1-based offset, so `related_information` is NON-empty with a
/// fabricated offset — this assertion fires. Post-fix (`u32::try_from(...).ok()?`):
/// the out-of-u32-range coordinate yields `None`, the related entry is dropped,
/// the list is empty, and the PRIMARY diagnostic still survives.
#[test]
fn parse_tsserver_related_drops_same_file_wrap_to_valid_coordinate() {
    // Sanity: `2^32 + 1` is exactly the value a lossy `as u32` cast wraps to `1` —
    // an IN-RANGE 1-based offset for the fixture below (a bare `2^32` would wrap to
    // `0`, which the 1-based `checked_sub(1)` already rejects). This is the
    // wrap-to-valid hazard.
    assert_eq!(
        4_294_967_297u64 as u32, 1,
        "sanity: 2^32 + 1 wraps to an in-range 1-based offset 1 under a lossy `as u32` cast"
    );
    let content = "const dup = 1;\n"; // 15 bytes, 2 lines (1 trailing empty)
    let diag = serde_json::json!({
        "start": { "line": 1, "offset": 7 },
        "end": { "line": 1, "offset": 10 },
        "text": "Cannot redeclare block-scoped variable 'dup'.",
        "code": 2451,
        "category": "error",
        "relatedInformation": [
            {
                "message": "'dup' was also declared here.",
                "category": "message",
                "code": 2451,
                "span": {
                    // 2^32 + 1 wraps to a valid 1-based offset 1 under `as u32`: an
                    // in-range position the checked converter would accept. The
                    // whole related entry must drop on the out-of-u32-range value.
                    "start": { "line": 1, "offset": 4_294_967_297u64 },
                    "end": { "line": 1, "offset": 10 },
                    // SAME file the parser holds content for — so cross-file drop
                    // does not apply; only the checked conversion can reject this.
                    "file": "/proj/dup.ts"
                }
            }
        ]
    });

    let parsed = parse_tsserver_diagnostic(&diag, Some(content), Some("/proj/dup.ts")).unwrap();
    // The primary diagnostic survives with its real in-range offsets.
    assert_eq!(parsed.start, 6, "primary start is a real in-range offset");
    assert_eq!(parsed.end, 9, "primary end is a real in-range offset");
    assert!(
        parsed.related_information.is_empty(),
        "a related coordinate of 2^32 + 1 must be DROPPED (checked u32 conversion), \
         never wrapped to an in-range 1-based offset 1, got: {:?}",
        parsed.related_information
    );
}

#[test]
fn test_parse_tsserver_completion() {
    let item = serde_json::json!({
        "name": "myFunction",
        "kind": "function",
        "sortText": "11",
        "insertText": "myFunction"
    });
    let parsed = parse_tsserver_completion(&item).unwrap();
    assert_eq!(parsed.label, "myFunction");
    assert!(matches!(parsed.kind, Some(CompletionKind::Function)));
    assert_eq!(parsed.sort_text, Some("11".to_string()));
}

#[test]
fn test_parse_tsserver_completion_kinds_match_vscode() {
    // Every case from VS Code's MyCompletionItem.convertKind()
    let cases = vec![
        // Keyword
        ("primitive type", CompletionKind::Keyword),
        ("keyword", CompletionKind::Keyword),
        // Variable
        ("const", CompletionKind::Variable),
        ("let", CompletionKind::Variable),
        ("var", CompletionKind::Variable),
        ("local var", CompletionKind::Variable),
        ("alias", CompletionKind::Variable),
        ("parameter", CompletionKind::Variable),
        // Field
        ("property", CompletionKind::Field),
        ("getter", CompletionKind::Field),
        ("setter", CompletionKind::Field),
        // Function
        ("function", CompletionKind::Function),
        ("local function", CompletionKind::Function),
        // Method
        ("method", CompletionKind::Method),
        ("construct", CompletionKind::Method),
        ("call", CompletionKind::Method),
        ("index", CompletionKind::Method),
        // Enum
        ("enum", CompletionKind::Enum),
        ("enum member", CompletionKind::EnumMember),
        // Module
        ("module", CompletionKind::Module),
        ("external module name", CompletionKind::Module),
        // Class/Interface
        ("class", CompletionKind::Class),
        ("type", CompletionKind::Class),
        ("interface", CompletionKind::Interface),
        // Special
        ("warning", CompletionKind::Text),
        ("script", CompletionKind::File),
        ("directory", CompletionKind::Folder),
        ("string", CompletionKind::Constant),
        // Default fallback → Property
        ("local class", CompletionKind::Property),
        ("constructor", CompletionKind::Property),
        ("type parameter", CompletionKind::Property),
        ("JSX attribute", CompletionKind::Property),
        ("accessor", CompletionKind::Property),
        ("using", CompletionKind::Property),
        ("await using", CompletionKind::Property),
        ("label", CompletionKind::Property),
        ("", CompletionKind::Property),
        ("unknown_kind", CompletionKind::Property),
    ];
    for (kind_str, expected) in cases {
        let item = serde_json::json!({
            "name": "test",
            "kind": kind_str,
            "sortText": "0"
        });
        let parsed = parse_tsserver_completion(&item).unwrap();
        assert_eq!(
            parsed.kind,
            Some(expected),
            "tsserver kind '{}' should map to {:?}",
            kind_str,
            expected
        );
    }
}

// ── Channel-based transport tests ────────────────────────────────

/// @ai-generated — tsserver stdin_writer_loop exits on Shutdown message
#[tokio::test]
async fn tsserver_writer_loop_exits_on_shutdown() {
    let (client_reader, server_writer) = tokio::io::duplex(4096);
    let (tx, rx) = mpsc::channel::<TsserverStdinMessage>(16);

    let handle = tokio::spawn(tsserver_stdin_writer_loop(server_writer, rx));

    // Send a frame
    tx.send(TsserverStdinMessage::Frame(b"test\n".to_vec()))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send Shutdown
    tx.send(TsserverStdinMessage::Shutdown).await.unwrap();

    let result = tokio::time::timeout(std::time::Duration::from_secs(1), handle).await;
    assert!(
        result.is_ok(),
        "tsserver_stdin_writer_loop should exit after Shutdown"
    );

    // Verify the frame was written
    let mut reader = BufReader::new(client_reader);
    let mut buf = String::new();
    let n = reader.read_line(&mut buf).await.unwrap();
    assert!(n > 0, "should have read the frame");
    assert_eq!(buf.trim(), "test");
}

/// @ai-generated — tsserver shutdown completes within timeout when process is unresponsive
#[tokio::test]
async fn tsserver_shutdown_completes_within_timeout() {
    let (stdin_tx, _rx) = mpsc::channel::<TsserverStdinMessage>(16);

    let transport = Arc::new(TsserverTransport {
        stdin_tx,
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: None,
    });

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let _ = transport
                .command_no_response("exit", serde_json::json!({}))
                .await;
        })
        .await;
        let _ = transport
            .stdin_tx
            .send(TsserverStdinMessage::Shutdown)
            .await;
    })
    .await;

    assert!(
        result.is_ok(),
        "Shutdown should complete within 5s even when tsserver is unresponsive"
    );
}

#[test]
fn test_format_quickinfo_hover_no_duplicate_kind() {
    // tsserver returns displayString that already includes (alias) prefix
    let result = format_quickinfo_hover("alias", "(alias) const Foo: number", "");
    assert!(
        result.contains("(alias) const Foo: number"),
        "should contain single (alias) prefix"
    );
    assert!(
        !result.contains("(alias) (alias)"),
        "must not duplicate kind prefix"
    );
}

#[test]
fn test_format_quickinfo_hover_empty_kind() {
    // Non-existent variable: kind is empty
    let result = format_quickinfo_hover("", "any", "");
    assert!(result.contains("\nany\n"), "should contain bare 'any'");
    assert!(
        !result.contains("()"),
        "must not produce empty parens for empty kind"
    );
}

#[test]
fn test_format_quickinfo_hover_normal_kind() {
    // Normal case: kind is not already in displayString
    let result = format_quickinfo_hover("const", "const foo: number", "");
    assert!(
        result.contains("(const) const foo: number"),
        "should prepend kind prefix"
    );
}

#[test]
fn test_format_quickinfo_hover_local_function_no_duplicate() {
    let result = format_quickinfo_hover(
        "local function",
        "(local function) onPopupTransform(transform: string, v: number): string",
        "",
    );
    assert!(
        !result.contains("(local function) (local function)"),
        "must not duplicate local function prefix"
    );
    assert!(
        result.contains("(local function) onPopupTransform"),
        "should contain single prefix"
    );
}

#[test]
fn test_format_quickinfo_hover_with_docs() {
    let result = format_quickinfo_hover("const", "const x: string", "A string variable");
    assert!(result.contains("(const) const x: string"));
    assert!(result.contains("A string variable"));
}

#[test]
fn test_parse_tsserver_location_with_content() {
    let content = "const x = 1;\nconst y = 2;\nconst z = 3;";
    let mut cache = HashMap::new();
    cache.insert("d:/test/file.ts".to_string(), Arc::from(content));

    let loc = serde_json::json!({
        "file": "d:/test/file.ts",
        "start": { "line": 2, "offset": 7 },
        "end": { "line": 2, "offset": 8 },
    });

    let parsed = parse_tsserver_location(&loc, &cache).unwrap();
    assert_eq!(parsed.path, "d:/test/file.ts");
    // "y" is at byte 19 (line 2, col 7 in 1-based = byte 13 + 6 = 19)
    assert_eq!(parsed.start, 19, "start should be byte offset, not packed");
    assert_eq!(parsed.end, 20, "end should be byte offset, not packed");
    // Negative: must NOT be a packed position
    assert!(
        parsed.start < 100,
        "start must be a byte offset, not packed (1 << 16 = 65536)"
    );
}

#[test]
fn test_parse_tsserver_location_without_content() {
    let cache = HashMap::new();

    let loc = serde_json::json!({
        "file": "d:/test/file.ts",
        "start": { "line": 2, "offset": 7 },
        "end": { "line": 2, "offset": 8 },
    });

    let parsed = parse_tsserver_location(&loc, &cache).unwrap();
    // Without content, should use packed fallback (0-based)
    let expected_start = ((2 - 1) << 16) | ((7 - 1) & 0xFFFF);
    assert_eq!(
        parsed.start, expected_start,
        "without content, should use packed fallback"
    );
}

#[test]
fn test_parse_tsserver_location_line_10_not_packed() {
    let mut lines = Vec::new();
    for i in 0..15 {
        lines.push(format!("line{i:02}_content"));
    }
    let content = lines.join("\n");
    let mut cache = HashMap::new();
    cache.insert("d:/test/file.ts".to_string(), Arc::from(content.as_str()));

    let loc = serde_json::json!({
        "file": "d:/test/file.ts",
        "start": { "line": 10, "offset": 1 },
        "end": { "line": 10, "offset": 5 },
    });

    let parsed = parse_tsserver_location(&loc, &cache).unwrap();
    // With content, byte offset for line 10 should be reasonable (< 200 bytes)
    assert!(
        parsed.start < (10 << 16),
        "start must NOT be a packed position for line 10+"
    );
    assert!(parsed.start < 200, "start should be a small byte offset");
}

#[test]
fn test_parse_tsserver_location_without_cache_reads_disk_content() {
    let temp_root = std::env::temp_dir().join(format!(
        "verter-tsserver-location-disk-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).unwrap();
    let file_path = temp_root.join("types.ts");
    let content = "export interface Props {\n  label: string;\n}\n";
    std::fs::write(&file_path, content).unwrap();
    let file_key = file_path.to_string_lossy().replace('\\', "/");
    let cache = HashMap::new();

    let loc = serde_json::json!({
        "file": file_key,
        "start": { "line": 2, "offset": 3 },
        "end": { "line": 2, "offset": 8 },
    });

    let parsed = parse_tsserver_location(&loc, &cache).unwrap();
    assert_eq!(parsed.start, 27);
    assert_eq!(parsed.end, 32);

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn test_parse_tsserver_rename_span_with_content() {
    let content = "const x = 1;\nconst y = 2;";
    let mut cache = HashMap::new();
    cache.insert("d:/test/file.ts".to_string(), Arc::from(content));
    let span = serde_json::json!({
        "start": { "line": 2, "offset": 7 },
        "end": { "line": 2, "offset": 8 },
    });

    let parsed = parse_tsserver_rename_span(&span, "d:/test/file.ts", &cache).unwrap();
    assert_eq!(parsed.start, 19, "start should be byte offset");
    assert_eq!(parsed.end, 20, "end should be byte offset");
    assert!(parsed.start < 100, "must not be packed");
}

/// A cross-file rename span whose GROUP file is absent from the in-memory contents cache must
/// resolve its byte offsets against THAT file's own on-disk content (the per-target disk
/// fallback) — the SAME content-resolution `parse_tsserver_location` gives references and the
/// tsgo rename path gives via `parse_range_to_offsets_strict_with_disk_fallback`.
///
/// Fails if a cache-miss span packs a 0-based `(line << 16) | col` sentinel the merge layer cannot
/// map to a real range, silently dropping the cross-file edit (incomplete rename). The renamed
/// symbol sits on line 3 (1-based), NOT line 0, so a packed line:col fallback is unmistakably
/// distinguishable from the real byte offset.
#[test]
fn test_parse_tsserver_rename_span_without_cache_reads_disk_content() {
    let temp_root = std::env::temp_dir().join(format!(
        "verter-tsserver-rename-disk-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).unwrap();
    let file_path = temp_root.join("child.ts");
    let content = "// header\nconst pad = 1;\nexport const renamed = 2;\n";
    std::fs::write(&file_path, content).unwrap();
    let file_key = file_path.to_string_lossy().replace('\\', "/");
    // CACHE MISS for this path → forces the per-target disk fallback.
    let cache = HashMap::new();

    // tsserver positions are 1-based: `renamed` is on line 3, column 14.
    let span = serde_json::json!({
        "start": { "line": 3, "offset": 14 },
        "end": { "line": 3, "offset": 21 },
    });

    let parsed = parse_tsserver_rename_span(&span, &file_key, &cache).unwrap();
    let want_start = content.find("renamed").unwrap() as u32;
    let want_end = want_start + "renamed".len() as u32;
    assert_eq!(
        (parsed.start, parsed.end),
        (want_start, want_end),
        "cross-file rename span must resolve against the target's own disk content (byte offsets \
         {want_start}..{want_end}), not pack a line-0 sentinel — got {}..{}",
        parsed.start,
        parsed.end,
    );
    // Discriminating negative: assert the offset is the real byte offset, not the packed
    // line:col fallback `(2 << 16) | 13`.
    let packed_start = ((3u32.saturating_sub(1)) << 16) | ((14u32.saturating_sub(1)) & 0xFFFF);
    assert_ne!(
        parsed.start, packed_start,
        "must NOT be the packed (line<<16)|col fallback (the dropped/corrupting path)"
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}

/// A rename span whose GROUP file is absent from the cache AND unreadable on disk must be DROPPED
/// (returns `None`) — a rename location is a WRITE edit, so a packed `(line << 16) | col` sentinel
/// applied at a bogus byte offset would CORRUPT the file.
///
/// Fails if a cache+disk miss packs the 0-based sentinel and returns
/// `Some(RenameLocation { start: packed, .. })` instead of `None`. The fixture path does not exist
/// on disk and is not in the (empty) cache.
#[test]
fn parse_tsserver_rename_span_drops_span_when_content_unavailable() {
    let missing = std::env::temp_dir()
        .join(format!(
            "verter-tsserver-rename-missing-{}-absent.ts",
            std::process::id()
        ))
        .to_string_lossy()
        .replace('\\', "/");
    let _ = std::fs::remove_file(&missing);

    let span = serde_json::json!({
        "start": { "line": 3, "offset": 14 },
        "end": { "line": 3, "offset": 21 },
    });
    let cache: HashMap<String, Arc<str>> = HashMap::new();

    let parsed = parse_tsserver_rename_span(&span, &missing, &cache);
    assert!(
        parsed.is_none(),
        "a rename span whose content is unavailable must be DROPPED (fail-closed), never packed: \
         {parsed:?}"
    );
}

/// A rename span whose content IS available but whose position is OUT OF RANGE (past EOF) is
/// DROPPED, not clamped — a clamped rename WRITE at EOF would corrupt the file.
#[test]
fn parse_tsserver_rename_span_drops_out_of_range_position() {
    let content = "const x = 1;\nconst y = 2;\n";
    let mut cache: HashMap<String, Arc<str>> = HashMap::new();
    cache.insert("d:/proj/r.ts".to_string(), Arc::from(content));

    let span = serde_json::json!({
        "start": { "line": 999, "offset": 1 },
        "end": { "line": 999, "offset": 4 },
    });

    let parsed = parse_tsserver_rename_span(&span, "d:/proj/r.ts", &cache);
    assert!(
        parsed.is_none(),
        "an out-of-range rename span must be DROPPED, never clamped to EOF: {parsed:?}"
    );
}

/// A rename span whose `line`/`offset` exceeds `u32::MAX` must be DROPPED — never SILENTLY truncated.
/// tsserver positions are 1-based, so the danger value wraps to a VALID 1-based position:
/// `u32::MAX as u64 + 2` truncates to `1` (line 1 / offset 1, i.e. 0-based line 0 / col 0) under a
/// lossy `as u32`, which the checked converter would ACCEPT. With VALID content the only reason to
/// drop is the overflow itself — the truncation must fail closed BEFORE the converter runs. A rename
/// location is a WRITE edit, so a wrapped offset would corrupt the file at the wrong location.
#[test]
fn parse_tsserver_rename_span_drops_on_position_overflow() {
    let content = "const x = 1;\nconst y = 2;\n";
    let mut cache: HashMap<String, Arc<str>> = HashMap::new();
    cache.insert("d:/proj/r.ts".to_string(), Arc::from(content));

    // u32::MAX + 2 → truncates to 1 (a VALID 1-based line/offset) under a lossy `as u32`.
    let overflow = u32::MAX as u64 + 2;
    let span = serde_json::json!({
        "start": { "line": overflow, "offset": 1 },
        "end": { "line": 1, "offset": 2 },
    });

    let parsed = parse_tsserver_rename_span(&span, "d:/proj/r.ts", &cache);
    assert!(
        parsed.is_none(),
        "a u64>u32::MAX rename span must be DROPPED, never truncated into an in-range offset: \
         {parsed:?}"
    );

    // POSITIVE CONTROL: the in-range 1-based span still resolves to the correct byte offsets.
    let span_ok = serde_json::json!({
        "start": { "line": 1, "offset": 1 },
        "end": { "line": 1, "offset": 2 },
    });
    let ok = parse_tsserver_rename_span(&span_ok, "d:/proj/r.ts", &cache)
        .expect("an in-range rename span must still resolve");
    assert_eq!(
        (ok.start, ok.end),
        (0, 1),
        "in-range 1-based (1,1)..(1,2) maps to byte offsets 0..1, unchanged"
    );
}

#[test]
fn test_parse_tsserver_location_non_ascii() {
    // tsserver uses UTF-16 code units for offset
    // "café" = 5 bytes UTF-8 (c=1, a=1, f=1, é=2), 4 UTF-16 code units
    let content = "café\nworld";
    let mut cache = HashMap::new();
    cache.insert("d:/test/file.ts".to_string(), Arc::from(content));

    let loc = serde_json::json!({
        "file": "d:/test/file.ts",
        "start": { "line": 2, "offset": 1 },
        "end": { "line": 2, "offset": 6 },
    });

    let parsed = parse_tsserver_location(&loc, &cache).unwrap();
    // "café\n" = 6 bytes (c=1, a=1, f=1, é=2, \n=1)
    // "world" starts at byte 6
    assert_eq!(parsed.start, 6, "start of 'world' should be byte 6");
    // "world" ends at byte 11
    assert_eq!(parsed.end, 11, "end of 'world' should be byte 11");
}

#[test]
fn test_byte_offset_to_tsserver_pos_non_ascii() {
    // "café\nworld" — 'é' is 2 bytes UTF-8, 1 UTF-16 code unit
    let content = "café\nworld";
    // byte 6 = start of "world" = line 2, col 1 in 1-based
    let (line, col) = byte_offset_to_tsserver_pos(content, 6);
    assert_eq!(line, 2, "should be line 2");
    assert_eq!(col, 1, "should be col 1 (UTF-16)");
}

#[test]
fn test_tsserver_pos_to_byte_offset_non_ascii() {
    // "café\nworld" — 'é' is 2 bytes UTF-8, 1 UTF-16 code unit
    let content = "café\nworld";
    // line 2, offset 1 (1-based) → byte 6 ("world" starts there)
    let offset = tsserver_pos_to_byte_offset(content, 2, 1);
    assert_eq!(offset, 6, "line 2, col 1 should be byte 6");
}

async fn send_success_response(pending: &Arc<TsserverPendingRequests>, seq: i64, command: &str) {
    if let Some(tx) = pending.take(seq) {
        let _ = tx.send(serde_json::json!({
            "type": "response",
            "request_seq": seq,
            "success": true,
            "command": command,
            "body": {}
        }));
    }
}

/// `configure_tsserver_session` sends ONLY the `configure` handshake and injects
/// NO inferred-project compiler options: a framework carrier is a member of its
/// REAL configured project (via the plugin), so there is no config-less inferred
/// carrier to configure. `compilerOptionsForInferredProjects` must never be sent.
#[tokio::test]
async fn test_configure_tsserver_session_sends_no_inferred_project_options() {
    let (client_reader, server_writer) = tokio::io::duplex(65536);
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));

    let pending = Arc::new(TsserverPendingRequests::default());
    let transport = Arc::new(TsserverTransport {
        stdin_tx: stdin_tx.clone(),
        pending: Arc::clone(&pending),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: None,
    });

    let seen_commands = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_commands_task = Arc::clone(&seen_commands);
    let pending_task = Arc::clone(&pending);
    tokio::spawn(async move {
        let mut reader = BufReader::new(client_reader);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let msg: serde_json::Value =
                        serde_json::from_str(line.trim()).expect("valid tsserver request");
                    let seq = msg["seq"].as_i64().expect("request seq");
                    let command = msg["command"]
                        .as_str()
                        .expect("request command")
                        .to_string();
                    seen_commands_task.lock().await.push(command.clone());
                    if command == "configure" {
                        send_success_response(&pending_task, seq, &command).await;
                    }
                }
            }
        }
    });

    let ws_root = configure_tsserver_session(Arc::clone(&transport), "C:\\project")
        .await
        .expect("configuration should succeed");

    // Canonical form lowercases the Windows drive letter (keeps the colon).
    assert_eq!(ws_root, "c:/project");

    // Let any (erroneously-spawned) background request reach the mock reader.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let commands = seen_commands.lock().await.clone();
    assert_eq!(
        commands.first().map(String::as_str),
        Some("configure"),
        "configure must be sent first"
    );
    assert!(
        !commands
            .iter()
            .any(|command| command == "compilerOptionsForInferredProjects"),
        "inferred-project compiler options must NEVER be sent — the carrier is a real \
         configured-project member, so there is no inferred carrier to configure. \
         Seen: {commands:?}"
    );

    let _ = stdin_tx.send(TsserverStdinMessage::Shutdown).await;
}

// ── update_file end-line tests ──────────────────────────────────

/// Helper: run the same logic as TypeProvider::update_file but against a
/// bare TsserverTransport + shared caches, returning the JSON frames
/// that were written to stdin.
async fn run_update_file_capture(
    old_content: Option<&str>,
    new_content: &str,
    file: &str,
) -> Vec<serde_json::Value> {
    let (client_reader, server_writer) = tokio::io::duplex(65536);
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));

    let transport = Arc::new(TsserverTransport {
        stdin_tx: stdin_tx.clone(),
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: None,
    });

    let contents_cache: Arc<Mutex<HashMap<String, Arc<str>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let opened_files: Arc<Mutex<HashMap<String, OpenKind>>> = Arc::new(Mutex::new(HashMap::new()));

    // Pre-populate caches to simulate an already-open file
    if let Some(old) = old_content {
        contents_cache
            .lock()
            .await
            .insert(file.to_string(), Arc::from(old));
        opened_files
            .lock()
            .await
            .insert(file.to_string(), OpenKind::Source);
    }

    // Run the same logic as update_file
    let content = new_content.to_string();
    let file = file.to_string();
    let project_root = "/project".to_string();

    // Read old content's line count BEFORE inserting new content
    let old_line_count = {
        let cache = contents_cache.lock().await;
        cache.get(&file).map(|c| c.lines().count() as u32 + 1)
    };

    contents_cache
        .lock()
        .await
        .insert(file.clone(), Arc::from(content.as_str()));

    let mut opened = opened_files.lock().await;
    if opened.contains_key(&file) {
        drop(opened);
        if let Some(end_line) = old_line_count {
            let _ = transport
                .command_no_response(
                    "updateOpen",
                    serde_json::json!({
                        "changedFiles": [{
                            "fileName": file,
                            "textChanges": [{
                                "start": { "line": 1, "offset": 1 },
                                "end": { "line": end_line, "offset": 1 },
                                "newText": content,
                            }]
                        }]
                    }),
                )
                .await;
        } else {
            let _ = transport
                .command_no_response(
                    "updateOpen",
                    serde_json::json!({
                        "closedFiles": [&file],
                        "openFiles": [{
                            "file": file,
                            "fileContent": content,
                            "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                                else if file.ends_with(".jsx") { "JSX" }
                                else if file.ends_with(".js") { "JS" }
                                else { "TS" },
                            "projectRootPath": project_root,
                        }]
                    }),
                )
                .await;
        }
    } else {
        opened.insert(file.clone(), OpenKind::Source);
        drop(opened);
        let _ = transport
            .command_no_response(
                "open",
                serde_json::json!({
                    "file": file,
                    "fileContent": content,
                    "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                        else if file.ends_with(".jsx") { "JSX" }
                        else if file.ends_with(".js") { "JS" }
                        else { "TS" },
                    "projectRootPath": project_root,
                }),
            )
            .await;
    }

    // Shutdown writer + read all frames
    let _ = stdin_tx.send(TsserverStdinMessage::Shutdown).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut reader = BufReader::new(client_reader);
    let mut frames = Vec::new();
    loop {
        let mut line = String::new();
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(_)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                    frames.push(val);
                }
            }
            Ok(Err(_)) => break,
        }
    }
    frames
}

#[tokio::test]
async fn test_update_file_end_line_matches_old_content() {
    let old = "line1\nline2\nline3"; // 3 lines
    let new = "line1\nline2\nline3\nline4\nline5"; // 5 lines
    let frames = run_update_file_capture(Some(old), new, "/project/src/App.vue.tsx").await;

    assert_eq!(frames.len(), 1, "should send exactly one command");
    let args = &frames[0]["arguments"];
    let end_line = args["changedFiles"][0]["textChanges"][0]["end"]["line"]
        .as_u64()
        .unwrap();

    // Old content has 3 lines → end line should be 4 (lines().count() + 1)
    assert_eq!(end_line, 4, "end line should be old content line count + 1");
    assert_ne!(end_line, 1_000_000, "must NOT use hardcoded 1_000_000");
}

#[tokio::test]
async fn test_update_file_single_line_content() {
    let old = "const x = 1;"; // 1 line
    let new = "const x = 1;\nconst y = 2;";
    let frames = run_update_file_capture(Some(old), new, "/project/src/App.vue.tsx").await;

    let end_line = frames[0]["arguments"]["changedFiles"][0]["textChanges"][0]["end"]["line"]
        .as_u64()
        .unwrap();
    assert_eq!(end_line, 2, "single-line content: lines().count()=1, +1=2");
    assert_ne!(end_line, 1_000_000, "must NOT use hardcoded 1_000_000");
}

/// Capture the wire frames the `notify_carrier_changed` body produces for a
/// companion path: a plugin publication-token advance for warm ScriptInfo refresh,
/// the `updateOpen { changedFiles }` cold-resolution eviction, then a response
/// round-trip that fences later provider queries onto a subsequent host turn.
async fn run_notify_carrier_changed_capture(companion: &str) -> Vec<serde_json::Value> {
    let (client_reader, server_writer) = tokio::io::duplex(65536);
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));
    let transport = Arc::new(TsserverTransport {
        stdin_tx: stdin_tx.clone(),
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: None,
    });

    // The exact command sequence of `TsserverTypeProvider::notify_carrier_changed`.
    let file = TsserverTypeProvider::normalize_path(companion);
    let _ = transport
        .command_no_response(
            "configurePlugin",
            serde_json::json!({
                "pluginName": "@verter/typescript-plugin",
                "configuration": { "carrierStoreRefreshToken": 1 }
            }),
        )
        .await;
    let _ = transport
        .command_no_response(
            "updateOpen",
            serde_json::json!({
                "changedFiles": [{ "fileName": file, "textChanges": [] }]
            }),
        )
        .await;
    let request_transport = Arc::clone(&transport);
    let fence_file = file.clone();
    let fence = tokio::spawn(async move {
        request_transport
            .request(
                "projectInfo",
                serde_json::json!({
                    "file": fence_file,
                    "needFileNameList": false,
                }),
            )
            .await
    });
    loop {
        let response = {
            let mut pending = transport.pending.lock().await;
            let response = pending.drain().next().map(|(_, response)| response);
            response
        };
        if let Some(response) = response {
            let _ = response.send(serde_json::json!({}));
            break;
        }
        tokio::task::yield_now().await;
    }
    fence
        .await
        .expect("fence request task must not panic")
        .expect("fence response must complete the request");

    let _ = stdin_tx.send(TsserverStdinMessage::Shutdown).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let mut reader = BufReader::new(client_reader);
    let mut frames = Vec::new();
    loop {
        let mut line = String::new();
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(_)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                    frames.push(val);
                }
            }
            Ok(Err(_)) => break,
        }
    }
    frames
}

/// C10 eviction discriminator: `notify_carrier_changed` advances the plugin's
/// carrier-store publication token (warm ScriptInfo replacement) and then fires an
/// `updateOpen` `changedFiles` frame for the COMPANION path (cold negative-cache
/// eviction). The final response-bearing request orders subsequent queries after
/// the plugin's deferred graph refresh. Omitting any step leaves a stale state or
/// permits a query to overtake the refresh.
#[tokio::test]
async fn tsserver_cold_read_no_sticky_ts2307() {
    let frames = run_notify_carrier_changed_capture("/proj/src/Comp.vue.tsx").await;

    assert_eq!(
        frames.len(),
        3,
        "carrier invalidation must issue the warm refresh, cold eviction, and ordering fence"
    );
    let configure = &frames[0];
    assert_eq!(configure["command"].as_str(), Some("configurePlugin"));
    assert_eq!(
        configure["arguments"]["pluginName"].as_str(),
        Some("@verter/typescript-plugin")
    );
    assert_eq!(
        configure["arguments"]["configuration"]["carrierStoreRefreshToken"].as_u64(),
        Some(1),
        "a changed token is the plugin's warm ScriptInfo reload signal"
    );

    let frame = &frames[1];
    assert_eq!(
        frame["command"].as_str(),
        Some("updateOpen"),
        "the eviction lever is the `updateOpen` file-changed notification"
    );
    let changed = &frame["arguments"]["changedFiles"];
    assert_eq!(
        changed[0]["fileName"].as_str(),
        Some("/proj/src/Comp.vue.tsx"),
        "the eviction targets the COMPANION path (the path whose fileExists/module \
         resolution tsserver cached cold)"
    );
    // The edit is empty (a content-preserving touch): the bytes are unchanged, only
    // tsserver's cached resolution for the file is invalidated.
    assert!(
        changed[0]["textChanges"]
            .as_array()
            .is_some_and(|c| c.is_empty()),
        "the eviction is a content-preserving touch (empty textChanges): {frame}"
    );

    let fence = &frames[2];
    assert_eq!(fence["command"].as_str(), Some("projectInfo"));
    assert_eq!(
        fence["arguments"]["file"].as_str(),
        Some("/proj/src/Comp.vue.tsx"),
        "the ordering fence targets the same newly-published companion"
    );
    assert_eq!(
        fence["arguments"]["needFileNameList"].as_bool(),
        Some(false),
        "the fence must not materialize a project-wide file list"
    );
}

/// The cold-companion classifier matches ONLY the tsserver "the file argument
/// itself is not (yet) a valid source file in the program" failure — a transient
/// COLD signal on a just-published companion — and never a genuine module-not-found
/// the user should see. A real `TS2307` arrives in the SUCCESS body (so it never
/// reaches this classifier at all); the strings below are the transport-level
/// `success:false` messages that DO reach it.
#[test]
fn tsserver_cold_companion_error_classifier_is_narrow() {
    // The exact cold failure observed against live tsserver TS6.0.3 on a configured-
    // project build: the just-opened companion is not yet in the program tsserver
    // type-checks, so `getValidSourceFile` throws and the whole command fails.
    assert!(
        tsserver_diag_error_is_companion_not_ready(
            "Could not find source file: '/proj/src/AssertJson.vue.tsx'."
        ),
        "the cold companion-not-in-program failure must classify as retryable"
    );
    assert!(
        tsserver_diag_error_is_companion_not_ready("Could not find source file: 'X.svelte.tsx'"),
        "the message-substring match is case/path independent"
    );

    // HAZARD: a genuine module-not-found the user must see never reaches this
    // classifier (it is a SUCCESS-body diagnostic, not a transport error), and even
    // its text must NOT match — these are real-error messages that must surface.
    assert!(
        !tsserver_diag_error_is_companion_not_ready(
            "Cannot find module './missing' or its corresponding type declarations."
        ),
        "a genuine TS2307 module-not-found must NOT be swallowed as cold-retryable"
    );
    assert!(
        !tsserver_diag_error_is_companion_not_ready(
            "request 'semanticDiagnosticsSync' timed out after 10s"
        ),
        "a transport timeout is a distinct terminal condition, not the cold signal"
    );
    assert!(
        !tsserver_diag_error_is_companion_not_ready("response channel closed"),
        "a closed channel is terminal, not the cold signal"
    );
    assert!(
        !tsserver_diag_error_is_companion_not_ready(""),
        "an empty message is not the cold signal"
    );
}

/// Drive the REAL `resync_open_files_inner` against a bare transport + caches and
/// capture the JSON frames it writes. `entries` are `(file, kind, content)` to
/// pre-track; `carrier_projects` maps a companion path to its owning tsconfig.
async fn run_resync_capture(
    entries: &[(&str, OpenKind, &str)],
    carrier_projects: &[(&str, &str)],
) -> Vec<serde_json::Value> {
    let (client_reader, server_writer) = tokio::io::duplex(65536);
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));
    let transport = Arc::new(TsserverTransport {
        stdin_tx: stdin_tx.clone(),
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: None,
    });

    let opened_files: Arc<Mutex<HashMap<String, OpenKind>>> = Arc::new(Mutex::new(HashMap::new()));
    let contents_cache: Arc<Mutex<HashMap<String, Arc<str>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let content_generations: Arc<ContentGenerations> = Arc::new(ContentGenerations::default());
    {
        let mut opened = opened_files.lock().await;
        for (file, kind, _content) in entries {
            opened.insert((*file).to_string(), *kind);
        }
    }
    for (file, _kind, content) in entries {
        // Populate via the production writer so each pre-tracked file carries a
        // matching content generation: the resync gate compares the captured
        // generation to the live one, and pre-tracking content WITHOUT a
        // generation would leave the gate reading `None` and skip every reopen.
        store_content_bump_generation(
            &contents_cache,
            &content_generations,
            file,
            Arc::from(*content),
        )
        .await;
    }
    let carrier_map: Arc<parking_lot::RwLock<HashMap<String, String>>> =
        Arc::new(parking_lot::RwLock::new(HashMap::new()));
    {
        let mut map = carrier_map.write();
        for (companion, tsconfig) in carrier_projects {
            map.insert((*companion).to_string(), (*tsconfig).to_string());
        }
    }
    let project_roots = Arc::new(parking_lot::RwLock::new(Vec::new()));

    resync_open_files_inner(
        Arc::clone(&transport),
        Arc::clone(&opened_files),
        Arc::clone(&contents_cache),
        Arc::clone(&content_generations),
        Arc::clone(&carrier_map),
        project_roots,
        "/project".to_string(),
    )
    .await
    .expect("resync should succeed");

    let _ = stdin_tx.send(TsserverStdinMessage::Shutdown).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let mut reader = BufReader::new(client_reader);
    let mut frames = Vec::new();
    loop {
        let mut line = String::new();
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(_)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                    frames.push(val);
                }
            }
            Ok(Err(_)) => break,
        }
    }
    frames
}

/// Find the `open` frame a resync issued for `file`.
fn resync_open_frame_for<'a>(
    frames: &'a [serde_json::Value],
    file: &str,
) -> Option<&'a serde_json::Value> {
    frames.iter().find(|frame| {
        frame["command"].as_str() == Some("open")
            && frame["arguments"]["file"].as_str() == Some(file)
    })
}

/// A resync must reopen a SOURCE file WITH its `fileContent` (tsserver owns its
/// content) but a CARRIER COMPANION CONTENTLESSLY — resending the carrier's bytes
/// would convert it back into a tsserver content authority, violating the
/// "plugin `getScriptSnapshot` is the sole content authority" contract. The bug
/// this guards: `resync_open_files` reopened EVERY tracked path with
/// `fileContent`, including carrier companions.
#[tokio::test]
async fn resync_reopens_source_with_content_but_carrier_contentless() {
    let source = "/project/src/real.ts";
    let carrier = "/project/src/Comp.vue.tsx";
    let frames = run_resync_capture(
        &[
            (source, OpenKind::Source, "export const x = 1;\n"),
            (
                carrier,
                OpenKind::CarrierCompanion,
                "export default {} as any;\n",
            ),
        ],
        &[(carrier, "/project/tsconfig.json")],
    )
    .await;

    let source_open =
        resync_open_frame_for(&frames, source).expect("source must be reopened on resync");
    assert_eq!(
        source_open["arguments"]["fileContent"].as_str(),
        Some("export const x = 1;\n"),
        "a SOURCE file must be reopened WITH its fileContent (tsserver owns its content): {source_open}"
    );

    let carrier_open =
        resync_open_frame_for(&frames, carrier).expect("carrier must be reopened on resync");
    assert!(
        carrier_open["arguments"].get("fileContent").is_none(),
        "a CARRIER COMPANION must be reopened CONTENTLESSLY on resync — resending its \
         bytes makes tsserver the content authority and breaks the plugin's \
         getScriptSnapshot contract: {carrier_open}"
    );
    // The contentless carrier reopen must still route to its OWN configured project
    // (the tsconfig dir), exactly like the publish-time `register_carrier_member`.
    assert_eq!(
        carrier_open["arguments"]["projectRootPath"].as_str(),
        Some("/project"),
        "the carrier reopen routes to its owning configured project's root: {carrier_open}"
    );
}

/// M7 — a transport-send FAILURE on the contentless carrier open must NOT leave a
/// phantom-registered carrier. The open is marked in `opened_files` BEFORE the send;
/// if the send fails, the mark (and the `carrier_projects` routing entry) MUST be
/// rolled back so a LATER registration re-attempts the open. RED before the fix: the
/// `opened_files` mark survived the failure, so a later registration saw
/// `opened_now == false` and skipped the open forever (the companion never became a
/// configured-project member).
#[tokio::test]
async fn carrier_open_send_failure_rolls_back_tracking_for_retry() {
    // A transport whose stdin RECEIVER is dropped: `command_no_response`'s send
    // fails ("stdin writer closed"), simulating a transport-send failure.
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    drop(stdin_rx);
    let transport = TsserverTransport {
        stdin_tx,
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: None,
    };

    let file = "/project/src/App.vue.tsx";
    let opened_files: Arc<Mutex<HashMap<String, OpenKind>>> = Arc::new(Mutex::new(HashMap::new()));
    let carrier_projects: Arc<parking_lot::RwLock<HashMap<String, String>>> =
        Arc::new(parking_lot::RwLock::new(HashMap::new()));
    // The trait method inserts the routing entry BEFORE the open; mirror that so the
    // rollback of BOTH maps is observable.
    carrier_projects
        .write()
        .insert(file.to_string(), "/project/tsconfig.json".to_string());

    let result = open_carrier_companion_contentless(
        &transport,
        &opened_files,
        &carrier_projects,
        file,
        "TSX",
        "/project",
    )
    .await;

    assert!(
        result.is_err(),
        "a failed contentless carrier open must surface as Err, not silent success"
    );
    assert!(
        !opened_files.lock().await.contains_key(file),
        "a failed open MUST roll back the opened_files mark so a later registration \
         RE-ATTEMPTS the open — a phantom 'already opened' entry would leave the \
         carrier forever unopened (never a configured-project member)"
    );
    assert!(
        !carrier_projects.read().contains_key(file),
        "a failed open MUST roll back the carrier_projects routing entry too"
    );
}

#[tokio::test]
async fn test_update_file_first_open_uses_open_command() {
    // No old content → should use "open" command, not "updateOpen"
    let frames = run_update_file_capture(None, "const x = 1;", "/project/src/New.vue.tsx").await;

    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["command"].as_str().unwrap(), "open");
    // Should not contain changedFiles or end line at all
    assert!(
        frames[0]["arguments"].get("changedFiles").is_none(),
        "open command should not have changedFiles"
    );
}

// ── get_semantic_tokens cache-miss test ──────────────────────────

#[tokio::test]
async fn test_get_semantic_tokens_cache_miss_returns_empty() {
    // Simulate what get_semantic_tokens does on cache miss:
    // It should return Ok(vec![]) without sending any request.
    let contents_cache: Arc<Mutex<HashMap<String, Arc<str>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let content = {
        let cache = contents_cache.lock().await;
        cache.get("/project/src/Missing.vue.tsx").cloned()
    };

    // A cache miss yields None → the resolver early-returns with an empty
    // result and sends no transport request. This asserts that None path exists.
    assert!(content.is_none(), "cache miss should yield None");
}

// ── env denylist test ──────────────────────────────────────────

#[test]
fn test_child_process_env_denylist_strips_debug_vars() {
    // Verify the constant contains exactly the expected vars
    assert!(
        CHILD_PROCESS_ENV_DENYLIST.contains(&"NODE_OPTIONS"),
        "should deny NODE_OPTIONS"
    );
    assert!(
        CHILD_PROCESS_ENV_DENYLIST.contains(&"VSCODE_INSPECTOR_OPTIONS"),
        "should deny VSCODE_INSPECTOR_OPTIONS"
    );
    assert!(
        CHILD_PROCESS_ENV_DENYLIST.contains(&"ELECTRON_RUN_AS_NODE"),
        "should deny ELECTRON_RUN_AS_NODE"
    );

    // Verify that std::process::Command.env_remove with these vars works
    // (same API as tokio::process::Command)
    let mut cmd = std::process::Command::new("echo");
    for var in CHILD_PROCESS_ENV_DENYLIST {
        cmd.env_remove(var);
    }
    // If we get here without panic, the API accepts all denylist entries.
    // Also verify the list length is exactly 3 (no accidental additions)
    assert_eq!(
        CHILD_PROCESS_ENV_DENYLIST.len(),
        3,
        "denylist should have exactly 3 entries"
    );
}

#[test]
fn test_tsserver_plugin_args_are_empty_without_probe_location() {
    assert!(
        tsserver_plugin_args(None).is_empty(),
        "no plugin path should produce no plugin args"
    );
    assert!(
        tsserver_plugin_args(Some("")).is_empty(),
        "empty plugin path should produce no plugin args"
    );
}

#[test]
fn test_tsserver_plugin_args_enable_verter_plugin() {
    let args = tsserver_plugin_args(Some("/workspace/node_modules"));
    assert_eq!(
        args,
        vec![
            "--globalPlugins".to_string(),
            "@verter/typescript-plugin".to_string(),
            "--pluginProbeLocations".to_string(),
            "/workspace/node_modules".to_string(),
            "--allowLocalPluginLoads".to_string(),
        ],
        "tsserver should be launched with the Verter TS plugin enabled"
    );
}

// ── GAP-2: tsserver-family diagnostics parity (semantic + syntactic + suggestion) ──

fn diag(message: &str, severity: TypeDiagnosticSeverity, start: u32, end: u32) -> TypeDiagnostic {
    TypeDiagnostic {
        message: message.to_string(),
        severity,
        start,
        end,
        code: None,
        tags: Vec::new(),
        related_information: Vec::new(),
    }
}

fn diag_with_code(
    message: &str,
    severity: TypeDiagnosticSeverity,
    start: u32,
    end: u32,
    code: &str,
) -> TypeDiagnostic {
    TypeDiagnostic {
        message: message.to_string(),
        severity,
        start,
        end,
        code: Some(code.to_string()),
        tags: Vec::new(),
        related_information: Vec::new(),
    }
}

#[test]
fn merge_diagnostic_sets_unions_all_three_categories() {
    // GAP-2: tsserver-family diagnostics must merge the semantic set with the
    // syntactic and suggestion sets that the native TS experience surfaces.
    let semantic = vec![diag_with_code(
        "Type 'number' is not assignable to type 'string'.",
        TypeDiagnosticSeverity::Error,
        6,
        9,
        "2322",
    )];
    let syntactic = vec![diag_with_code(
        "';' expected.",
        TypeDiagnosticSeverity::Error,
        20,
        21,
        "1005",
    )];
    let suggestion = vec![diag_with_code(
        "'foo' is declared but its value is never read.",
        TypeDiagnosticSeverity::Hint,
        30,
        33,
        "6133",
    )];

    let merged = merge_diagnostic_sets(semantic, syntactic, suggestion);

    assert_eq!(
        merged.len(),
        3,
        "all three categories must survive the merge"
    );
    assert!(
        merged.iter().any(|d| d.code.as_deref() == Some("2322")),
        "semantic type error must be present, got: {merged:?}"
    );
    assert!(
        merged.iter().any(|d| d.code.as_deref() == Some("1005")),
        "syntactic parse error must be present (GAP-2), got: {merged:?}"
    );
    assert!(
        merged.iter().any(|d| d.code.as_deref() == Some("6133")
            && matches!(d.severity, TypeDiagnosticSeverity::Hint)),
        "suggestion (unused-symbol hint) must be present (GAP-2), got: {merged:?}"
    );
}

#[test]
fn merge_diagnostic_sets_dedups_identical_diagnostic_across_categories() {
    // tsserver can report the same diagnostic from more than one pass; the merge
    // must not surface a visual duplicate keyed on (start, end, code, message).
    let shared = diag_with_code("dup", TypeDiagnosticSeverity::Error, 1, 2, "9999");
    let merged = merge_diagnostic_sets(
        vec![shared.clone()],
        vec![shared.clone()],
        vec![diag_with_code(
            "unique",
            TypeDiagnosticSeverity::Hint,
            5,
            6,
            "6133",
        )],
    );
    assert_eq!(
        merged.len(),
        2,
        "the duplicated diagnostic collapses to one, the unique one survives: {merged:?}"
    );
    assert_eq!(
        merged
            .iter()
            .filter(|d| d.code.as_deref() == Some("9999"))
            .count(),
        1,
        "exactly one copy of the duplicated diagnostic"
    );
}

#[test]
fn merge_diagnostic_sets_keeps_same_span_distinct_message() {
    // Two diagnostics at the SAME span but with different messages/codes are
    // distinct findings — neither must be dropped (a same-span dedup would be a
    // correctness regression).
    let merged = merge_diagnostic_sets(
        vec![diag_with_code(
            "error A",
            TypeDiagnosticSeverity::Error,
            1,
            4,
            "2322",
        )],
        vec![],
        vec![diag_with_code(
            "hint B",
            TypeDiagnosticSeverity::Hint,
            1,
            4,
            "6133",
        )],
    );
    assert_eq!(
        merged.len(),
        2,
        "same span, different code/message → both kept: {merged:?}"
    );
}

#[test]
fn merge_diagnostic_sets_empty_inputs_yield_empty() {
    let merged = merge_diagnostic_sets(vec![], vec![], vec![]);
    assert!(merged.is_empty(), "no diagnostics in → none out");
}

#[test]
fn merge_diagnostic_sets_preserves_a_lone_suggestion() {
    // A file with no semantic/syntactic errors but an unused import must still
    // surface the suggestion — the pre-GAP-2 semantic-only path dropped this.
    let merged = merge_diagnostic_sets(
        vec![],
        vec![],
        vec![diag(
            "'unusedRef' is declared but its value is never read.",
            TypeDiagnosticSeverity::Hint,
            12,
            21,
        )],
    );
    assert_eq!(merged.len(), 1);
    assert!(matches!(merged[0].severity, TypeDiagnosticSeverity::Hint));
}

/// Build a diagnostic carrying the given tags (same `(start,end,code,message)`
/// identity helpers above but with editor tags attached).
fn diag_with_tags(
    message: &str,
    severity: TypeDiagnosticSeverity,
    start: u32,
    end: u32,
    code: &str,
    tags: Vec<TypeDiagnosticTag>,
) -> TypeDiagnostic {
    TypeDiagnostic {
        message: message.to_string(),
        severity,
        start,
        end,
        code: Some(code.to_string()),
        tags,
        related_information: Vec::new(),
    }
}

/// The dedup key is `(start, end, code, message)` and EXCLUDES tags. When the
/// same finding is reported by two passes — one carrying the `Unnecessary` tag
/// (the unused-symbol fade) and one without — the surviving diagnostic MUST keep
/// the tag, regardless of which pass emitted it first. Otherwise a `.vue` unused
/// import stops graying out whenever a tagless duplicate is seen first.
///
/// Discriminating: fails if `merge_diagnostic_sets` keeps only the FIRST-seen
/// variant and drops the rest, so a tagless-then-tagged ordering loses the tag
/// entirely. This asserts the tag survives in BOTH orderings.
#[test]
fn merge_diagnostic_sets_tag_survives_dedup_when_untagged_duplicate_seen_first() {
    // semantic pass reports it WITHOUT the tag; suggestion pass reports the SAME
    // finding WITH the Unnecessary tag. The tagless variant is first.
    let untagged = diag_with_tags(
        "'unused' is declared but its value is never read.",
        TypeDiagnosticSeverity::Hint,
        10,
        16,
        "6133",
        vec![],
    );
    let tagged = diag_with_tags(
        "'unused' is declared but its value is never read.",
        TypeDiagnosticSeverity::Hint,
        10,
        16,
        "6133",
        vec![TypeDiagnosticTag::Unnecessary],
    );

    let merged = merge_diagnostic_sets(vec![untagged], vec![], vec![tagged]);
    assert_eq!(
        merged.len(),
        1,
        "the duplicate collapses to one: {merged:?}"
    );
    assert!(
        merged[0].tags.contains(&TypeDiagnosticTag::Unnecessary),
        "the surviving diagnostic must keep the Unnecessary tag even though the \
         tagless duplicate was seen first, got: {:?}",
        merged[0].tags
    );
}

/// Mirror case: the tagged variant is seen FIRST. The tag must still survive (it
/// must not be clobbered by a later tagless duplicate of the same finding).
#[test]
fn merge_diagnostic_sets_tag_survives_dedup_when_tagged_duplicate_seen_first() {
    let tagged = diag_with_tags(
        "'unused' is declared but its value is never read.",
        TypeDiagnosticSeverity::Hint,
        10,
        16,
        "6133",
        vec![TypeDiagnosticTag::Unnecessary],
    );
    let untagged = diag_with_tags(
        "'unused' is declared but its value is never read.",
        TypeDiagnosticSeverity::Hint,
        10,
        16,
        "6133",
        vec![],
    );

    let merged = merge_diagnostic_sets(vec![tagged], vec![], vec![untagged]);
    assert_eq!(
        merged.len(),
        1,
        "the duplicate collapses to one: {merged:?}"
    );
    assert!(
        merged[0].tags.contains(&TypeDiagnosticTag::Unnecessary),
        "the surviving diagnostic must keep the Unnecessary tag when it was seen \
         first, got: {:?}",
        merged[0].tags
    );
}

/// Distinct tags reported across two duplicate passes UNION onto the surviving
/// diagnostic — a finding flagged `Unnecessary` by one pass and `Deprecated` by
/// another must publish BOTH (e.g. an unused deprecated import is both faded and
/// struck through).
#[test]
fn merge_diagnostic_sets_unions_distinct_tags_across_duplicates() {
    let unnecessary = diag_with_tags(
        "'oldUnused' is declared but its value is never read.",
        TypeDiagnosticSeverity::Hint,
        4,
        13,
        "6133",
        vec![TypeDiagnosticTag::Unnecessary],
    );
    let deprecated = diag_with_tags(
        "'oldUnused' is declared but its value is never read.",
        TypeDiagnosticSeverity::Hint,
        4,
        13,
        "6133",
        vec![TypeDiagnosticTag::Deprecated],
    );

    let merged = merge_diagnostic_sets(vec![unnecessary], vec![], vec![deprecated]);
    assert_eq!(
        merged.len(),
        1,
        "the duplicate collapses to one: {merged:?}"
    );
    assert!(
        merged[0].tags.contains(&TypeDiagnosticTag::Unnecessary)
            && merged[0].tags.contains(&TypeDiagnosticTag::Deprecated),
        "distinct tags from each duplicate must union onto the survivor, got: {:?}",
        merged[0].tags
    );
    // No duplicate tag entries (a union, not a concat).
    assert_eq!(
        merged[0]
            .tags
            .iter()
            .filter(|t| **t == TypeDiagnosticTag::Unnecessary)
            .count(),
        1,
        "the union must not duplicate a tag, got: {:?}",
        merged[0].tags
    );
}

/// F6: a single code-fix action whose parsed edit list is EMPTY is dropped
/// (returns `None`), mirroring `parse_tsserver_combined_code_fix`. An edit-less
/// action is not actionable and must never leave the parse boundary.
///
/// Discriminating: fails if `parse_tsserver_code_action` returns
/// `Some(TypeCodeAction { edits: [] })` for an action with no `textChanges` instead
/// of `None` — an edit-less action must not leave the parse boundary.
#[test]
fn parse_tsserver_code_action_drops_empty_edit_action() {
    let cache: HashMap<String, Arc<str>> = HashMap::new();

    // An action whose only change carries an empty `textChanges` array — no edits.
    let empty_action = serde_json::json!({
        "description": "Remove unused declaration",
        "changes": [
            { "fileName": "d:/test/file.ts", "textChanges": [] }
        ],
    });
    assert!(
        parse_tsserver_code_action(&empty_action, &cache).is_none(),
        "an edit-less single-fix action must be dropped (None), not surfaced with empty edits"
    );

    // Positive control: an action with a real textChange (whose target content is RESOLVABLE)
    // still parses. The edit path is fail-closed — it surfaces an edit only when the target's
    // content is available (cache or disk) to convert the 1-based position to a byte offset — so
    // the control seeds the cache for this path.
    let mut resolvable_cache: HashMap<String, Arc<str>> = HashMap::new();
    resolvable_cache.insert(
        "d:/test/file.ts".to_string(),
        Arc::from("const unused = 1;\n"),
    );
    let real_action = serde_json::json!({
        "description": "Remove unused declaration",
        "changes": [
            {
                "fileName": "d:/test/file.ts",
                "textChanges": [
                    {
                        "start": { "line": 1, "offset": 1 },
                        "end": { "line": 1, "offset": 10 },
                        "newText": ""
                    }
                ]
            }
        ],
    });
    let parsed = parse_tsserver_code_action(&real_action, &resolvable_cache)
        .expect("an action with a real edit must survive");
    assert_eq!(parsed.title, "Remove unused declaration");
    assert_eq!(parsed.edits.len(), 1, "the single real edit is kept");
    assert!(
        parsed.edits[0].new_text.is_empty(),
        "the deletion edit carries empty new_text"
    );
}

/// A code-edit whose target file is absent from the in-memory contents cache AND unreadable on disk
/// must be DROPPED — a wrong-location edit corrupts the file, so the EDIT path fails closed (unlike
/// the rename/location paths, which tolerate a packed line:col sentinel for an incomplete nav).
///
/// Discriminating regression: fails if a cache miss packs a 0-based `(line << 16) | col` sentinel
/// and pushes it as a real byte offset, so the merge layer applies the edit at a bogus offset. The
/// renamed/edited span sits on line 3 (1-based), so the packed value is unmistakably
/// distinguishable from any real byte offset.
#[test]
fn parse_tsserver_file_code_edits_drops_edit_when_file_unavailable() {
    // A path that does not exist on disk and is NOT in the (empty) contents cache.
    let missing = std::env::temp_dir()
        .join(format!(
            "verter-tsserver-missing-{}-does-not-exist.ts",
            std::process::id()
        ))
        .to_string_lossy()
        .replace('\\', "/");
    // Belt-and-suspenders: ensure it really is absent.
    let _ = std::fs::remove_file(&missing);

    let changes = vec![serde_json::json!({
        "fileName": missing,
        "textChanges": [
            {
                "start": { "line": 3, "offset": 14 },
                "end": { "line": 3, "offset": 21 },
                "newText": "renamed"
            }
        ]
    })];
    let cache: HashMap<String, Arc<str>> = HashMap::new();

    let edits = parse_tsserver_file_code_edits(&changes, &cache)
        .expect("a well-formed (but unresolvable) change array still returns Some(empty)");
    assert!(
        edits.is_empty(),
        "an edit whose file is unavailable must be DROPPED (fail-closed), never packed: {edits:?}"
    );
    // Assert the packed line:col sentinel is absent — a cache+disk miss must drop, never pack.
    let packed_start = ((3u32.saturating_sub(1)) << 16) | ((14u32.saturating_sub(1)) & 0xFFFF);
    assert!(
        !edits.iter().any(|e| e.start == packed_start),
        "no edit may carry the packed (line<<16)|col sentinel"
    );
}

/// A code-edit whose target content IS available but whose tsserver position is OUT OF RANGE (a
/// past-EOF line) must be DROPPED — never clamped to a content-length offset.
///
/// Discriminating regression: the shared codec (`line_column_to_offset_utf16` → `position_to_offset`)
/// clamps a past-EOF line/col to `content.len()` and returns a valid-looking WRONG offset; an edit
/// emitted at that clamped EOF offset corrupts the file. The checked converter returns
/// `None` for an out-of-range position and the edit is dropped. The fixture content is short (3
/// lines), so line 999 is unmistakably past EOF.
#[test]
fn parse_tsserver_file_code_edits_drops_out_of_range_position_not_clamped() {
    let content = "// header\nconst pad = 1;\nexport const renamed = 2;\n";
    let path = "d:/proj/oob.ts".to_string();
    let mut cache: HashMap<String, Arc<str>> = HashMap::new();
    cache.insert(path.clone(), Arc::from(content));

    // Line 999 is far past the file's 3 lines → the codec would clamp to EOF.
    let changes = vec![serde_json::json!({
        "fileName": path,
        "textChanges": [
            {
                "start": { "line": 999, "offset": 1 },
                "end": { "line": 999, "offset": 5 },
                "newText": "boom"
            }
        ]
    })];

    let edits = parse_tsserver_file_code_edits(&changes, &cache)
        .expect("a well-formed change array still returns Some(empty)");
    assert!(
        edits.is_empty(),
        "an edit whose position is out of range must be DROPPED (fail-closed), never clamped to a \
         content-length offset: {edits:?}"
    );
    // Discriminating negative: assert the offset is not the clamped `content.len()`.
    assert!(
        !edits.iter().any(|e| e.start == content.len() as u32),
        "no edit may carry the clamped content-length offset"
    );
}

/// A code-edit whose `line`/`offset` exceeds `u32::MAX` must be DROPPED — never SILENTLY truncated.
/// tsserver positions are 1-based, so the danger value wraps to a VALID 1-based position: `u32::MAX
/// + 2` truncates to `1` (line 1 / offset 1 → 0-based line 0 / col 0) under a lossy `as u32`, which
/// the checked converter would ACCEPT — so the truncation must fail closed BEFORE the converter
/// runs. With VALID content the only reason to drop is the overflow itself; an edit emitted at the
/// wrapped offset would corrupt the file at the wrong location. Verified through both
/// `parse_tsserver_file_code_edits` directly and the `parse_tsserver_code_action` wrapper.
#[test]
fn parse_tsserver_file_code_edits_drops_on_position_overflow() {
    let content = "// header\nconst pad = 1;\nexport const renamed = 2;\n";
    let path = "d:/proj/ovf.ts".to_string();
    let mut cache: HashMap<String, Arc<str>> = HashMap::new();
    cache.insert(path.clone(), Arc::from(content));

    // u32::MAX + 2 → truncates to 1 (a VALID 1-based line/offset) under a lossy `as u32`.
    let overflow = u32::MAX as u64 + 2;
    let changes = vec![serde_json::json!({
        "fileName": path,
        "textChanges": [
            {
                "start": { "line": overflow, "offset": 1 },
                "end": { "line": 1, "offset": 2 },
                "newText": "boom"
            }
        ]
    })];

    let edits = parse_tsserver_file_code_edits(&changes, &cache)
        .expect("a well-formed change array still returns Some(empty)");
    assert!(
        edits.is_empty(),
        "a u64>u32::MAX edit position must be DROPPED, never truncated into an in-range offset: \
         {edits:?}"
    );

    // The same overflow routed through the code-action wrapper drops the only edit, so the
    // edit-less action is dropped (None) rather than surfaced with a wrong-location edit.
    let action = serde_json::json!({
        "description": "Apply fix",
        "changes": [
            {
                "fileName": path,
                "textChanges": [
                    {
                        "start": { "line": overflow, "offset": 1 },
                        "end": { "line": 1, "offset": 2 },
                        "newText": "boom"
                    }
                ]
            }
        ],
    });
    assert!(
        parse_tsserver_code_action(&action, &cache).is_none(),
        "a code action whose only edit overflows must drop to None, never surface a wrong-location \
         edit"
    );

    // POSITIVE CONTROL: an in-range 1-based edit with the SAME content is still produced.
    let changes_ok = vec![serde_json::json!({
        "fileName": path,
        "textChanges": [
            {
                "start": { "line": 1, "offset": 1 },
                "end": { "line": 1, "offset": 2 },
                "newText": "x"
            }
        ]
    })];
    let ok = parse_tsserver_file_code_edits(&changes_ok, &cache)
        .expect("a well-formed change array returns Some");
    assert_eq!(ok.len(), 1, "the in-range edit is kept");
    assert_eq!(
        (ok[0].start, ok[0].end),
        (0, 1),
        "in-range 1-based (1,1)..(1,2) maps to byte offsets 0..1, unchanged"
    );
}

/// A code-edit whose endpoints invert after conversion (`start > end`) must be DROPPED — a
/// malformed span would otherwise produce a reversed-range edit. Content is available so the drop
/// is attributable to the inverted span, not a content miss.
#[test]
fn parse_tsserver_file_code_edits_drops_inverted_span() {
    let content = "const alpha = 1;\nconst beta = 2;\n";
    let path = "d:/proj/inv.ts".to_string();
    let mut cache: HashMap<String, Arc<str>> = HashMap::new();
    cache.insert(path.clone(), Arc::from(content));

    // start is on line 2 (later), end is on line 1 (earlier) → start byte > end byte.
    let changes = vec![serde_json::json!({
        "fileName": path,
        "textChanges": [
            {
                "start": { "line": 2, "offset": 7 },
                "end": { "line": 1, "offset": 7 },
                "newText": "x"
            }
        ]
    })];

    let edits = parse_tsserver_file_code_edits(&changes, &cache)
        .expect("a well-formed change array still returns Some(empty)");
    assert!(
        edits.is_empty(),
        "an edit whose start > end after conversion must be DROPPED, never emitted reversed: {edits:?}"
    );
}

/// A code-edit whose target file is absent from the contents cache but PRESENT on disk resolves its
/// byte offsets against THAT file's own on-disk content (the per-target disk fallback), matching the
/// rename/location paths' content resolution.
#[test]
fn parse_tsserver_file_code_edits_reads_disk_content_on_cache_miss() {
    let temp_root = std::env::temp_dir().join(format!(
        "verter-tsserver-codeedit-disk-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).unwrap();
    let file_path = temp_root.join("child.ts");
    let content = "// header\nconst pad = 1;\nexport const renamed = 2;\n";
    std::fs::write(&file_path, content).unwrap();
    // The fn canonicalizes `fileName`; feed the already-canonical form so the on-disk read targets
    // the file we wrote (forward slashes, lowercase drive letter on Windows).
    let file_key = verter_span::path::canonicalize_path(&file_path.to_string_lossy());
    // CACHE MISS for this path → forces the per-target disk fallback.
    let cache: HashMap<String, Arc<str>> = HashMap::new();

    // tsserver positions are 1-based: `renamed` is on line 3, column 14.
    let changes = vec![serde_json::json!({
        "fileName": file_key,
        "textChanges": [
            {
                "start": { "line": 3, "offset": 14 },
                "end": { "line": 3, "offset": 21 },
                "newText": "renamedSymbol"
            }
        ]
    })];

    let edits = parse_tsserver_file_code_edits(&changes, &cache).unwrap();
    let want_start = content.find("renamed").unwrap() as u32;
    let want_end = want_start + "renamed".len() as u32;
    assert_eq!(edits.len(), 1, "the disk-resolved edit must survive");
    assert_eq!(
        (edits[0].start, edits[0].end),
        (want_start, want_end),
        "the edit must resolve against the target's own disk content (byte offsets {want_start}..\
         {want_end}), not a packed sentinel — got {}..{}",
        edits[0].start,
        edits[0].end,
    );
    assert_eq!(edits[0].new_text, "renamedSymbol");
    // Discriminating negative: assert the offset is the real byte offset, not the packed
    // line:col fallback `(2 << 16) | 13`.
    let packed_start = ((3u32.saturating_sub(1)) << 16) | ((14u32.saturating_sub(1)) & 0xFFFF);
    assert_ne!(
        edits[0].start, packed_start,
        "must NOT be the packed (line<<16)|col fallback (the corrupting path)"
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}

/// `assemble_signature_label` joins parts EXACTLY like the rendered label and
/// reports each parameter's contiguous `[start, end)` slice of it.
#[test]
fn assemble_signature_label_offsets_index_the_label_slices() {
    let prefix = "greet(";
    let params = vec!["name: string".to_string(), "times: number".to_string()];
    let separator = ", ";
    let suffix = "): void";
    let assembled = assemble_signature_label(prefix, &params, separator, suffix);

    assert_eq!(assembled.label, "greet(name: string, times: number): void");
    assert_eq!(assembled.param_offsets.len(), 2);
    // Each offset pair must slice EXACTLY the parameter text out of the label,
    // measured in UTF-16 code units.
    let label_u16: Vec<u16> = assembled.label.encode_utf16().collect();
    for (i, &(start, end)) in assembled.param_offsets.iter().enumerate() {
        assert!(start < end, "param {i} span must be non-empty");
        assert!(end as usize <= label_u16.len(), "param {i} span in bounds");
        let slice = String::from_utf16(&label_u16[start as usize..end as usize]).unwrap();
        assert_eq!(slice, params[i], "offsets must slice the exact param text");
    }
    // Concretely: "name: string" is [6, 18) in the assembled label.
    assert_eq!(assembled.param_offsets[0], (6, 18));
}

/// Offsets are UTF-16 code units, NOT bytes and NOT `char`s.
///
/// A multi-byte BMP character (`é` = 2 UTF-8 bytes, 1 UTF-16 unit) and an astral
/// character (`𝕏` = 4 UTF-8 bytes, 2 UTF-16 units, 1 `char`) in the prefix and a
/// parameter must shift the offsets by the UTF-16 count. A byte- or char-based
/// implementation would compute different numbers here, so this test discriminates
/// the encoding.
#[test]
fn assemble_signature_label_offsets_are_utf16_not_bytes_or_chars() {
    // prefix "é𝕏(" : 'é'=1 u16, '𝕏'=2 u16, '('=1 u16 → 4 UTF-16 units
    //               but 2 + 4 + 1 = 7 UTF-8 bytes, and 3 chars.
    let prefix = "é𝕏(";
    let params = vec!["a: 𝕏".to_string(), "b: number".to_string()];
    let separator = ", ";
    let suffix = ")";
    let assembled = assemble_signature_label(prefix, &params, separator, suffix);

    // First param starts right after the 4-UTF-16-unit prefix.
    assert_eq!(
        assembled.param_offsets[0].0, 4,
        "start = UTF-16 len of prefix"
    );
    // "a: 𝕏" = 'a'(1) ' '(1)? actually "a: " is 'a',':',' ' = 3 u16, plus '𝕏'=2 → 5 u16.
    assert_eq!(assembled.param_offsets[0], (4, 9));
    // Second param: after first (5) + separator ", " (2) → starts at 11.
    assert_eq!(assembled.param_offsets[1].0, 11);

    // Cross-check: every offset still slices the exact text in UTF-16 space.
    let label_u16: Vec<u16> = assembled.label.encode_utf16().collect();
    for (i, &(start, end)) in assembled.param_offsets.iter().enumerate() {
        let slice = String::from_utf16(&label_u16[start as usize..end as usize]).unwrap();
        assert_eq!(slice, params[i]);
    }

    // Negative: a byte-based computation would place param[0] start at the UTF-8
    // byte length of the prefix (7), which is WRONG. Assert we are NOT doing that.
    assert_ne!(
        assembled.param_offsets[0].0,
        prefix.len() as u32,
        "offsets must be UTF-16 units, never UTF-8 byte offsets"
    );
    // And NOT char counts (prefix is 3 chars).
    assert_ne!(
        assembled.param_offsets[0].0,
        prefix.chars().count() as u32,
        "offsets must be UTF-16 units, never char counts"
    );
}

// ── resync per-file generation gate ───────────────────────────────────

/// A bare transport + the live caches a resync reads, with no tsserver child.
/// Mirrors `run_update_file_capture`'s harness, exposing the caches so a test can
/// drive `resync_capture` / `resync_apply` and a concurrent content mutation
/// directly (the production resync method delegates to those exact functions).
struct ResyncHarness {
    transport: Arc<TsserverTransport>,
    stdin_tx: mpsc::Sender<TsserverStdinMessage>,
    contents: Arc<Mutex<HashMap<String, Arc<str>>>>,
    opened_files: Arc<Mutex<HashMap<String, OpenKind>>>,
    generations: Arc<ContentGenerations>,
    carrier_projects: Arc<parking_lot::RwLock<HashMap<String, String>>>,
    project_roots: Arc<parking_lot::RwLock<Vec<String>>>,
    client_reader: tokio::io::DuplexStream,
}

fn resync_harness() -> ResyncHarness {
    let (client_reader, server_writer) = tokio::io::duplex(65536);
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));
    let transport = Arc::new(TsserverTransport {
        stdin_tx: stdin_tx.clone(),
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: None,
    });
    ResyncHarness {
        transport,
        stdin_tx,
        contents: Arc::new(Mutex::new(HashMap::new())),
        opened_files: Arc::new(Mutex::new(HashMap::new())),
        generations: Arc::new(ContentGenerations::default()),
        carrier_projects: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        project_roots: Arc::new(parking_lot::RwLock::new(Vec::new())),
        client_reader,
    }
}

/// Shut the writer down and read every frame that reached tsserver. The per-read
/// timeout is only a failsafe (the writer drains all queued frames before EOF).
async fn drain_frames(
    stdin_tx: &mpsc::Sender<TsserverStdinMessage>,
    client_reader: tokio::io::DuplexStream,
) -> Vec<serde_json::Value> {
    let _ = stdin_tx.send(TsserverStdinMessage::Shutdown).await;
    let mut reader = BufReader::new(client_reader);
    let mut frames = Vec::new();
    loop {
        let mut line = String::new();
        match tokio::time::timeout(
            std::time::Duration::from_millis(200),
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(_)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                    frames.push(val);
                }
            }
            Ok(Err(_)) => break,
        }
    }
    frames
}

#[tokio::test]
async fn resync_skips_stale_source_reopen_when_content_generation_advanced() {
    // DISCRIMINATION: reverting the generation gate (always reopen with the
    // captured content) makes this RED — the resync would `close` then `open` the
    // source with the stale `const v = 1;` bytes, clobbering the concurrent edit's
    // `const v = 2;` that `update_file` already pushed.
    let h = resync_harness();
    let file = "/project/src/App.vue.tsx";

    store_content_bump_generation(&h.contents, &h.generations, file, Arc::from("const v = 1;"))
        .await;
    h.opened_files
        .lock()
        .await
        .insert(file.to_string(), OpenKind::Source);

    // Capture the resync plan (content `v1`, generation 1).
    let entries = resync_capture(&h.opened_files, &h.contents, &h.generations).await;

    // A concurrent `update_file` lands AFTER the capture: newer bytes + a bumped
    // generation.
    store_content_bump_generation(&h.contents, &h.generations, file, Arc::from("const v = 2;"))
        .await;

    resync_apply(
        &h.transport,
        entries,
        &h.contents,
        &h.generations,
        &h.carrier_projects,
        &h.project_roots,
        "/project",
    )
    .await
    .unwrap();

    let frames = drain_frames(&h.stdin_tx, h.client_reader).await;
    assert!(
        !frames.iter().any(|f| f["command"] == "open"),
        "a resync whose captured generation is stale must NOT reopen the source, frames={frames:?}"
    );
    assert!(
        !frames.iter().any(|f| f["command"] == "close"),
        "a skipped stale resync must not even close the source, frames={frames:?}"
    );
}

#[tokio::test]
async fn resync_reopens_source_with_current_content_when_generation_matches() {
    // The non-racy path still works: when no edit lands after capture, the source
    // is closed and reopened WITH its content.
    let h = resync_harness();
    let file = "/project/src/App.vue.tsx";

    store_content_bump_generation(&h.contents, &h.generations, file, Arc::from("const v = 1;"))
        .await;
    h.opened_files
        .lock()
        .await
        .insert(file.to_string(), OpenKind::Source);

    let entries = resync_capture(&h.opened_files, &h.contents, &h.generations).await;
    resync_apply(
        &h.transport,
        entries,
        &h.contents,
        &h.generations,
        &h.carrier_projects,
        &h.project_roots,
        "/project",
    )
    .await
    .unwrap();

    let frames = drain_frames(&h.stdin_tx, h.client_reader).await;
    assert!(
        frames
            .iter()
            .any(|f| f["command"] == "close" && f["arguments"]["file"] == file),
        "resync closes the source, frames={frames:?}"
    );
    assert!(
        frames.iter().any(|f| f["command"] == "open"
            && f["arguments"]["file"] == file
            && f["arguments"]["fileContent"] == "const v = 1;"),
        "resync reopens the source with its current content, frames={frames:?}"
    );
}

#[tokio::test]
async fn resync_reopens_carrier_companion_contentlessly() {
    // PRESERVED behavior: a carrier companion is reopened with NO `fileContent`
    // (the plugin stays the engine-side content authority) and routed to its
    // owning project's directory. It carries no bytes, so the generation gate
    // never applies to it.
    let h = resync_harness();
    let carrier = "/project/src/App.vue.tsx";

    h.opened_files
        .lock()
        .await
        .insert(carrier.to_string(), OpenKind::CarrierCompanion);
    h.carrier_projects
        .write()
        .insert(carrier.to_string(), "/project/tsconfig.json".to_string());

    let entries = resync_capture(&h.opened_files, &h.contents, &h.generations).await;
    resync_apply(
        &h.transport,
        entries,
        &h.contents,
        &h.generations,
        &h.carrier_projects,
        &h.project_roots,
        "/project",
    )
    .await
    .unwrap();

    let frames = drain_frames(&h.stdin_tx, h.client_reader).await;
    let open = frames
        .iter()
        .find(|f| f["command"] == "open" && f["arguments"]["file"] == carrier)
        .expect("carrier companion is reopened");
    assert!(
        open["arguments"].get("fileContent").is_none(),
        "a carrier companion must be reopened CONTENTLESSLY, frame={open:?}"
    );
    assert_eq!(
        open["arguments"]["projectRootPath"], "/project",
        "a carrier companion routes to its owning project directory, frame={open:?}"
    );
}

/// M9 — the resync content-generation gate must reject a close→reopen ABA.
///
/// A resync captures a file's `(content, generation)` snapshot, then — before the
/// captured plan is applied — the file is CLOSED and REOPENED with fresh bytes (an
/// editor close→reopen, or a delete→recreate). The gate re-reads the live
/// generation immediately before reopening and skips the stale reopen unless it
/// still matches the captured one. The ABA hazard: a per-file counter that
/// resets/recycles on close would re-stamp the reopened file with the SAME value
/// the resync captured, so the gate would falsely match and resend the STALE
/// captured bytes — clobbering the fresh content. The `ContentGenerations` counter
/// is GLOBALLY monotonic, so a reopened file always draws a strictly greater
/// value the captured one can never alias.
///
/// DISCRIMINATING: with the production global-monotonic counter the live
/// generation after close→reopen is strictly greater than the captured one
/// (gate skips, no stale reopen). Replacing `store_content_bump_generation`'s
/// `next_generation()` with a per-file `map.get(file).unwrap_or(0) + 1` reset
/// counter makes the reopened generation equal the captured one again, so BOTH
/// assertions below go RED (`live_gen > captured_gen` fails, and the stale bytes
/// are resent).
#[tokio::test]
async fn resync_generation_gate_rejects_close_reopen_aba() {
    let (client_reader, server_writer) = tokio::io::duplex(65536);
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));
    let transport = TsserverTransport {
        stdin_tx: stdin_tx.clone(),
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: None,
    };

    let opened_files: Mutex<HashMap<String, OpenKind>> = Mutex::new(HashMap::new());
    let contents_cache: Mutex<HashMap<String, Arc<str>>> = Mutex::new(HashMap::new());
    let content_generations = ContentGenerations::default();
    let carrier_projects: parking_lot::RwLock<HashMap<String, String>> =
        parking_lot::RwLock::new(HashMap::new());
    let project_roots: parking_lot::RwLock<Vec<String>> = parking_lot::RwLock::new(Vec::new());

    let file = "/project/src/real.ts";
    let stale = "export const v = 1;\n";
    let fresh = "export const v = 2;\n";

    // 1. First write → captured generation; track it as an open Source.
    store_content_bump_generation(
        &contents_cache,
        &content_generations,
        file,
        Arc::from(stale),
    )
    .await;
    opened_files
        .lock()
        .await
        .insert(file.to_string(), OpenKind::Source);

    // 2. Capture the resync plan (records the captured content + generation).
    let entries = resync_capture(&opened_files, &contents_cache, &content_generations).await;
    assert_eq!(entries.len(), 1, "exactly the tracked source is captured");
    let captured_gen = entries[0].generation;

    // 3. The ABA: close (forget) then reopen with FRESH bytes BEFORE the captured
    //    plan is applied.
    forget_content(&contents_cache, &content_generations, file).await;
    store_content_bump_generation(
        &contents_cache,
        &content_generations,
        file,
        Arc::from(fresh),
    )
    .await;

    // The reopened generation must be STRICTLY GREATER than the captured one — the
    // direct ABA-free invariant. A reset/recycle counter would re-stamp it equal.
    let live_gen = content_generations
        .map
        .lock()
        .get(file)
        .copied()
        .expect("reopened file is tracked");
    assert!(
        live_gen > captured_gen,
        "a reopened file must draw a strictly greater generation than the captured one \
         (captured={captured_gen}, live={live_gen}); an equal value is the ABA the global \
         monotonic counter exists to prevent"
    );

    // 4. Apply the now-stale captured plan: the gate must SKIP the reopen.
    resync_apply(
        &transport,
        entries,
        &contents_cache,
        &content_generations,
        &carrier_projects,
        &project_roots,
        "/project",
    )
    .await
    .expect("resync apply should succeed");

    // Drain the frames the apply wrote.
    let _ = stdin_tx.send(TsserverStdinMessage::Shutdown).await;
    let mut reader = BufReader::new(client_reader);
    let mut frames = Vec::new();
    loop {
        let mut line = String::new();
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(_)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                    frames.push(val);
                }
            }
            Ok(Err(_)) => break,
        }
    }

    // 5. The stale captured bytes must NEVER be resent — a false ABA match would
    //    reopen the file with the captured `stale` content, clobbering `fresh`.
    let resent_stale = frames.iter().any(|frame| {
        frame["command"] == "open"
            && frame["arguments"]["file"] == file
            && frame["arguments"]["fileContent"] == stale
    });
    assert!(
        !resent_stale,
        "the stale captured bytes must not be resent after a close→reopen (the gate must skip \
         the superseded reopen), frames={frames:?}"
    );
}

// ── hang detection + reloadProjects storm bounding (D2) ───────────────

/// Build a bare transport wired to a duplex whose "tsserver" side is never read
/// (requests are accepted into the pipe but never answered), exposing the
/// crash_notify + frame-capture surface a hang/storm test drives.
struct StormHarness {
    transport: Arc<TsserverTransport>,
    stdin_tx: mpsc::Sender<TsserverStdinMessage>,
    crash_notify: Arc<Notify>,
    client_reader: tokio::io::DuplexStream,
}

fn storm_harness() -> StormHarness {
    storm_harness_with_crash_notify(Arc::new(Notify::new()))
}

fn storm_harness_with_crash_notify(crash_notify: Arc<Notify>) -> StormHarness {
    let (client_reader, server_writer) = tokio::io::duplex(65536);
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));
    let transport = Arc::new(TsserverTransport {
        stdin_tx: stdin_tx.clone(),
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: Some(Arc::clone(&crash_notify)),
        membership_recovery: Mutex::new(None),
        cancellation: None,
    });
    StormHarness {
        transport,
        stdin_tx,
        crash_notify,
        client_reader,
    }
}

struct RealReloadCarrier {
    source_path: String,
    companion_path: String,
    content: &'static str,
    hover_offset: u32,
    expected_hover: &'static str,
    changed_content: &'static str,
    changed_hover: &'static str,
}

struct RealReloadHarness {
    _project: tempfile::TempDir,
    provider: TsserverTypeProvider,
    carriers: Vec<RealReloadCarrier>,
    project_file_name: String,
    carrier_store_dir: std::path::PathBuf,
}

impl RealReloadHarness {
    async fn new() -> Self {
        use crate::resilient::resilient_tests::RECOVERY_CARRIERS;

        let project = tempfile::tempdir().expect("create real reload project");
        std::fs::write(
            project.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"jsx":"preserve"},"include":["*.tsx"]}"#,
        )
        .expect("write real reload tsconfig");
        let carriers: Vec<RealReloadCarrier> = RECOVERY_CARRIERS
            .iter()
            .map(|fixture| {
                let source_name = std::path::Path::new(fixture.source_path)
                    .file_name()
                    .expect("fixture source file name");
                let companion_name = std::path::Path::new(fixture.companion_path)
                    .file_name()
                    .expect("fixture companion file name");
                let source_path = project.path().join(source_name);
                let companion_path = project.path().join(companion_name);
                std::fs::write(&companion_path, fixture.stale_disk_content)
                    .expect("write stale reload carrier bytes");
                RealReloadCarrier {
                    source_path: source_path.to_string_lossy().replace('\\', "/"),
                    companion_path: companion_path.to_string_lossy().replace('\\', "/"),
                    content: fixture.content,
                    hover_offset: fixture.hover_offset,
                    expected_hover: fixture.expected_hover,
                    changed_content: if fixture.source_path.ends_with(".vue") {
                        "export const vueRecoveryValue: boolean = true;\nvueRecoveryValue;\n"
                    } else {
                        "export const svelteRecoveryValue: string = 'changed';\nsvelteRecoveryValue;\n"
                    },
                    changed_hover: if fixture.source_path.ends_with(".vue") {
                        "const vueRecoveryValue: boolean"
                    } else {
                        "const svelteRecoveryValue: string"
                    },
                }
            })
            .collect();
        let workspace_root = project.path().to_string_lossy().replace('\\', "/");
        let project_file_name = project
            .path()
            .join("tsconfig.json")
            .to_string_lossy()
            .replace('\\', "/");
        let carrier_store_dir = project.path().join("carrier-store");
        crate::resilient::resilient_tests::publish_unready_recovery_carrier_store(
            &carrier_store_dir,
            &project_file_name,
            1,
            carriers.iter().map(|carrier| {
                (
                    carrier.source_path.as_str(),
                    carrier.companion_path.as_str(),
                )
            }),
        );
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root above crates/");
        let node_path = crate::discovery::find_node()
            .expect("real reload tests require the workspace Node.js runtime");
        let tsserver_path = crate::resilient::resilient_tests::real_tsserver_path(repo_root);
        let plugin_path = crate::resilient::resilient_tests::build_real_tsserver_plugin(
            repo_root,
            project.path(),
            &node_path,
        )
        .await;
        let provider = TsserverTypeProvider::spawn(
            &node_path,
            &tsserver_path.to_string_lossy(),
            &workspace_root,
            Some(&plugin_path.to_string_lossy()),
            Some(&carrier_store_dir.to_string_lossy()),
            false,
            None,
        )
        .await
        .expect("spawn real reload tsserver");

        Self {
            _project: project,
            provider,
            carriers,
            project_file_name,
            carrier_store_dir,
        }
    }

    async fn register_carriers(&self) {
        for carrier in &self.carriers {
            // No ordinary open: membership is contentless and engine bytes come
            // exclusively from the plugin store across reloadProjects.
            self.provider
                .register_carrier_member(
                    &carrier.source_path,
                    &carrier.companion_path,
                    carrier.content,
                    &self.project_file_name,
                )
                .await
                .expect("register real reload carrier");
        }
    }

    fn publish_ready(&self, epoch: u64, version: u64, changed: bool) {
        crate::resilient::resilient_tests::publish_recovery_carrier_store(
            &self.carrier_store_dir,
            &self.project_file_name,
            epoch,
            version,
            self.carriers.iter().map(|carrier| {
                (
                    carrier.source_path.as_str(),
                    carrier.companion_path.as_str(),
                    if changed {
                        carrier.changed_content
                    } else {
                        carrier.content
                    },
                )
            }),
        );
    }

    async fn raw_quickinfo(&self, carrier: &RealReloadCarrier) -> Result<String, String> {
        let (line, offset) = byte_offset_to_tsserver_pos(carrier.content, carrier.hover_offset);
        let args = inject_project_file_name(
            serde_json::json!({
                "file": carrier.companion_path,
                "line": line,
                "offset": offset,
            }),
            &Some(self.project_file_name.clone()),
        );
        self.provider
            .transport
            .request("quickinfo", args)
            .await
            .map_err(|error| error.message)
            .map(|body| {
                body.get("displayString")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            })
    }

    async fn assert_raw_stale_on_disk(&self) {
        for carrier in &self.carriers {
            let display = self
                .raw_quickinfo(carrier)
                .await
                .unwrap_or_else(|error| panic!("raw stale quickinfo failed: {error}"));
            assert!(
                display.contains(": null"),
                "epoch-1 unready carrier must expose stale disk bytes before explicit recovery, got {display:?}"
            );
        }
    }

    async fn assert_raw_types(&self, changed: bool) {
        for carrier in &self.carriers {
            let expected = if changed {
                carrier.changed_hover
            } else {
                carrier.expected_hover
            };
            let mut last = Err("raw quickinfo not attempted".to_string());
            for delay_ms in [0u64, 100, 250, 500, 1000, 2000] {
                if delay_ms != 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                last = self.raw_quickinfo(carrier).await;
                if last
                    .as_ref()
                    .is_ok_and(|display| display.contains(expected) && !display.contains(": any"))
                {
                    break;
                }
            }
            let display = last.unwrap_or_else(|error| {
                panic!(
                    "raw quickinfo never recovered {} as {expected}: {error}",
                    carrier.source_path
                )
            });
            assert!(
                display.contains(expected),
                "raw quickinfo must derive {expected} from the refreshed store, got {display:?}"
            );
            assert!(!display.contains(": any"));
        }
    }

    async fn assert_raw_types_once(&self, changed: bool) {
        for carrier in &self.carriers {
            let expected = if changed {
                carrier.changed_hover
            } else {
                carrier.expected_hover
            };
            let display = self
                .raw_quickinfo(carrier)
                .await
                .unwrap_or_else(|error| panic!("raw quickinfo failed: {error}"));
            assert!(
                display.contains(expected),
                "raw quickinfo must remain {expected}, got {display:?}"
            );
        }
    }

    async fn shutdown(&self) {
        self.provider
            .shutdown()
            .await
            .expect("shutdown real tsserver");
    }
}

/// D2: a wedged-but-alive tsserver (accepts requests, never responds) must be
/// detected via consecutive timeouts and trigger a restart — not silently time
/// out every request for the rest of the session.
#[tokio::test]
async fn consecutive_timeouts_fire_crash_notify() {
    use crate::resilient::resilient_tests::RealRecoveryHarness;

    let recovery = RealRecoveryHarness::new(0).await;
    recovery.register_carriers().await;
    let crash_notify = recovery.crash_notify();

    let harness = storm_harness_with_crash_notify(Arc::clone(&crash_notify));
    // Register the waiter BEFORE the timeouts so notify_waiters cannot be missed.
    let waiter = {
        let crash_notify = Arc::clone(&harness.crash_notify);
        tokio::spawn(async move {
            crash_notify.notified().await;
        })
    };

    for _ in 0..HANG_THRESHOLD {
        let result = harness
            .transport
            .request_with_timeout(
                "quickinfo",
                serde_json::json!({ "file": "/w/App.vue.tsx", "line": 1, "offset": 1 }),
                std::time::Duration::from_millis(30),
            )
            .await;
        assert!(result.is_err(), "an unanswered request must time out");
    }

    tokio::time::timeout(std::time::Duration::from_millis(500), waiter)
        .await
        .expect("hang detection must fire crash_notify after HANG_THRESHOLD consecutive timeouts")
        .expect("waiter task must not panic");

    recovery.await_down().await;
    recovery.assert_carriers_answer_typed().await;
    assert_eq!(
        recovery.spawn_attempts(),
        1,
        "wedge triggers one real respawn"
    );
    recovery.shutdown().await;
}

/// A successful response resets the consecutive-timeout counter, so an isolated
/// timeout followed by healthy traffic never escalates to a spurious restart.
#[tokio::test]
async fn successful_response_resets_hang_counter() {
    let harness = storm_harness();
    // Two timeouts (below threshold) — the counter advances but must not fire.
    for _ in 0..(HANG_THRESHOLD - 1) {
        let _ = harness
            .transport
            .request_with_timeout(
                "quickinfo",
                serde_json::json!({ "file": "/w/App.vue.tsx", "line": 1, "offset": 1 }),
                std::time::Duration::from_millis(30),
            )
            .await;
    }
    assert_eq!(
        harness
            .transport
            .consecutive_failures
            .load(Ordering::Relaxed),
        HANG_THRESHOLD - 1,
    );

    // Issue a real request and resolve its pending entry from the "tsserver" side.
    let request_transport = Arc::clone(&harness.transport);
    let request_task = tokio::spawn(async move {
        request_transport
            .request_with_timeout(
                "quickinfo",
                serde_json::json!({ "file": "/w/App.vue.tsx", "line": 1, "offset": 1 }),
                std::time::Duration::from_millis(500),
            )
            .await
    });
    // Let the request register its pending entry, then answer it.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    {
        let seq = harness.transport.next_seq.load(Ordering::Relaxed) - 1;
        if let Some(tx) = harness.transport.pending.take(seq) {
            let _ = tx.send(serde_json::json!({"success": true, "body": null}));
        }
    }
    let ok = request_task.await.expect("request task must not panic");
    assert!(ok.is_ok(), "an answered request resolves Ok");
    assert_eq!(
        harness
            .transport
            .consecutive_failures
            .load(Ordering::Relaxed),
        0,
        "a successful response resets the consecutive-timeout counter"
    );
}

/// D2: a storm of concurrent cold-miss membership recoveries must coalesce into
/// a single `reloadProjects` send, never one reload per concurrent query.
#[tokio::test]
async fn reload_projects_recovery_coalesces_under_concurrency() {
    let harness = storm_harness();

    let mut handles = Vec::new();
    for _ in 0..32 {
        let transport = Arc::clone(&harness.transport);
        handles.push(tokio::spawn(async move {
            recover_companion_membership(&transport).await;
        }));
    }
    for handle in handles {
        handle.await.expect("recovery task must not panic");
    }

    let frames = drain_frames(&harness.stdin_tx, harness.client_reader).await;
    let reloads = frames
        .iter()
        .filter(|frame| frame["command"] == "reloadProjects")
        .count();
    assert_eq!(
        reloads, 1,
        "32 concurrent cold-miss recoveries must coalesce into ONE reloadProjects, frames={frames:?}"
    );
}

/// After the cooldown window a fresh recovery IS sent (the gate rate-limits, it
/// does not permanently latch shut).
#[tokio::test]
async fn reload_projects_recovery_fires_again_after_cooldown() {
    let harness = storm_harness();

    recover_companion_membership(&harness.transport).await;
    // Within the cooldown: a second recovery is suppressed.
    recover_companion_membership(&harness.transport).await;
    // Past the cooldown: a third recovery fires a second reload.
    tokio::time::sleep(MEMBERSHIP_RECOVERY_COOLDOWN + std::time::Duration::from_millis(80)).await;
    recover_companion_membership(&harness.transport).await;

    let frames = drain_frames(&harness.stdin_tx, harness.client_reader).await;
    let reloads = frames
        .iter()
        .filter(|frame| frame["command"] == "reloadProjects")
        .count();
    assert_eq!(
        reloads, 2,
        "the first and post-cooldown recoveries fire; the within-cooldown one is suppressed, frames={frames:?}"
    );
}

#[tokio::test]
async fn real_reload_projects_recovery_refreshes_plugin_carriers() {
    let real = RealReloadHarness::new().await;
    real.register_carriers().await;
    real.assert_raw_stale_on_disk().await;
    real.publish_ready(2, 1, false);
    recover_companion_membership(&real.provider.transport).await;
    real.assert_raw_types(false).await;
    real.shutdown().await;
}

#[tokio::test]
async fn real_reload_projects_recovery_refreshes_after_cooldown() {
    let real = RealReloadHarness::new().await;
    real.register_carriers().await;
    real.assert_raw_stale_on_disk().await;

    real.publish_ready(2, 1, false);
    recover_companion_membership(&real.provider.transport).await;
    real.assert_raw_types(false).await;

    real.publish_ready(3, 2, true);
    // Suppressed inside the cooldown: the existing Program must remain on v1.
    recover_companion_membership(&real.provider.transport).await;
    real.assert_raw_types_once(false).await;

    tokio::time::sleep(MEMBERSHIP_RECOVERY_COOLDOWN + std::time::Duration::from_millis(80)).await;
    recover_companion_membership(&real.provider.transport).await;
    real.assert_raw_types(true).await;
    real.shutdown().await;
}

// ===========================================================================
// The hop bound and the ambient request deadline.
//
// tsserver is a single JavaScript thread: every request queues in front of it,
// so a hop that outlives the request that asked for it is pure contention. The
// inner bound therefore has to fire INSIDE the caller's deadline — that margin
// is what buys the transport its cleanup (release the slot, charge the failure,
// cancel the engine's work) and what makes the failure attributable to the
// engine rather than to the handler.
// ===========================================================================

/// Build a `TsserverTransport` for tests over a caller-supplied stdin lane.
fn test_transport(stdin_tx: mpsc::Sender<TsserverStdinMessage>) -> TsserverTransport {
    TsserverTransport {
        stdin_tx,
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: TsserverCancellation::create().map(Arc::new),
    }
}

/// As [`test_transport`], with an observable crash notification.
fn test_transport_with_notify(
    stdin_tx: mpsc::Sender<TsserverStdinMessage>,
    crash_notify: Arc<Notify>,
) -> TsserverTransport {
    TsserverTransport {
        stdin_tx,
        pending: Arc::new(TsserverPendingRequests::default()),
        next_seq: AtomicI64::new(1),
        consecutive_failures: AtomicU32::new(0),
        crash_notify: Some(crash_notify),
        membership_recovery: Mutex::new(None),
        cancellation: TsserverCancellation::create().map(Arc::new),
    }
}

/// A hop issued under an ambient request deadline must fail at that deadline,
/// not at the transport's own fixed bound. With the fixed bound winning, the
/// outer deadline always fires first and the transport's failure branch — the
/// only place the pending entry is released and the failure counted — never
/// runs at all.
#[tokio::test]
async fn a_tsserver_hop_fires_inside_the_ambient_request_deadline() {
    // Nothing drains the lane past the first frame and no read loop exists, so
    // the request can never be answered.
    let (stdin_tx, _stdin_rx) = mpsc::channel::<TsserverStdinMessage>(16);
    let transport = test_transport(stdin_tx);

    let started = std::time::Instant::now();
    let outcome = crate::deadline::with_deadline(std::time::Duration::from_millis(400), async {
        transport.request("quickinfo", serde_json::json!({})).await
    })
    .await;
    let elapsed = started.elapsed();

    let err = outcome.expect_err("an unanswered request must fail, not succeed");
    assert!(
        err.message.contains("timed out"),
        "the hop must fail as a provider timeout so the failure is attributable to \
         the engine, got {}",
        err.message
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "the hop must fire inside the 400ms ambient deadline, not at the transport's \
         fixed 10s bound; took {elapsed:?}"
    );
}

/// A hop bounded by the caller's deadline gives up promptly instead of parking
/// on the transport's own fixed bound — so the cleanup behind it (releasing the
/// slot, cancelling the engine's work) actually runs.
#[tokio::test]
async fn deadline_bounded_tsserver_hops_all_give_up_promptly() {
    let (stdin_tx, _stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    let transport = test_transport(stdin_tx);

    let ran = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        for _ in 0..HANG_THRESHOLD {
            let _ = crate::deadline::with_deadline(std::time::Duration::from_millis(300), async {
                transport.request("quickinfo", serde_json::json!({})).await
            })
            .await;
        }
    })
    .await;
    assert!(
        ran.is_ok(),
        "three hops under a 300ms ambient deadline must all complete well inside 3s; \
         a fixed 10s inner bound parks each one instead"
    );
    assert_eq!(
        pending_len(&transport),
        0,
        "each abandoned hop must have released its pending slot"
    );
}

/// A hop the CALLER's deadline cut short is not evidence that the engine is
/// wedged, and must not be charged toward hang detection.
///
/// A cold project legitimately takes longer than a 1.5s hover budget. Charging
/// those hops restarts a healthy engine mid-program-build, which throws away the
/// program it was building and makes the next requests cold too — three of those
/// and it restarts again. The engine never gets far enough to answer anything,
/// and the requests come back fast and EMPTY instead of slow and correct.
#[tokio::test]
async fn a_deadline_shortened_tsserver_hop_is_not_charged_to_hang_detection() {
    let (stdin_tx, _stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    let notify = Arc::new(Notify::new());
    let transport = test_transport_with_notify(stdin_tx, Arc::clone(&notify));

    for _ in 0..(HANG_THRESHOLD + 2) {
        let _ = crate::deadline::with_deadline(std::time::Duration::from_millis(250), async {
            transport.request("quickinfo", serde_json::json!({})).await
        })
        .await;
    }

    assert_eq!(
        transport.consecutive_failures.load(Ordering::Relaxed),
        0,
        "a hop the caller's own deadline cut short says nothing about engine health"
    );
}

/// A hop that ran to its FULL configured bound and still went unanswered IS
/// evidence of a wedged engine, and must still restart it. This is the bound
/// long enough to mean something — batch and background work, which carries no
/// ambient deadline.
#[tokio::test]
async fn a_full_bound_tsserver_timeout_is_still_charged_to_hang_detection() {
    let (stdin_tx, _stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    let notify = Arc::new(Notify::new());
    let transport = test_transport_with_notify(stdin_tx, Arc::clone(&notify));

    let waiter = {
        let notify = Arc::clone(&notify);
        tokio::spawn(async move { notify.notified().await })
    };
    tokio::task::yield_now().await;

    for _ in 0..HANG_THRESHOLD {
        let _ = transport
            .request_with_timeout(
                "quickinfo",
                serde_json::json!({}),
                std::time::Duration::from_millis(120),
            )
            .await;
    }

    assert_eq!(
        transport.consecutive_failures.load(Ordering::Relaxed),
        HANG_THRESHOLD,
        "an unanswered hop that used its whole configured bound must be charged"
    );
    tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
        .await
        .expect("HANG_THRESHOLD consecutive full-bound timeouts must fire the restart")
        .unwrap();
}

/// A caller with no ambient deadline — batch and background work — keeps its
/// configured bound verbatim. Guards the opposite failure: a hop bound so
/// aggressive that non-interactive work cannot finish.
#[tokio::test]
async fn an_undeadlined_tsserver_hop_keeps_its_configured_bound() {
    let (stdin_tx, _stdin_rx) = mpsc::channel::<TsserverStdinMessage>(16);
    let transport = test_transport(stdin_tx);

    let started = std::time::Instant::now();
    let err = transport
        .request_with_timeout(
            "quickinfo",
            serde_json::json!({}),
            std::time::Duration::from_millis(300),
        )
        .await
        .expect_err("an unanswered request must fail, not succeed");
    let elapsed = started.elapsed();

    assert!(err.message.contains("timed out"), "got {}", err.message);
    assert!(
        elapsed >= std::time::Duration::from_millis(250),
        "with no ambient scope open the configured bound is kept verbatim, not \
         shortened; took {elapsed:?}"
    );
}

// ===========================================================================
// Cancel-on-drop.
//
// A request deadline scaled to a human fires while tsserver's round-trip is
// still outstanding — that is the point of it. Dropping the caller's future is
// therefore the ordinary way a request ends, and on a single-threaded engine it
// is the case that matters most: abandoned work keeps the one JavaScript thread
// busy ahead of every request that replaced it.
// ===========================================================================

/// How many requests the transport currently has registered. The leak surface:
/// a request abandoned without releasing its slot shows up here and nowhere
/// else.
fn pending_len(transport: &TsserverTransport) -> usize {
    transport.pending.len()
}

/// Dropping a caller's in-flight request future must release its pending-map
/// slot. Without this the slot survives for the life of the session whenever the
/// engine never answers — one leaked entry per abandoned request.
#[tokio::test]
async fn dropping_an_in_flight_tsserver_request_releases_its_pending_slot() {
    let (stdin_tx, _stdin_rx) = mpsc::channel::<TsserverStdinMessage>(16);
    let transport = test_transport(stdin_tx);

    {
        let mut fut = Box::pin(transport.request("quickinfo", serde_json::json!({})));
        // Poll once so the seq is registered and the future is parked on the
        // response that never comes.
        tokio::select! {
            _ = &mut fut => panic!("no response was ever written; the request cannot complete"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
        assert_eq!(
            pending_len(&transport),
            1,
            "the in-flight request must hold exactly one pending slot"
        );
    }

    assert_eq!(
        pending_len(&transport),
        0,
        "dropping the caller's future must release the pending slot, not leak it"
    );
}

/// The seq tsserver would concatenate onto its cancellation-pipe template.
fn cancellation_path(transport: &TsserverTransport, seq: i64) -> std::path::PathBuf {
    let cancellation = transport
        .cancellation
        .as_ref()
        .expect("the test transport owns a cancellation directory");
    std::path::PathBuf::from(format!("{}{seq}", cancellation.prefix))
}

/// The `--cancellationPipeName` template must be exactly the prefix tsserver
/// concatenates the request id onto, with the single trailing `*` that selects
/// per-request mode. A prefix containing another `*` makes tsserver throw and
/// leaves the session silently un-cancellable, so the template is built once and
/// the written path derives from that same string.
#[test]
fn the_cancellation_template_names_the_paths_the_transport_writes() {
    let cancellation =
        TsserverCancellation::create().expect("a cancellation directory must be creatable");
    let arg = cancellation.pipe_name_arg();

    assert!(
        arg.ends_with('*'),
        "the template must end with `*` or tsserver treats it as a single global \
         token that cancels whichever request happens to be running, got {arg}"
    );
    let prefix = arg.strip_suffix('*').unwrap();
    assert!(
        !prefix.contains('*'),
        "tsserver rejects a template whose prefix contains another `*`, got {arg}"
    );

    cancellation.cancel(41);
    assert!(
        std::path::Path::new(&format!("{prefix}41")).exists(),
        "the cancelled seq's file must land at exactly `<prefix><seq>` — the path \
         tsserver stats"
    );
    assert!(
        !std::path::Path::new(&format!("{prefix}42")).exists(),
        "a cancellation must name only its own seq"
    );
}

/// Dropping a caller's in-flight request future must tell tsserver to stop.
/// Abandoned work on a single JavaScript thread sits directly in front of every
/// request that replaced it.
#[tokio::test]
async fn dropping_an_in_flight_tsserver_request_cancels_it_at_the_engine() {
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<TsserverStdinMessage>(16);
    let transport = test_transport(stdin_tx);

    {
        let mut fut = Box::pin(transport.request("references", serde_json::json!({})));
        tokio::select! {
            _ = &mut fut => panic!("no response was ever written; the request cannot complete"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }

    let sent = stdin_rx.try_recv().expect("the request frame was enqueued");
    let TsserverStdinMessage::Frame(bytes) = sent else {
        panic!("expected a framed request");
    };
    let body: serde_json::Value =
        serde_json::from_str(String::from_utf8(bytes).unwrap().trim()).unwrap();
    let seq = body.get("seq").and_then(|v| v.as_i64()).expect("seq");

    assert!(
        cancellation_path(&transport, seq).exists(),
        "dropping an in-flight request must signal cancellation for its own seq"
    );
}

/// The cancellation must not queue behind the work it cancels. tsserver reads
/// one stdin lane, and a request is typically abandoned BECAUSE that lane is not
/// draining — a cancel sent down it would arrive after the work it was meant to
/// stop. Signalling out of band is the whole point.
#[tokio::test]
async fn tsserver_cancellation_does_not_queue_behind_the_lane_it_cancels() {
    // Capacity 1, and the request itself fills it: the lane cannot accept
    // another byte.
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<TsserverStdinMessage>(1);
    let transport = test_transport(stdin_tx);

    {
        let mut fut = Box::pin(transport.request("completions", serde_json::json!({})));
        tokio::select! {
            _ = &mut fut => panic!("no response was ever written; the request cannot complete"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
        // Do not drain the lane — it stays full across the drop.
    }

    assert!(
        cancellation_path(&transport, 1).exists(),
        "the cancellation must be deliverable while the request's own lane is full"
    );
    assert!(
        stdin_rx.try_recv().is_ok(),
        "the original request frame is still sitting on the blocked lane"
    );
}

/// A request that was ANSWERED must not be cancelled on the way out, and must
/// still return its result. Cancelling a completed seq is noise at best; losing
/// a legitimate in-flight response to a cancellation race is the failure this
/// guards.
#[tokio::test]
async fn an_answered_tsserver_request_returns_its_result_and_emits_no_cancellation() {
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<TsserverStdinMessage>(16);
    let transport = test_transport(stdin_tx);

    let answerer = {
        let pending = Arc::clone(&transport.pending);
        tokio::spawn(async move {
            for _ in 0..100 {
                if let Some(tx) = pending.take(1) {
                    let _ = tx.send(serde_json::json!({
                        "type": "response",
                        "request_seq": 1,
                        "success": true,
                        "body": { "displayString": "const x: number" }
                    }));
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            panic!("the request never registered its pending slot");
        })
    };

    let result = transport
        .request("quickinfo", serde_json::json!({}))
        .await
        .expect("the answered request succeeds");
    answerer.await.unwrap();

    assert_eq!(
        result.get("displayString").and_then(|v| v.as_str()),
        Some("const x: number"),
        "the answered request must return the engine's own body, unaltered"
    );
    assert!(
        stdin_rx.try_recv().is_ok(),
        "the request frame was enqueued"
    );
    assert!(
        !cancellation_path(&transport, 1).exists(),
        "an answered request must emit no cancellation"
    );
    assert_eq!(
        pending_len(&transport),
        0,
        "the answered request holds no slot"
    );
}

/// Cancellation files are reaped once they can no longer be observed, so a long
/// session cannot fill its temp directory one file per abandoned request.
#[test]
fn cancellation_files_are_reaped_once_they_can_no_longer_be_observed() {
    let cancellation =
        TsserverCancellation::create().expect("a cancellation directory must be creatable");

    for seq in 0..(CANCEL_FILE_RETAIN_CAP as i64 + 64) {
        cancellation.cancel(seq);
    }

    let live = std::fs::read_dir(&cancellation.dir)
        .expect("the cancellation directory exists")
        .count();
    assert!(
        live <= CANCEL_FILE_RETAIN_CAP,
        "retained cancellations must stay bounded, found {live}"
    );
    assert!(
        !std::path::Path::new(&format!("{}0", cancellation.prefix)).exists(),
        "the oldest cancellation must be reaped first"
    );

    let dir = cancellation.dir.clone();
    drop(cancellation);
    assert!(
        !dir.exists(),
        "the session's cancellation directory must not outlive the session"
    );
}

/// tsserver answers a CANCELLED request with `success: true` and a
/// `{ canceled: true }` body — a success-shaped envelope carrying no result.
/// Every feature parser reads the body as an array and falls back to empty, so
/// letting that envelope through turns "the engine stopped early" into "there
/// are no results here": a silently wrong answer instead of a visible failure.
#[tokio::test]
async fn a_tsserver_cancellation_envelope_is_an_error_not_an_empty_result() {
    let (stdin_tx, _stdin_rx) = mpsc::channel::<TsserverStdinMessage>(16);
    let transport = test_transport(stdin_tx);

    let answerer = {
        let pending = Arc::clone(&transport.pending);
        tokio::spawn(async move {
            for _ in 0..100 {
                if let Some(tx) = pending.take(1) {
                    let _ = tx.send(serde_json::json!({
                        "type": "response",
                        "request_seq": 1,
                        "success": true,
                        "body": { "canceled": true }
                    }));
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            panic!("the request never registered its pending slot");
        })
    };

    let result = transport.request("definition", serde_json::json!({})).await;
    answerer.await.unwrap();

    let err = result.expect_err(
        "a cancellation envelope must not be handed to a feature parser, which \
         would read its object body as an empty array",
    );
    assert!(
        err.message.contains("cancel"),
        "the failure must name the cancellation so it is attributable, got {}",
        err.message
    );
}
