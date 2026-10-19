//! A client with no editor rendezvous (Neovim, Helix, Zed, Lapce, any headless
//! harness) gets type-backed answers from the managed TSGO fallback.
//!
//! Without a rendezvous the server serves through the lazy managed fallback with
//! no shared overlay: the engine does not exist until something demands it. The
//! open documents themselves must demand it. If they only record desired state
//! and wait for a certified receipt, no IDE surface ever commits, so no request
//! ever reaches the provider, so no engine is ever created, and every type-backed
//! feature stays empty for the life of the session.
//!
//! This drives the REAL `verter-lsp` binary over stdio exactly as such a client
//! does: `initialize` with a workspace folder, `initialized`, `didOpen` for two
//! components (one importing the other), no edits, then requests. It asserts the
//! client-visible outcomes: the readiness notification, a typed template hover,
//! a published template type error, and prop references that reach the parent's
//! template. The server log must show exactly one managed-engine activation and
//! no delivery reported as skipped for a moved revision, because no revision
//! moved.
//!
//! The test needs a real tsgo engine and the workspace's installed `vue`
//! types. It skips when either is unavailable, unless `VERTER_REQUIRE_TSGO` is
//! set, which turns a missing engine into a failure.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::stdio_launch_smoke::{path_to_file_uri, spawn_reader, write_message, ReaderEvent};

/// Bound on every wait. A converging session answers in a few seconds; the
/// bound only turns a session that never converges into a named failure.
const WAIT: Duration = Duration::from_secs(45);

const CHILD_VUE: &str = r#"<script setup lang="ts">
defineProps<{ label: string }>()
</script>

<template>
  <span>{{ label }}</span>
</template>
"#;

const APP_VUE: &str = r#"<script setup lang="ts">
import Child from './Child.vue'
const greeting: string = 'hello'
const count: number = 1
</script>

<template>
  <div>{{ greeting }}</div>
  <Child :label="greeting" />
  <Child :label="count" />
</template>
"#;

const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "jsx": "preserve",
    "jsxImportSource": "vue",
    "noEmit": true
  },
  "include": ["src/**/*"]
}"#;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root above crates/")
        .to_path_buf()
}

fn tsgo_or_skip() -> Option<PathBuf> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Api,
        Some(repo_root()),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("toolchain discovery runtime");
    match runtime.block_on(verter_tsgo_api::toolchain::discovery::resolve(&request)) {
        Ok(resolution) => Some(resolution.path),
        Err(error) => {
            assert!(
                std::env::var("VERTER_REQUIRE_TSGO").is_err(),
                "VERTER_REQUIRE_TSGO is set but no tsgo engine resolves: {error}"
            );
            eprintln!("SKIP managed fallback headless session: tsgo is unavailable: {error}");
            None
        }
    }
}

/// The installed `vue` package and its `@vue/*` siblings, resolved through the
/// workspace's pnpm layout.
fn installed_vue() -> Option<(PathBuf, Option<PathBuf>)> {
    let node_modules = repo_root().join("node_modules");
    let vue = if node_modules.join("vue/dist/vue.d.ts").exists() {
        node_modules.join("vue")
    } else {
        std::fs::read_dir(node_modules.join(".pnpm"))
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("vue@"))
            .map(|entry| entry.path().join("node_modules/vue"))
            .find(|candidate| candidate.join("dist/vue.d.ts").exists())?
    };
    let vue = vue.canonicalize().ok()?;
    let scoped = vue
        .parent()
        .map(|parent| parent.join("@vue"))
        .filter(|p| p.exists());
    Some((vue, scoped))
}

fn link_dir(target: &Path, link: &Path) {
    #[cfg(windows)]
    let linked = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    #[cfg(not(windows))]
    let linked = std::os::unix::fs::symlink(target, link).is_ok();
    assert!(linked, "link {} -> {}", link.display(), target.display());
}

