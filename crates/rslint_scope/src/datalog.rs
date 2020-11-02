use differential_datalog::{
    ddval::{DDValConvert, DDValue},
    int::Int,
    program::{RelId, Update},
    record::Record,
    DDlog, DeltaMap,
};
use rslint_parser::{BigInt, TextRange};
use rslint_scoping_ddlog::{api::HDDlog, relid2name, Relations};
use std::{
    cell::{Cell, RefCell},
    mem,
    sync::{Arc, Mutex},
};
use types::{internment::Intern, *};

// TODO: Work on the internment situation, I don't like
//       having to allocate strings for idents

pub type DatalogResult<T> = Result<T, String>;

#[derive(Debug, Clone)]
pub struct DerivedFacts {
    pub invalid_name_uses: Vec<InvalidNameUse>,
    pub var_use_before_decl: Vec<VarUseBeforeDeclaration>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Weight {
    Insert,
    Delete,
}

impl From<isize> for Weight {
    fn from(weight: isize) -> Self {
        match weight {
            1 => Self::Insert,
            -1 => Self::Delete,

            invalid => unreachable!("invalid weight given: {}", invalid),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Datalog {
    datalog: Arc<Mutex<DatalogInner>>,
}

impl Datalog {
    pub fn new() -> DatalogResult<Self> {
        let (hddlog, init_state) = HDDlog::run(2, false, |_: usize, _: &Record, _: isize| {})?;
        let this = Self {
            datalog: Arc::new(Mutex::new(DatalogInner::new(hddlog))),
        };
        this.update(init_state);

        Ok(this)
    }

    // Note: Ddlog only allows one concurrent transaction, so all calls to this function
    //       will block until the previous completes
    pub fn transaction<F>(&self, transaction: F) -> DatalogResult<DerivedFacts>
    where
        F: for<'trans> FnOnce(&mut DatalogTransaction<'trans>) -> DatalogResult<()>,
    {
        let delta = {
            let datalog = self
                .datalog
                .lock()
                .expect("failed to lock datalog for transaction");

            let mut trans = DatalogTransaction::new(&*datalog)?;
            transaction(&mut trans)?;
            trans.commit()?
        };

        Ok(self.update(delta))
    }

    fn update(&self, mut delta: DeltaMap<DDValue>) -> DerivedFacts {
        macro_rules! drain_relations {
            ($($relation:ident->$field:ident),* $(,)?) => {
                $(
                    let relation = delta.clear_rel(Relations::$relation as RelId);
                    let mut $field = Vec::with_capacity(relation.len());
                    for (usage, weight) in relation.into_iter() {
                        match Weight::from(weight) {
                            Weight::Insert => {
                                // Safety: This is the correct type since we pulled it from
                                //         the correct relation
                                $field.push(unsafe { $relation::from_ddvalue(usage) });
                            }
                            Weight::Delete => {}
                        }
                    }
                )*

                DerivedFacts { $( $field, )* }
            };
        }

        drain_relations! {
            InvalidNameUse->invalid_name_uses,
            VarUseBeforeDeclaration->var_use_before_decl,
        }
    }
}

impl Default for Datalog {
    fn default() -> Self {
        Self::new().expect("failed to create ddlog instance")
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub struct DatalogInner {
    hddlog: HDDlog,
    updates: RefCell<Vec<Update<DDValue>>>,
    scope_id: Cell<Scope>,
    function_id: Cell<FuncId>,
    statement_id: Cell<StmtId>,
    expression_id: Cell<ExprId>,
}

impl DatalogInner {
    fn new(hddlog: HDDlog) -> Self {
        Self {
            hddlog,
            updates: RefCell::new(Vec::with_capacity(100)),
            scope_id: Cell::new(Scope::new(0)),
            function_id: Cell::new(FuncId::new(0)),
            statement_id: Cell::new(StmtId::new(0)),
            expression_id: Cell::new(ExprId::new(0)),
        }
    }

    fn inc_scope(&self) -> Scope {
        self.scope_id.inc()
    }

    fn inc_function(&self) -> FuncId {
        self.function_id.inc()
    }

    fn inc_statement(&self) -> StmtId {
        self.statement_id.inc()
    }

    fn inc_expression(&self) -> ExprId {
        self.expression_id.inc()
    }

    fn push_scope(&self, scope: InputScope) -> &Self {
        self.insert(Relations::InputScope as RelId, scope)
    }

    fn insert<V>(&self, relation: RelId, val: V) -> &Self
    where
        V: DDValConvert,
    {
        self.updates.borrow_mut().push(Update::Insert {
            relid: relation,
            v: val.into_ddvalue(),
        });

        self
    }
}

pub struct DatalogTransaction<'ddlog> {
    datalog: &'ddlog DatalogInner,
}

impl<'ddlog> DatalogTransaction<'ddlog> {
    fn new(datalog: &'ddlog DatalogInner) -> DatalogResult<Self> {
        datalog.hddlog.transaction_start()?;

        Ok(Self { datalog })
    }

    pub fn scope(&self) -> DatalogScope<'_> {
        let parent = self.datalog.scope_id.get();
        let scope_id = self.datalog.inc_scope();
        self.datalog.push_scope(InputScope {
            parent,
            child: scope_id,
        });

        DatalogScope {
            datalog: self.datalog,
            scope_id,
        }
    }

    pub fn commit(self) -> DatalogResult<DeltaMap<DDValue>> {
        let updates = mem::take(&mut *self.datalog.updates.borrow_mut());
        self.datalog.hddlog.apply_valupdates(updates.into_iter())?;

        let delta = self.datalog.hddlog.transaction_commit_dump_changes()?;

        #[cfg(debug_assertions)]
        {
            println!("== start transaction ==");
            dump_delta(&delta);
            println!("==  end transaction  ==\n\n");
        }

        Ok(delta)
    }
}

pub trait DatalogBuilder<'ddlog> {
    fn scope_id(&self) -> Scope;

    fn datalog(&self) -> &'ddlog DatalogInner;

    fn scope(&self) -> DatalogScope<'ddlog> {
        let new_scope_id = self.datalog().inc_scope();
        self.datalog().push_scope(InputScope {
            parent: self.scope_id(),
            child: new_scope_id,
        });

        DatalogScope {
            datalog: self.datalog(),
            scope_id: new_scope_id,
        }
    }

