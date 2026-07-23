//! Live process-lifetime contract: the standard LSP client process witness
//! terminates `verter-lsp` even while its stdin remains open.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct ChildCleanup(Option<Child>);

impl Drop for ChildCleanup {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_idle_client() -> Child {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = Command::new("powershell");
        command
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 300"])
            .creation_flags(CREATE_NO_WINDOW);
        command.spawn().expect("spawn idle fake IDE client")
    }
    #[cfg(unix)]
    {
        Command::new("sleep")
            .arg("300")
            .spawn()
            .expect("spawn idle fake IDE client")
    }
}

fn initialize_with_standard_client_pid(lsp: &mut Child, client_pid: u32) {
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"processId":{client_pid},"capabilities":{{}},"rootUri":null}}}}"#
    );
    let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    lsp.stdin
        .as_mut()
        .expect("LSP stdin remains open")
        .write_all(frame.as_bytes())
        .expect("write initialize request");
    lsp.stdin
        .as_mut()
        .expect("LSP stdin remains open")
        .flush()
        .expect("flush initialize request");

    let stdout = lsp.stdout.take().expect("LSP stdout is piped");
    let (response_tx, response_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut content_length = None;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                let _ = response_tx.send(Err("LSP stdout closed before initialize".to_string()));
                return;
            }
            if header == "\r\n" {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
        let Some(content_length) = content_length else {
            let _ = response_tx.send(Err("initialize response has no Content-Length".to_string()));
            return;
        };
        let mut body = vec![0; content_length];
        if let Err(error) = reader.read_exact(&mut body) {
            let _ = response_tx.send(Err(format!("read initialize response: {error}")));
            return;
        }
        let _ = response_tx.send(String::from_utf8(body).map_err(|error| error.to_string()));

        // Keep the stdout pipe owned until the LSP exits; only stdin/client
        // lifetime should drive this test's shutdown.
        let mut discard = Vec::new();
        let _ = reader.read_to_end(&mut discard);
    });

    let response = response_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("initialize response timeout")
        .expect("initialize response read");
    assert!(
        response.contains("\"result\""),
        "initialize failed: {response}"
    );
}

// @ai-generated
#[test]
fn standard_lsp_client_death_terminates_the_lsp_with_open_stdio() {
    let client = spawn_idle_client();
    let client_pid = client.id();
    let mut client = ChildCleanup(Some(client));

    let lsp = Command::new(env!("CARGO_BIN_EXE_verter-lsp"))
        .arg("--type-provider=off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn contained verter-lsp");
    let mut lsp = ChildCleanup(Some(lsp));
    initialize_with_standard_client_pid(lsp.0.as_mut().unwrap(), client_pid);

    assert!(
        lsp.0.as_mut().unwrap().try_wait().unwrap().is_none(),
        "the LSP should remain alive while its standard client witness is alive"
    );

    client.0.as_mut().unwrap().kill().expect("kill fake client");
    let _ = client.0.as_mut().unwrap().wait();
    client.0 = None;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if lsp.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            lsp.0 = None;
            break;
        }
        assert!(
            Instant::now() < deadline,
            "verter-lsp survived its standard LSP client process; provider executables could be orphaned"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