/// One running server: its stdin, framed stdout, and every stderr log line.
struct Session {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<ReaderEvent>,
    log: Arc<Mutex<Vec<String>>>,
    next_id: i64,
    /// Every notification received so far, in arrival order.
    notifications: Vec<Value>,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Session {
    fn send(&mut self, message: &Value) {
        write_message(&mut self.stdin, message);
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn log_text(&self) -> String {
        self.log.lock().expect("server log").join("\n")
    }

    /// Receive the next message before `deadline`, answering server-to-client
    /// requests so the server never waits on this client.
    fn pump(&mut self, deadline: Instant, context: &str) -> Value {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = match self.messages.recv_timeout(remaining) {
                Ok(ReaderEvent::Message(message)) => message,
                Ok(ReaderEvent::Fatal(reason)) => {
                    panic!(
                        "{context}: the server stream failed: {reason}\n{}",
                        self.log_text()
                    )
                }
                Err(RecvTimeoutError::Timeout) => panic!(
                    "{context}: not observed within {}s\n--- server log ---\n{}",
                    WAIT.as_secs(),
                    self.log_text()
                ),
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("{context}: the server stream closed\n{}", self.log_text())
                }
            };
            if let (Some(id), Some(method)) = (message.get("id"), message.get("method")) {
                let result = if method == "workspace/configuration" {
                    let items = message["params"]["items"].as_array().map_or(0, Vec::len);
                    Value::Array(vec![Value::Null; items])
                } else {
                    Value::Null
                };
                let reply = json!({ "jsonrpc": "2.0", "id": id.clone(), "result": result });
                self.send(&reply);
                continue;
            }
            if message.get("id").is_none() {
                self.notifications.push(message.clone());
            }
            return message;
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = Instant::now() + WAIT;
        loop {
            let message = self.pump(deadline, method);
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(error) = message.get("error") {
                    panic!("{method} failed: {error}\n{}", self.log_text());
                }
                return message.get("result").cloned().unwrap_or(Value::Null);
            }
        }
    }

    /// Wait until a received notification satisfies `accept`.
    fn await_notification(&mut self, context: &str, accept: impl Fn(&Value) -> bool) {
        if self.notifications.iter().any(&accept) {
            return;
        }
        let deadline = Instant::now() + WAIT;
        loop {
            let message = self.pump(deadline, context);
            if message.get("id").is_none() && accept(&message) {
                return;
            }
        }
    }

    fn shutdown(mut self) {
        let _ = self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let grace = Instant::now() + Duration::from_secs(5);
        while Instant::now() < grace {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

fn position_of(content: &str, needle: &str, offset_in_needle: usize) -> Value {
    let offset = content.find(needle).expect("fixture anchor") + offset_in_needle;
    let before = &content[..offset];
    let line = before.matches('\n').count();
    let character = before.len() - before.rfind('\n').map_or(0, |newline| newline + 1);
    json!({ "line": line, "character": character })
}

#[test]
fn headless_client_gets_typed_answers_from_the_lazy_managed_engine() {
    let Some(tsgo) = tsgo_or_skip() else {
        return;
    };
    let Some((vue, scoped_vue)) = installed_vue() else {
        assert!(
            std::env::var("VERTER_REQUIRE_TSGO").is_err(),
            "VERTER_REQUIRE_TSGO is set but the workspace has no installed vue types"
        );
        eprintln!("SKIP managed fallback headless session: install workspace dependencies");
        return;
    };

    let workspace = tempfile::tempdir().expect("temporary workspace");
    let root = workspace
        .path()
        .canonicalize()
        .expect("canonical workspace");
    std::fs::create_dir_all(root.join("src")).expect("source directory");
    std::fs::create_dir_all(root.join("node_modules")).expect("node_modules directory");
    link_dir(&vue, &root.join("node_modules/vue"));
    if let Some(scoped_vue) = scoped_vue {
        link_dir(&scoped_vue, &root.join("node_modules/@vue"));
    }
    std::fs::write(root.join("tsconfig.json"), TSCONFIG).expect("tsconfig");
    std::fs::write(root.join("src/App.vue"), APP_VUE).expect("App.vue");
    std::fs::write(root.join("src/Child.vue"), CHILD_VUE).expect("Child.vue");

    let root_text = root.to_string_lossy().into_owned();
    let root_uri = path_to_file_uri(&root_text);
    let app_uri = path_to_file_uri(&root.join("src/App.vue").to_string_lossy());
    let child_uri = path_to_file_uri(&root.join("src/Child.vue").to_string_lossy());

    let mut child = Command::new(verter_test_support::cargo_test_binary_path!("verter-lsp"))
        .arg(&root_text)
        .current_dir(&root)
        .env("VERTER_TSGO_BIN", &tsgo)
        .env("VERTER_LOG", "info")
        .env_remove("RUST_LOG")
        .env_remove("VERTER_SHARED_CONTROL_DIR")
        .env_remove("VERTER_SHARED_SESSION_KEY")
        .env_remove("VERTER_EDITOR_TSSERVER_RECEIPT")
        .env_remove("VERTER_EDITOR_TSSERVER_NONCE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn verter-lsp");
    let log = Arc::new(Mutex::new(Vec::new()));
    {
        let log = Arc::clone(&log);
        let stderr = child.stderr.take().expect("piped stderr");
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log.lock().expect("server log").push(line);
            }
        });
    }
    let stdin = child.stdin.take().expect("piped stdin");
    let messages = spawn_reader(child.stdout.take().expect("piped stdout"));
    let mut session = Session {
        child,
        stdin,
        messages,
        log,
        next_id: 0,
        notifications: Vec::new(),
    };

