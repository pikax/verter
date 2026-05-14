//! `cutover-state` — read/write `.cutover-state` TOML.
//!
//! Subcommands:
//!   `dispatch <block-id>` — sets `active_block` to `<block-id>`.
//!   `land <block-id>`    — appends `<block-id>` to `landed_blocks`,
//!                          clears `active_block`.
//!
//! Exits non-zero on schema validation errors.

use std::path::{Path, PathBuf};
use std::process;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct CutoverState {
    active_block: String,
    landed_blocks: Vec<String>,
}

fn state_path(root: &Path) -> PathBuf {
    root.join(".cutover-state")
}

fn repo_root() -> PathBuf {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse --show-toplevel failed");
    let s = String::from_utf8(out.stdout).expect("non-utf8 git output");
    PathBuf::from(s.trim())
}

fn read_state(path: &Path) -> CutoverState {
    match std::fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<CutoverState>(&content) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: failed to parse .cutover-state TOML: {e}");
                process::exit(1);
            }
        },
        Err(_) => CutoverState::default(),
    }
}

fn write_state(path: &Path, state: &CutoverState) {
    let header = "# Stage 7 cutover state. Owned by the cutover-state xtask.\n\
                  # This file is deleted at Block 10 completion.\n";
    let body = toml::to_string_pretty(state).expect("failed to serialise cutover state");
    let content = format!("{header}\n{body}");
    std::fs::write(path, content).unwrap_or_else(|e| {
        eprintln!("Error: failed to write .cutover-state: {e}");
        process::exit(1);
    });
}

fn validate_block_id(id: &str) -> bool {
    // Block IDs are non-empty strings matching [0-9A-Za-z_.-]+
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
}

fn cmd_dispatch(block_id: &str, root: &Path) {
    if !validate_block_id(block_id) {
        eprintln!("Error: invalid block-id {:?}", block_id);
        process::exit(1);
    }
    let path = state_path(root);
    let mut state = read_state(&path);
    state.active_block = block_id.to_owned();
    write_state(&path, &state);
    println!("active_block = {:?}", state.active_block);
}

fn cmd_land(block_id: &str, root: &Path) {
    if !validate_block_id(block_id) {
        eprintln!("Error: invalid block-id {:?}", block_id);
        process::exit(1);
    }
    let path = state_path(root);
    let mut state = read_state(&path);
    if !state.landed_blocks.contains(&block_id.to_owned()) {
        state.landed_blocks.push(block_id.to_owned());
    }
    state.active_block = String::new();
    write_state(&path, &state);
    println!("landed_blocks = {:?}", state.landed_blocks);
}

fn cmd_show(root: &Path) {
    let path = state_path(root);
    let state = read_state(&path);
    println!("active_block  = {:?}", state.active_block);
    println!("landed_blocks = {:?}", state.landed_blocks);
}

fn usage() -> ! {
    eprintln!(
        "Usage: cutover-state <subcommand> [args]\n\
         Subcommands:\n\
           dispatch <block-id>  set active_block\n\
           land <block-id>      append to landed_blocks, clear active_block\n\
           show                 print current state\n"
    );
    process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }

    let root = repo_root();

    match args[1].as_str() {
        "dispatch" => {
            if args.len() < 3 {
                eprintln!("Error: dispatch requires a block-id argument");
                process::exit(1);
            }
            cmd_dispatch(&args[2], &root);
        }
        "land" => {
            if args.len() < 3 {
                eprintln!("Error: land requires a block-id argument");
                process::exit(1);
            }
            cmd_land(&args[2], &root);
        }
        "show" => {
            cmd_show(&root);
        }
        other => {
            eprintln!("Error: unknown subcommand {:?}", other);
            usage();
        }
    }
}