    fn next_function_id(&self) -> FuncId {
        self.datalog().inc_function()
    }

    fn next_expr_id(&self) -> ExprId {
        self.datalog().inc_expression()
    }

    fn decl_function(&self, id: FuncId, name: Option<Intern<String>>) -> DatalogFunction<'ddlog> {
        self.datalog().insert(
            Relations::Function as RelId,
            Function {
                id,
                name: name.into(),
                scope: self.scope_id(),
            },
        );

        DatalogFunction {
            datalog: self.datalog(),
            func_id: id,
            scope_id: self.scope_id(),
        }
    }

    fn decl_let(
        &self,
        pattern: Option<Intern<Pattern>>,
        value: Option<ExprId>,
        span: TextRange,
    ) -> (StmtId, DatalogScope<'ddlog>) {
        let scope = self.scope();
        let stmt_id = {
            let datalog = scope.datalog();
            let stmt_id = datalog.inc_statement();

            datalog
                .insert(
                    Relations::LetDecl as RelId,
                    LetDecl {
                        stmt_id,
                        pattern: pattern.into(),
                        value: value.into(),
                    },
                )
                .insert(
                    Relations::Statement as RelId,
                    Statement {
                        id: stmt_id,
                        kind: StmtKind::StmtLetDecl,
                        scope: scope.scope_id(),
                        span: span.into(),
                    },
                );

            stmt_id
        };

        (stmt_id, scope)
    }

    fn decl_const(
        &self,
        pattern: Option<Intern<Pattern>>,
        value: Option<ExprId>,
        span: TextRange,
    ) -> (StmtId, DatalogScope<'ddlog>) {
        let scope = self.scope();
        let stmt_id = {
            let datalog = scope.datalog();
            let stmt_id = datalog.inc_statement();

            datalog
                .insert(
                    Relations::ConstDecl as RelId,
                    ConstDecl {
                        stmt_id,
                        pattern: pattern.into(),
                        value: value.into(),
                    },
                )
                .insert(
                    Relations::Statement as RelId,
                    Statement {
                        id: stmt_id,
                        kind: StmtKind::StmtConstDecl,
                        scope: scope.scope_id(),
                        span: span.into(),
                    },
                );

            stmt_id
        };

        (stmt_id, scope)
    }

