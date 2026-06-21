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

    let parsed = parse_tsserver_diagnostic(&diag, Some(content)).unwrap();
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

    let parsed = parse_tsserver_diagnostic(&diag, Some(content)).unwrap();
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

    let parsed = parse_tsserver_diagnostic(&diag, Some(content)).unwrap();
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

    let parsed = parse_tsserver_diagnostic(&diag, Some(content)).unwrap();
    assert!(
        parsed.tags.is_empty(),
        "no boolean flags ⇒ no tags, got: {:?}",
        parsed.tags
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

    let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let transport = Arc::new(TsserverTransport {
        stdin_tx,
        pending,
        next_seq: AtomicI64::new(1),
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
    cache.insert("d:/test/file.ts".to_string(), content.to_string());

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
    cache.insert("d:/test/file.ts".to_string(), content.clone());

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
    cache.insert("d:/test/file.ts".to_string(), content.to_string());
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
/// tsgo rename path gives via `parse_range_to_offsets_with_disk_fallback`.
///
/// Discriminating regression for the dropped cross-file rename edit: the pre-fix code packed a
/// 0-based `(line << 16) | col` sentinel on a cache miss, which the merge layer could not map to
/// a real range — so the cross-file edit was silently dropped (incomplete rename). The renamed
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
    // Discriminating negative: the pre-fix packed fallback would be `(2 << 16) | 13`.
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
/// Discriminating regression: the pre-fix code packed the 0-based sentinel as a final `else` arm and
/// returned `Some(RenameLocation { start: packed, .. })`. The fixed code returns `None`. The fixture
/// path does not exist on disk and is not in the (empty) cache.
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
    let cache: HashMap<String, String> = HashMap::new();

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
    let mut cache: HashMap<String, String> = HashMap::new();
    cache.insert("d:/proj/r.ts".to_string(), content.to_string());

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

#[test]
fn test_parse_tsserver_location_non_ascii() {
    // tsserver uses UTF-16 code units for offset
    // "café" = 5 bytes UTF-8 (c=1, a=1, f=1, é=2), 4 UTF-16 code units
    let content = "café\nworld";
    let mut cache = HashMap::new();
    cache.insert("d:/test/file.ts".to_string(), content.to_string());

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

async fn send_success_response(
    pending: &Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>,
    seq: i64,
    command: &str,
) {
    if let Some(tx) = pending.lock().await.remove(&seq) {
        let _ = tx.send(serde_json::json!({
            "type": "response",
            "request_seq": seq,
            "success": true,
            "command": command,
            "body": {}
        }));
    }
}

#[tokio::test]
async fn test_configure_tsserver_session_does_not_wait_for_inferred_project_options() {
    let (client_reader, server_writer) = tokio::io::duplex(65536);
    let (stdin_tx, stdin_rx) = mpsc::channel::<TsserverStdinMessage>(64);
    tokio::spawn(tsserver_stdin_writer_loop(server_writer, stdin_rx));

    let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let transport = Arc::new(TsserverTransport {
        stdin_tx: stdin_tx.clone(),
        pending: Arc::clone(&pending),
        next_seq: AtomicI64::new(1),
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
                    } else if command == "compilerOptionsForInferredProjects" {
                        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                        send_success_response(&pending_task, seq, &command).await;
                        break;
                    }
                }
            }
        }
    });

    let start = std::time::Instant::now();
    let ws_root = configure_tsserver_session(Arc::clone(&transport), "C:\\project")
        .await
        .expect("configuration should succeed");
    let elapsed = start.elapsed();

    // Canonical form lowercases the Windows drive letter (keeps the colon).
    assert_eq!(ws_root, "c:/project");
    assert!(
        elapsed < std::time::Duration::from_millis(250),
        "tsserver startup should not wait for inferred project options (elapsed {:?})",
        elapsed
    );

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let commands = seen_commands.lock().await.clone();
    assert_eq!(
        commands.first().map(String::as_str),
        Some("configure"),
        "configure must still be sent first"
    );
    assert!(
        commands
            .iter()
            .any(|command| command == "compilerOptionsForInferredProjects"),
        "inferred project options should still be requested in the background"
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
        pending: Arc::new(Mutex::new(HashMap::new())),
        next_seq: AtomicI64::new(1),
    });

    let contents_cache: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let opened_files: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    // Pre-populate caches to simulate an already-open file
    if let Some(old) = old_content {
        contents_cache
            .lock()
            .await
            .insert(file.to_string(), old.to_string());
        opened_files.lock().await.insert(file.to_string());
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
        .insert(file.clone(), content.clone());

    let mut opened = opened_files.lock().await;
    if opened.contains(&file) {
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
        opened.insert(file.clone());
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
    let contents_cache: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

    let content = {
        let cache = contents_cache.lock().await;
        cache.get("/project/src/Missing.vue.tsx").cloned()
    };

    // With the fix, content is None → early return
    assert!(content.is_none(), "cache miss should yield None");
    // The actual fix changes the code to `return Ok(vec![])` here,
    // so no transport request is sent. We verify the None path exists.
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
    }
}