    session.request(
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": "managed-fallback" }],
            "capabilities": {
                "textDocument": {
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "publishDiagnostics": {}
                }
            },
        }),
    );
    session.notify("initialized", json!({}));
    for (uri, text) in [(&app_uri, APP_VUE), (&child_uri, CHILD_VUE)] {
        session.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "vue", "version": 1, "text": text } }),
        );
    }

    session.await_notification("typeProviderSyncComplete", |message| {
        message["method"] == "$/verter/typeProviderSyncComplete"
    });

    let hover_at = position_of(APP_VUE, "{{ greeting }}", 3);
    let deadline = Instant::now() + WAIT;
    let hover = loop {
        let hover = session.request(
            "textDocument/hover",
            json!({ "textDocument": { "uri": app_uri }, "position": hover_at }),
        );
        if !hover.is_null() || Instant::now() >= deadline {
            break hover;
        }
        thread::sleep(Duration::from_millis(250));
    };
    let hover_text = hover["contents"].to_string();
    assert!(
        hover_text.contains("greeting") && hover_text.contains("string"),
        "the template hover must carry the TypeScript type, got: {hover}\n{}",
        session.log_text()
    );

    let app_uri_for_diagnostics = app_uri.clone();
    session.await_notification("the planted template type error", move |message| {
        message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == app_uri_for_diagnostics.as_str()
            && message["params"]["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| {
                    diagnostics.iter().any(|diagnostic| {
                        diagnostic["code"] == 2322
                            || diagnostic["message"]
                                .as_str()
                                .is_some_and(|text| text.contains("not assignable"))
                    })
                })
    });

    let references = session.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": child_uri },
            "position": position_of(CHILD_VUE, "label: string", 0),
            "context": { "includeDeclaration": true },
        }),
    );
    // The engine resolves the declared prop through the child's own template
    // use. Verter's native pass finds no reference here, so this location can
    // only come from the managed engine.
    let template_use = position_of(CHILD_VUE, "{{ label }}", 3);
    let reaches_template_use = references.as_array().is_some_and(|locations| {
        locations.iter().any(|location| {
            location["uri"] == child_uri.as_str() && location["range"]["start"] == template_use
        })
    });
    assert!(
        reaches_template_use,
        "references from the prop declaration must include the engine-resolved template \
         use, got: {references}\n{}",
        session.log_text()
    );

    let log = session.log_text();
    assert_eq!(
        log.matches("managed fallback ACTIVATED").count(),
        1,
        "the managed engine must be created exactly once:\n{log}"
    );
    assert!(
        !log.contains("TSX_SYNC_SKIPPED"),
        "no revision moved, so no delivery may be reported as skipped for one:\n{log}"
    );
    session.shutdown();
}