    fn decl_var(
        &self,
        pattern: Option<Intern<Pattern>>,
        value: Option<ExprId>,
        span: TextRange,
    ) -> (StmtId, DatalogScope<'ddlog>) {
        let scope = self.scope();
        let stmt_id = {
            let datalog = scope.datalog();
            let stmt_id = datalog.inc_statement();

            datalog
                .insert(
                    Relations::VarDecl as RelId,
                    VarDecl {
                        stmt_id,
                        pattern: pattern.into(),
                        value: value.into(),
                    },
                )
                .insert(
                    Relations::Statement as RelId,
                    Statement {
                        id: stmt_id,
                        kind: StmtKind::StmtVarDecl,
                        scope: scope.scope_id(),
                        span: span.into(),
                    },
                );

            stmt_id
        };

        (stmt_id, scope)
    }

    fn number(&self, number: f64, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let id = datalog.inc_expression();

        datalog
            .insert(
                Relations::ExprNumber as RelId,
                ExprNumber {
                    id,
                    value: number.into(),
                },
            )
            .insert(
                Relations::Expression as RelId,
                Expression {
                    id,
                    kind: ExprKind::ExprLit {
                        kind: LitKind::LitNumber,
                    },
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        id
    }

    fn bigint(&self, bigint: BigInt, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let id = datalog.inc_expression();

        datalog
            .insert(
                Relations::ExprNumber as RelId,
                ExprBigInt {
                    id,
                    value: Int::from_bigint(bigint),
                },
            )
            .insert(
                Relations::Expression as RelId,
                Expression {
                    id,
                    kind: ExprKind::ExprLit {
                        kind: LitKind::LitBigInt,
                    },
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        id
    }

    fn string(&self, string: String, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let id = datalog.inc_expression();

        datalog
            .insert(
                Relations::ExprString as RelId,
                ExprString {
                    id,
                    value: internment::intern(&string),
                },
            )
            .insert(
                Relations::Expression as RelId,
                Expression {
                    id,
                    kind: ExprKind::ExprLit {
                        kind: LitKind::LitString,
                    },
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        id
    }

    fn null(&self, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let id = datalog.inc_expression();

        datalog.insert(
            Relations::Expression as RelId,
            Expression {
                id,
                kind: ExprKind::ExprLit {
                    kind: LitKind::LitNull,
                },
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        id
    }

    fn boolean(&self, boolean: bool, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let id = datalog.inc_expression();

        datalog
            .insert(
                Relations::ExprBool as RelId,
                ExprBool { id, value: boolean },
            )
            .insert(
                Relations::Expression as RelId,
                Expression {
                    id,
                    kind: ExprKind::ExprLit {
                        kind: LitKind::LitBool,
                    },
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        id
    }

    // TODO: Do we need to take in the regex literal?
    fn regex(&self, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let id = datalog.inc_expression();

        datalog.insert(
            Relations::Expression as RelId,
            Expression {
                id,
                kind: ExprKind::ExprLit {
                    kind: LitKind::LitRegex,
                },
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        id
    }

    fn name_ref(&self, name: String, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let id = datalog.inc_expression();

        datalog
            .insert(
                Relations::ExprNameRef as RelId,
                ExprNameRef {
                    id,
                    value: internment::intern(&name),
                },
            )
            .insert(
                Relations::Expression as RelId,
                Expression {
                    id,
                    kind: ExprKind::NameRef,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        id
    }

    fn ret(&self, value: Option<ExprId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::Return as RelId,
                Return {
                    stmt_id,
                    value: value.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtReturn,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn if_stmt(
        &self,
        cond: Option<ExprId>,
        if_body: Option<StmtId>,
        else_body: Option<StmtId>,
        span: TextRange,
    ) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::If as RelId,
                If {
                    stmt_id,
                    cond: cond.into(),
                    if_body: if_body.into(),
                    else_body: else_body.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtIf,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn brk(&self, label: Option<String>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::Break as RelId,
                Break {
                    stmt_id,
                    label: label.as_ref().map(internment::intern).into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtBreak,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn do_while(&self, body: Option<StmtId>, cond: Option<ExprId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::DoWhile as RelId,
                DoWhile {
                    stmt_id,
                    body: body.into(),
                    cond: cond.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtDoWhile,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn while_stmt(&self, cond: Option<ExprId>, body: Option<StmtId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::While as RelId,
                DoWhile {
                    stmt_id,
                    cond: cond.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtWhile,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn for_stmt(
        &self,
        init: Option<ForInit>,
        test: Option<ExprId>,
        update: Option<ExprId>,
        body: Option<StmtId>,
        span: TextRange,
    ) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::For as RelId,
                For {
                    stmt_id,
                    init: init.into(),
                    test: test.into(),
                    update: update.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtFor,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn for_in(
        &self,
        elem: Option<ForInit>,
        collection: Option<ExprId>,
        body: Option<StmtId>,
        span: TextRange,
    ) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::ForIn as RelId,
                ForIn {
                    stmt_id,
                    elem: elem.into(),
                    collection: collection.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtForIn,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn cont(&self, label: Option<String>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::Continue as RelId,
                Continue {
                    stmt_id,
                    label: label.as_ref().map(internment::intern).into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtContinue,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn with(&self, cond: Option<ExprId>, body: Option<StmtId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::With as RelId,
                With {
                    stmt_id,
                    cond: cond.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtWith,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn label(&self, name: Option<String>, body: Option<StmtId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::Label as RelId,
                Label {
                    stmt_id,
                    name: name.as_ref().map(internment::intern).into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtLabel,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn switch(
        &self,
        test: Option<ExprId>,
        cases: Vec<(SwitchClause, Option<StmtId>)>,
        span: TextRange,
    ) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::Switch as RelId,
                Switch {
                    stmt_id,
                    test: test.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtSwitch,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        for (case, body) in cases {
            datalog.insert(
                Relations::SwitchCase as RelId,
                SwitchCase {
                    stmt_id,
                    case,
                    body: body.into(),
                },
            );
        }

        stmt_id
    }

    fn throw(&self, exception: Option<ExprId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::Throw as RelId,
                Throw {
                    stmt_id,
                    exception: exception.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtThrow,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn try_stmt(
        &self,
        body: Option<StmtId>,
        handler: TryHandler,
        finalizer: Option<StmtId>,
        span: TextRange,
    ) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::Try as RelId,
                Try {
                    stmt_id,
                    body: body.into(),
                    handler,
                    finalizer: finalizer.into(),
                },
            )
            .insert(
                Relations::Statement as RelId,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtTry,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn debugger(&self, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog.insert(
            Relations::Statement as RelId,
            Statement {
                id: stmt_id,
                kind: StmtKind::StmtDebugger,
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        stmt_id
    }

    fn stmt_expr(&self, expr: Option<ExprId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog.insert(
            Relations::Statement as RelId,
            Statement {
                id: stmt_id,
                kind: StmtKind::StmtExpr {
                    expr_id: expr.into(),
                },
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        stmt_id
    }

    fn empty(&self, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog.insert(
            Relations::Statement as RelId,
            Statement {
                id: stmt_id,
                kind: StmtKind::StmtEmpty,
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        stmt_id
    }
}

#[derive(Clone)]
pub struct DatalogFunction<'ddlog> {
    datalog: &'ddlog DatalogInner,
    func_id: FuncId,
    scope_id: Scope,
}

impl<'ddlog> DatalogFunction<'ddlog> {
    pub fn func_id(&self) -> FuncId {
        self.func_id
    }

    pub fn argument(&self, pattern: Intern<Pattern>) {
        self.datalog.insert(
            Relations::FunctionArg as RelId,
            FunctionArg {
                parent_func: self.func_id(),
                pattern,
            },
        );
    }
}

impl<'ddlog> DatalogBuilder<'ddlog> for DatalogFunction<'ddlog> {
    fn datalog(&self) -> &'ddlog DatalogInner {
        self.datalog
    }

    fn scope_id(&self) -> Scope {
        self.scope_id
    }
}

#[derive(Clone)]
pub struct DatalogScope<'ddlog> {
    datalog: &'ddlog DatalogInner,
    scope_id: Scope,
}

impl<'ddlog> DatalogScope<'ddlog> {
    pub fn scope_id(&self) -> Scope {
        self.scope_id
    }
}

impl<'ddlog> DatalogBuilder<'ddlog> for DatalogScope<'ddlog> {
    fn datalog(&self) -> &'ddlog DatalogInner {
        self.datalog
    }

    fn scope_id(&self) -> Scope {
        self.scope_id
    }
}

#[cfg(debug_assertions)]
fn dump_delta(delta: &DeltaMap<DDValue>) {
    for (rel, changes) in delta.iter() {
        println!("Changes to relation {}", relid2name(*rel).unwrap());

        for (val, weight) in changes.iter() {
            println!(">> {} {:+}", val, weight);
        }

        if !changes.is_empty() {
            println!();
        }
    }
}