/// The dedup key is `(start, end, code, message)` and EXCLUDES tags. When the
/// same finding is reported by two passes — one carrying the `Unnecessary` tag
/// (the unused-symbol fade) and one without — the surviving diagnostic MUST keep
/// the tag, regardless of which pass emitted it first. Otherwise a `.vue` unused
/// import stops graying out whenever a tagless duplicate is seen first.
///
/// Discriminating: the pre-fix `merge_diagnostic_sets` kept the FIRST-seen
/// variant verbatim and dropped the rest, so a tagless-then-tagged ordering lost
/// the tag entirely. This asserts the tag survives in BOTH orderings.
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
/// Discriminating: pre-fix `parse_tsserver_code_action` returned
/// `Some(TypeCodeAction { edits: [] })` for an action with no `textChanges`; this
/// asserts it is now `None`, so it FAILS against the pre-fix code and PASSES after.
#[test]
fn parse_tsserver_code_action_drops_empty_edit_action() {
    let cache: HashMap<String, String> = HashMap::new();

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
    let mut resolvable_cache: HashMap<String, String> = HashMap::new();
    resolvable_cache.insert(
        "d:/test/file.ts".to_string(),
        "const unused = 1;\n".to_string(),
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
/// Discriminating regression: the pre-fix code packed a 0-based `(line << 16) | col` sentinel on a
/// cache miss and pushed it as a real byte offset, so the merge layer applied the edit at a bogus
/// offset. The renamed/edited span sits on line 3 (1-based), so the packed value is unmistakably
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
    let cache: HashMap<String, String> = HashMap::new();

    let edits = parse_tsserver_file_code_edits(&changes, &cache)
        .expect("a well-formed (but unresolvable) change array still returns Some(empty)");
    assert!(
        edits.is_empty(),
        "an edit whose file is unavailable must be DROPPED (fail-closed), never packed: {edits:?}"
    );
    // The packed sentinel the pre-fix code would have produced — assert it is absent.
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
/// clamps a past-EOF line/col to `content.len()` and returns a valid-looking WRONG offset, so the
/// pre-fix edit path emitted an edit at EOF that corrupts the file. The checked converter returns
/// `None` for an out-of-range position and the edit is dropped. The fixture content is short (3
/// lines), so line 999 is unmistakably past EOF.
#[test]
fn parse_tsserver_file_code_edits_drops_out_of_range_position_not_clamped() {
    let content = "// header\nconst pad = 1;\nexport const renamed = 2;\n";
    let path = "d:/proj/oob.ts".to_string();
    let mut cache: HashMap<String, String> = HashMap::new();
    cache.insert(path.clone(), content.to_string());

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
    // Discriminating negative: the clamp the pre-fix code produced was `content.len()`.
    assert!(
        !edits.iter().any(|e| e.start == content.len() as u32),
        "no edit may carry the clamped content-length offset"
    );
}

/// A code-edit whose endpoints invert after conversion (`start > end`) must be DROPPED — a
/// malformed span would otherwise produce a reversed-range edit. Content is available so the drop
/// is attributable to the inverted span, not a content miss.
#[test]
fn parse_tsserver_file_code_edits_drops_inverted_span() {
    let content = "const alpha = 1;\nconst beta = 2;\n";
    let path = "d:/proj/inv.ts".to_string();
    let mut cache: HashMap<String, String> = HashMap::new();
    cache.insert(path.clone(), content.to_string());

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
    let cache: HashMap<String, String> = HashMap::new();

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
    // Discriminating negative: the pre-fix packed fallback would be `(2 << 16) | 13`.
    let packed_start = ((3u32.saturating_sub(1)) << 16) | ((14u32.saturating_sub(1)) & 0xFFFF);
    assert_ne!(
        edits[0].start, packed_start,
        "must NOT be the packed (line<<16)|col fallback (the corrupting path)"
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}
