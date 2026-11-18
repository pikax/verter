// A grammar witness says a compiler producer wrote a fragment in one grammar
// context, so publication may trust the bytes without parsing them. It has no
// constructor and no struct literal outside its owner.

use verter_compiler::assembly::fragment::{FragmentDialect, GrammarWitness, SyntacticContract};

fn main() {
    let _literal = GrammarWitness {
        contract: SyntacticContract::CompleteModule,
        dialect: FragmentDialect::JavaScript,
    };
}
