//! Batch bridge used by the pinned Svelte golden generator.
//!
//! Input is a JSON array of raw emitted JavaScript modules on stdin; output is
//! the corresponding JSON array of semantic-comment signatures. One process
//! handles the whole corpus so golden regeneration does not spawn per fixture.

use std::io::{self, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let modules: Vec<String> = serde_json::from_str(&input)?;
    let mut signatures = Vec::with_capacity(modules.len());
    for (index, module) in modules.iter().enumerate() {
        let signature = verter_compiler::svelte_semantic_comments::semantic_comment_signature(
            module,
        )
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "module {index} is invalid emitted JavaScript ({} parser diagnostic(s))",
                    error.diagnostic_count
                ),
            )
        })?;
        signatures.push(signature);
    }
    serde_json::to_writer(io::stdout().lock(), &signatures)?;
    Ok(())
}
