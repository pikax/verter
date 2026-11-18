// Only the compiler's own producers vouch for the grammar of what they write:
// the producer trait is sealed, so no other crate can implement it and mint a
// witness through it. A witness is what lets publication skip parsing a
// module, so a forged one would let unchecked bytes through unparsed.
//

use verter_compiler::assembly::fragment::{GrammarProducer, SyntacticContract};

/// A producer outside the compiler.
struct ForgedProducer;

impl GrammarProducer for ForgedProducer {
    const CONTRACT: SyntacticContract = SyntacticContract::CompleteModule;
}

fn main() {}
