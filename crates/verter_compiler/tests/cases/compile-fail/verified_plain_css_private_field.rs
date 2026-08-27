use verter_compiler::style_planner::VerifiedPlainCss;
use verter_css_syntax::StyleSyntaxIr;

fn forge(ir: &StyleSyntaxIr) -> VerifiedPlainCss<'_> {
    VerifiedPlainCss { ir }
}

fn main() {}
