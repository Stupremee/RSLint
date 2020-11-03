use crate::{datalog::DatalogBuilder, AnalyzerInner, Visit};
use rslint_core::rule_prelude::{
    ast::{
        ArrowExpr, ArrowExprParams, AwaitExpr, BinExpr, CondExpr, Expr, Literal, LiteralKind,
        NameRef, UnaryExpr, YieldExpr,
    },
    AstNode, SyntaxNodeExt,
};
use rslint_parser::ast::ExprOrBlock;
use types::{ddlog_std::Either, internment::Intern, ExprId, Pattern as DatalogPattern};

impl<'ddlog> Visit<'ddlog, Expr> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, expr: Expr) -> Self::Output {
        match expr {
            Expr::Literal(literal) => self.visit(scope, literal),
            Expr::NameRef(name) => self.visit(scope, name),
            Expr::ArrowExpr(arrow) => self.visit(scope, arrow),
            Expr::Template(_) => ExprId::new(u32::max_value()),
            Expr::ThisExpr(_) => ExprId::new(u32::max_value()),
            Expr::ArrayExpr(_) => ExprId::new(u32::max_value()),
            Expr::ObjectExpr(_) => ExprId::new(u32::max_value()),
            Expr::GroupingExpr(_) => ExprId::new(u32::max_value()),
            Expr::BracketExpr(_) => ExprId::new(u32::max_value()),
            Expr::DotExpr(_) => ExprId::new(u32::max_value()),
            Expr::NewExpr(_) => ExprId::new(u32::max_value()),
            Expr::CallExpr(_) => ExprId::new(u32::max_value()),
            Expr::UnaryExpr(unary) => self.visit(scope, unary),
            Expr::BinExpr(bin) => self.visit(scope, bin),
            Expr::CondExpr(cond) => self.visit(scope, cond),
            Expr::AssignExpr(_) => ExprId::new(u32::max_value()),
            Expr::SequenceExpr(_) => ExprId::new(u32::max_value()),
            Expr::FnExpr(_) => ExprId::new(u32::max_value()),
            Expr::ClassExpr(_) => ExprId::new(u32::max_value()),
            Expr::NewTarget(_) => ExprId::new(u32::max_value()),
            Expr::ImportMeta(_) => ExprId::new(u32::max_value()),
            Expr::SuperCall(_) => ExprId::new(u32::max_value()),
            Expr::ImportCall(_) => ExprId::new(u32::max_value()),
            Expr::YieldExpr(yield_expr) => self.visit(scope, yield_expr),
            Expr::AwaitExpr(await_expr) => self.visit(scope, await_expr),
        }
    }
}

impl<'ddlog> Visit<'ddlog, NameRef> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, name: NameRef) -> Self::Output {
        scope.name_ref(name.to_string(), name.syntax().trimmed_range())
    }
}

impl<'ddlog> Visit<'ddlog, Literal> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, literal: Literal) -> Self::Output {
        let span = literal.syntax().trimmed_range();

        match literal.kind() {
            LiteralKind::Number(number) => scope.number(number, span),
            LiteralKind::BigInt(bigint) => scope.bigint(bigint, span),
            LiteralKind::String => {
                scope.string(literal.inner_string_text().unwrap().to_string(), span)
            }
            LiteralKind::Null => scope.null(span),
            LiteralKind::Bool(boolean) => scope.boolean(boolean, span),
            LiteralKind::Regex => scope.regex(span),
        }
    }
}

impl<'ddlog> Visit<'ddlog, YieldExpr> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, yield_expr: YieldExpr) -> Self::Output {
        let expr = self.visit(scope, yield_expr.value());
        scope.yield_expr(expr, yield_expr.range())
    }
}

impl<'ddlog> Visit<'ddlog, AwaitExpr> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, await_expr: AwaitExpr) -> Self::Output {
        let expr = self.visit(scope, await_expr.expr());
        scope.await_expr(expr, await_expr.range())
    }
}

impl<'ddlog> Visit<'ddlog, ArrowExpr> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, arrow: ArrowExpr) -> Self::Output {
        let body = arrow.body().and_then(|body| {
            Some(match body {
                ExprOrBlock::Expr(expr) => Either::Left {
                    l: self.visit(scope, expr),
                },
                ExprOrBlock::Block(block) => Either::Right {
                    r: self.visit(scope, block)?,
                },
            })
        });
        let params = arrow
            .params()
            .map(|params| match params {
                ArrowExprParams::Name(name) => vec![Intern::new(DatalogPattern {
                    name: Intern::new(name.text()),
                })],

                ArrowExprParams::ParameterList(params) => params
                    .parameters()
                    .map(|pat| self.visit_pattern(pat))
                    .collect(),
            })
            .unwrap_or_default();

        scope.arrow(body, params, arrow.range())
    }
}

impl<'ddlog> Visit<'ddlog, UnaryExpr> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, unary: UnaryExpr) -> Self::Output {
        let op = unary.op().map(Into::into);
        let expr = self.visit(scope, unary.expr());

        scope.unary(op, expr, unary.range())
    }
}

impl<'ddlog> Visit<'ddlog, BinExpr> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, bin: BinExpr) -> Self::Output {
        let op = bin.op().map(Into::into);
        let lhs = self.visit(scope, bin.lhs());
        let rhs = self.visit(scope, bin.rhs());

        scope.bin(op, lhs, rhs, bin.range())
    }
}

impl<'ddlog> Visit<'ddlog, CondExpr> for AnalyzerInner {
    type Output = ExprId;

    fn visit(&self, scope: &dyn DatalogBuilder<'ddlog>, cond: CondExpr) -> Self::Output {
        let test = self.visit(scope, cond.test());
        let true_val = self.visit(scope, cond.cons());
        let false_val = self.visit(scope, cond.alt());

        scope.ternary(test, true_val, false_val, cond.range())
    }
}
