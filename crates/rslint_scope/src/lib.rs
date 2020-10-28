mod datalog;
mod visit;

pub use datalog::{
    Datalog, DatalogBuilder, DatalogFunction, DatalogResult, DatalogScope, DatalogTransaction,
};

use rslint_core::{
    rule_prelude::{
        ast::{Decl, Expr, FnDecl, Literal, LiteralKind, NameRef, Pattern, Stmt, VarDecl},
        AstNode, SyntaxNode, SyntaxNodeExt,
    },
    CstRule, Rule, RuleCtx,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use types::{
    internment::{self, Intern},
    ExprId, Pattern as DatalogPattern,
};
use visit::Visit;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScopeAnalyzer {
    #[serde(skip)]
    datalog: Arc<Mutex<Option<Datalog>>>,
}

impl ScopeAnalyzer {
    pub fn new() -> DatalogResult<Self> {
        Ok(Self {
            datalog: Arc::new(Mutex::new(Some(Datalog::new()?))),
        })
    }

    pub fn analyze(&self, syntax: &SyntaxNode) -> DatalogResult<()> {
        let mut lock = self.datalog.lock().unwrap();
        let mut datalog = lock.take().unwrap();

        datalog.transaction(|trans| {
            let scope = trans.scope();
            for stmt in syntax.children().filter_map(|node| node.try_to::<Stmt>()) {
                (&*self).visit(&scope, stmt);
            }

            Ok(())
        })?;

        *lock = Some(datalog);

        Ok(())
    }

    fn visit_pattern(&self, pattern: Pattern) -> Intern<DatalogPattern> {
        match pattern {
            Pattern::SinglePattern(single) => internment::intern(&DatalogPattern {
                name: internment::intern(&single.text()),
            }),

            _ => todo!(),
        }
    }
}

impl Rule for ScopeAnalyzer {
    fn name(&self) -> &'static str {
        "scope-analysis"
    }

    fn group(&self) -> &'static str {
        "errors"
    }
}

#[typetag::serde]
impl CstRule for ScopeAnalyzer {
    fn check_root(&self, root: &SyntaxNode, _ctx: &mut RuleCtx) -> Option<()> {
        if let Err(err) = self.analyze(root) {
            eprintln!("Datalog error: {:?}", err);
        }

        Some(())
    }
}

impl<'ddlog> Visit<'ddlog, Stmt> for ScopeAnalyzer {
    type Output = Option<DatalogScope<'ddlog>>;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, stmt: Stmt) -> Self::Output {
        match stmt {
            Stmt::BlockStmt(_) => {}
            Stmt::EmptyStmt(_) => {}
            Stmt::ExprStmt(expr) => {
                expr.expr().map(|expr| self.visit(scope, expr));
            }
            Stmt::IfStmt(_) => {}
            Stmt::DoWhileStmt(_) => {}
            Stmt::WhileStmt(_) => {}
            Stmt::ForStmt(_) => {}
            Stmt::ForInStmt(_) => {}
            Stmt::ContinueStmt(_) => {}
            Stmt::BreakStmt(_) => {}
            Stmt::ReturnStmt(_) => {}
            Stmt::WithStmt(_) => {}
            Stmt::LabelledStmt(_) => {}
            Stmt::SwitchStmt(_) => {}
            Stmt::ThrowStmt(_) => {}
            Stmt::TryStmt(_) => {}
            Stmt::DebuggerStmt(_) => {}
            Stmt::Decl(decl) => return self.visit(scope, decl),
        }

        None
    }
}

impl<'ddlog> Visit<'ddlog, Decl> for ScopeAnalyzer {
    type Output = Option<DatalogScope<'ddlog>>;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, decl: Decl) -> Self::Output {
        match decl {
            Decl::FnDecl(func) => {
                self.visit(scope, func);
                None
            }
            Decl::ClassDecl(_) => None,
            Decl::VarDecl(var) => Some(self.visit(scope, var)),
        }
    }
}

impl<'ddlog> Visit<'ddlog, FnDecl> for ScopeAnalyzer {
    type Output = ();

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, func: FnDecl) -> Self::Output {
        let function_id = scope.next_function_id();
        let name = func.name().map(|name| internment::intern(&name.text()));

        let function = scope.decl_function(function_id, name);

        if let Some(params) = func.parameters() {
            for param in params.parameters() {
                function.argument(self.visit_pattern(param));
            }
        }

        if let Some(body) = func.body() {
            let mut scope: Box<dyn DatalogBuilder<'_>> = Box::new(function);

            for stmt in body.stmts() {
                // Enter a new scope after each statement that requires one
                if let Some(new_scope) = self.visit(&*scope, stmt) {
                    scope = Box::new(new_scope);
                }
            }
        }
    }
}

impl<'ddlog> Visit<'ddlog, VarDecl> for ScopeAnalyzer {
    type Output = DatalogScope<'ddlog>;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, var: VarDecl) -> Self::Output {
        let mut last_scope = None;
        for decl in var.declared() {
            let pattern = decl.pattern().map(|pat| self.visit_pattern(pat));
            let value = self.visit(scope, decl.value());

            last_scope = Some(if var.is_let() {
                scope.decl_let(pattern, value)
            } else if var.is_const() {
                scope.decl_const(pattern, value)
            } else if var.is_var() {
                scope.decl_var(pattern, value)
            } else {
                unreachable!("a variable declaration was neither `let`, `const` or `var`");
            });
        }

        last_scope.expect("at least one variable was declared, right?")
    }
}

impl<'ddlog> Visit<'ddlog, Expr> for ScopeAnalyzer {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, expr: Expr) -> Self::Output {
        match expr {
            Expr::Literal(literal) => self.visit(scope, literal),
            Expr::NameRef(name) => self.visit(scope, name),

            // FIXME: This is here so things can function before everything is 100%
            //        translatable into datalog, mostly for my sanity
            _ => 0,
        }
    }
}

impl<'ddlog> Visit<'ddlog, Literal> for ScopeAnalyzer {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, literal: Literal) -> Self::Output {
        match literal.kind() {
            LiteralKind::Number(number) => scope.number(number),
            LiteralKind::BigInt(bigint) => scope.bigint(bigint),
            LiteralKind::String => scope.string(literal.inner_string_text().unwrap().to_string()),
            LiteralKind::Null => scope.null(),
            LiteralKind::Bool(boolean) => scope.boolean(boolean),

            // FIXME: This is here so things can function before everything is 100%
            //        translatable into datalog, mostly for my sanity
            LiteralKind::Regex => 0,
        }
    }
}

impl<'ddlog> Visit<'ddlog, NameRef> for ScopeAnalyzer {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, name: NameRef) -> Self::Output {
        scope.name_ref(name.to_string())
    }
}
