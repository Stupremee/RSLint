mod expr;
mod stmt;
mod visit;

pub(crate) use visit::Visit;

use rslint_parser::{ast::Pattern, AstNode};
use types::{internment::Intern, Pattern as DatalogPattern};

pub(super) struct AnalyzerInner;

impl AnalyzerInner {
    fn visit_pattern(&self, pattern: Pattern) -> Intern<DatalogPattern> {
        match pattern {
            Pattern::SinglePattern(single) => Intern::new(DatalogPattern {
                name: Intern::new(single.text()),
            }),

            // FIXME: Implement the rest of the patterns
            _ => Intern::new(DatalogPattern {
                name: Intern::new(String::from("TODO")),
            }),
        }
    }
}
