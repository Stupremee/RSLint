mod derived_facts;

pub use derived_facts::Outputs;

use crate::globals::JsGlobal;
use differential_datalog::{
    ddval::{DDValConvert, DDValue},
    int::Int,
    program::{IdxId, RelId, Update},
    record::Record,
    DDlog, DeltaMap,
};
use rslint_parser::{BigInt, TextRange};
use rslint_scoping_ddlog::{api::HDDlog, Indexes, Relations, INPUT_RELIDMAP};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    sync::Mutex,
};
use types::{
    ast::{
        ArrayElement, AssignOperand, BinOperand, ClassId, ExportKind, ExprId, ExprKind, FileId,
        FileKind, ForInit, FuncId, GlobalId, IClassElement, IPattern, ImportClause, ImportId,
        Increment, LitKind, Name, Pattern, PropertyKey, PropertyVal, ScopeId, Span, Spanned,
        StmtId, StmtKind, SwitchClause, TryHandler, UnaryOperand,
    },
    ddlog_std::Either,
    inputs::{
        Array, Arrow, ArrowParam, Assign, Await, BinOp, BracketAccess, Break, Call, Class,
        ClassExpr, ConstDecl, Continue, DoWhile, DotAccess, ExprBigInt, ExprBool, ExprNumber,
        ExprString, Expression, File as InputFile, FileExport, For, ForIn, Function, FunctionArg,
        If, ImplicitGlobal, ImportDecl, InlineFunc, InlineFuncParam, InputScope, Label, LetDecl,
        NameRef, New, Property, Return, Statement, Switch, SwitchCase, Template, Ternary, Throw,
        Try, UnaryOp, VarDecl, While, With, Yield,
    },
    internment::Intern,
};

// TODO: Work on the internment situation, I don't like
//       having to allocate strings for idents

pub type DatalogResult<T> = Result<T, String>;

#[derive(Debug, Clone, PartialEq)]
pub enum DatalogLint {
    NoUndef {
        var: Name,
        span: Span,
        file: FileId,
    },
    NoUnusedVars {
        var: Name,
        declared: Span,
        file: FileId,
    },
    TypeofUndef {
        whole_expr: Span,
        undefined_portion: Span,
        file: FileId,
    },
    UseBeforeDef {
        name: Name,
        used: Span,
        declared: Span,
        file: FileId,
    },
}

impl DatalogLint {
    pub fn is_no_undef(&self) -> bool {
        matches!(self, Self::NoUndef { .. })
    }

    pub fn is_no_unused_vars(&self) -> bool {
        matches!(self, Self::NoUnusedVars { .. })
    }

    pub fn is_typeof_undef(&self) -> bool {
        matches!(self, Self::TypeofUndef { .. })
    }

    pub fn is_use_before_def(&self) -> bool {
        matches!(self, Self::UseBeforeDef { .. })
    }

    #[cfg(test)]
    pub(crate) fn no_undef(var: impl Into<Name>, span: std::ops::Range<u32>) -> Self {
        Self::NoUndef {
            var: var.into(),
            span: span.into(),
            file: FileId::new(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn no_unused_vars(var: impl Into<Name>, declared: std::ops::Range<u32>) -> Self {
        Self::NoUnusedVars {
            var: var.into(),
            declared: declared.into(),
            file: FileId::new(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn typeof_undef(
        whole_expr: std::ops::Range<u32>,
        undefined_portion: std::ops::Range<u32>,
    ) -> Self {
        Self::TypeofUndef {
            whole_expr: whole_expr.into(),
            undefined_portion: undefined_portion.into(),
            file: FileId::new(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn use_before_def(
        name: impl Into<Name>,
        used: std::ops::Range<u32>,
        declared: std::ops::Range<u32>,
    ) -> Self {
        Self::UseBeforeDef {
            name: name.into(),
            used: used.into(),
            declared: declared.into(),
            file: FileId::new(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn file_id(&self) -> FileId {
        match *self {
            Self::NoUndef { file, .. } => file,
            Self::NoUnusedVars { file, .. } => file,
            Self::TypeofUndef { file, .. } => file,
            Self::UseBeforeDef { file, .. } => file,
        }
    }

    #[cfg(test)]
    pub(crate) fn file_id_mut(&mut self) -> &mut FileId {
        match self {
            Self::NoUndef { file, .. } => file,
            Self::NoUnusedVars { file, .. } => file,
            Self::TypeofUndef { file, .. } => file,
            Self::UseBeforeDef { file, .. } => file,
        }
    }
}

#[derive(Debug)]
pub struct Datalog {
    hddlog: HDDlog,
    transaction_lock: Mutex<()>,
    outputs: Outputs,
}

impl Datalog {
    pub fn new() -> DatalogResult<Self> {
        let (hddlog, _init_state) = HDDlog::run(2, false, |_: usize, _: &Record, _: isize| {})?;
        let this = Self {
            hddlog,
            transaction_lock: Mutex::new(()),
            outputs: Outputs::new(),
        };

        Ok(this)
    }

    pub fn outputs(&self) -> &Outputs {
        &self.outputs
    }

    pub fn reset(&self) -> DatalogResult<()> {
        self.transaction(|_trans| {
            for relation in INPUT_RELIDMAP.keys().copied() {
                self.hddlog.clear_relation(relation as RelId)?;
            }

            Ok(())
        })?;

        self.outputs.clear();

        Ok(())
    }

    // TODO: Make this take an iterator
    pub fn inject_globals(&self, file: FileId, globals: &[JsGlobal]) -> DatalogResult<()> {
        self.transaction(|trans| {
            for global in globals {
                trans.implicit_global(file, global);
            }

            Ok(())
        })
    }

    // FIXME: Make this only apply to a single file or remove it
    pub fn clear_globals(&self) -> DatalogResult<()> {
        let _transaction_guard = self.transaction_lock.lock().unwrap();

        self.hddlog.transaction_start()?;
        self.hddlog
            .clear_relation(Relations::inputs_ImplicitGlobal as RelId)?;

        self.hddlog.transaction_commit()
    }

    pub fn dump_inputs(&self) -> DatalogResult<String> {
        let mut inputs = Vec::new();
        self.hddlog.dump_input_snapshot(&mut inputs).unwrap();

        Ok(String::from_utf8(inputs).unwrap())
    }

    // Note: Ddlog only allows one concurrent transaction, so all calls to this function
    //       will block until the previous completes
    // TODO: We can actually add to the transaction batch concurrently, but transactions
    //       themselves have to be synchronized in some fashion (barrier?)
    pub fn transaction<T, F>(&self, transaction: F) -> DatalogResult<T>
    where
        F: for<'trans> FnOnce(&mut DatalogTransaction<'trans>) -> DatalogResult<T>,
    {
        let inner = DatalogInner::new(FileId::new(0));
        let mut trans = DatalogTransaction::new(&inner)?;
        let result = transaction(&mut trans)?;

        let delta = {
            let _transaction_guard = self.transaction_lock.lock().unwrap();

            self.hddlog.transaction_start()?;
            self.hddlog
                .apply_valupdates(inner.updates.borrow_mut().drain(..))?;

            self.hddlog.transaction_commit_dump_changes()?
        };
        self.outputs.batch_update(delta);

        Ok(result)
    }

    pub(crate) fn query(
        &self,
        index: Indexes,
        key: Option<DDValue>,
    ) -> DatalogResult<BTreeSet<DDValue>> {
        if let Some(key) = key {
            self.hddlog.query_index(index as IdxId, key)
        } else {
            self.hddlog.dump_index(index as IdxId)
        }
    }

    pub fn get_lints(&self) -> DatalogResult<Vec<DatalogLint>> {
        let mut lints = Vec::new();

        lints.extend(
            self.outputs()
                .no_undef
                .iter()
                .map(|usage| DatalogLint::NoUndef {
                    var: usage.key().name.clone(),
                    span: usage.key().span,
                    file: usage.key().file,
                }),
        );

        lints.extend(self.outputs().unused_variables.iter().map(|unused| {
            DatalogLint::NoUnusedVars {
                var: unused.key().name.clone(),
                declared: unused.key().span,
                file: unused.key().file,
            }
        }));

        lints.extend(self.outputs().typeof_undef.iter().map(|undef| {
            let whole_expr = self
                .query(
                    Indexes::inputs_ExpressionById,
                    Some(undef.key().whole_expr.into_ddvalue()),
                )
                .unwrap()
                .into_iter()
                .next()
                .map(|expr| unsafe { Expression::from_ddvalue(expr) })
                .unwrap();

            let undefined_portion = self
                .query(
                    Indexes::inputs_ExpressionById,
                    Some(undef.key().undefined_expr.into_ddvalue()),
                )
                .unwrap()
                .into_iter()
                .next()
                .map(|expr| unsafe { Expression::from_ddvalue(expr) })
                .unwrap()
                .span;

            DatalogLint::TypeofUndef {
                whole_expr: whole_expr.span,
                undefined_portion,
                file: whole_expr.scope.file,
            }
        }));

        lints.extend(
            self.outputs()
                .use_before_decl
                .iter()
                .map(|used| DatalogLint::UseBeforeDef {
                    name: used.key().name.clone(),
                    used: used.key().used_in,
                    declared: used.key().declared_in,
                    file: used.key().file,
                }),
        );

        Ok(lints)
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
    updates: RefCell<Vec<Update<DDValue>>>,
    scope_id: Cell<ScopeId>,
    global_id: Cell<GlobalId>,
    import_id: Cell<ImportId>,
    class_id: Cell<ClassId>,
    function_id: Cell<FuncId>,
    statement_id: Cell<StmtId>,
    expression_id: Cell<ExprId>,
}

impl DatalogInner {
    fn new(file: FileId) -> Self {
        Self {
            updates: RefCell::new(Vec::with_capacity(100)),
            scope_id: Cell::new(ScopeId::new(0, file)),
            global_id: Cell::new(GlobalId::new(0, file)),
            import_id: Cell::new(ImportId::new(0, file)),
            class_id: Cell::new(ClassId::new(0, file)),
            function_id: Cell::new(FuncId::new(0, file)),
            statement_id: Cell::new(StmtId::new(0, file)),
            expression_id: Cell::new(ExprId::new(0, file)),
        }
    }

    fn inc_scope(&self) -> ScopeId {
        self.scope_id.inc()
    }

    fn inc_global(&self) -> GlobalId {
        self.global_id.inc()
    }

    fn inc_import(&self) -> ImportId {
        self.import_id.inc()
    }

    fn inc_class(&self) -> ClassId {
        self.class_id.inc()
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

    fn file_id(&self) -> FileId {
        self.scope_id.get().file
    }

    fn insert<V>(&self, relation: Relations, val: V) -> &Self
    where
        V: DDValConvert,
    {
        self.updates.borrow_mut().push(Update::Insert {
            relid: relation as RelId,
            v: val.into_ddvalue(),
        });

        self
    }
}

pub struct DatalogTransaction<'ddlog> {
    datalog: &'ddlog DatalogInner,
}

impl<'ddlog> DatalogTransaction<'ddlog> {
    const fn new(datalog: &'ddlog DatalogInner) -> DatalogResult<Self> {
        Ok(Self { datalog })
    }

    pub fn file(&self, id: FileId, kind: FileKind) -> DatalogScope<'ddlog> {
        self.datalog.scope_id.set(ScopeId::new(0, id));
        self.datalog.global_id.set(GlobalId::new(0, id));
        self.datalog.import_id.set(ImportId::new(0, id));
        self.datalog.class_id.set(ClassId::new(0, id));
        self.datalog.function_id.set(FuncId::new(0, id));
        self.datalog.statement_id.set(StmtId::new(0, id));
        self.datalog.expression_id.set(ExprId::new(0, id));

        let scope_id = self.datalog.inc_scope();
        self.datalog
            .insert(
                Relations::inputs_File,
                InputFile {
                    id,
                    kind,
                    top_level_scope: scope_id,
                },
            )
            .insert(
                Relations::inputs_InputScope,
                InputScope {
                    parent: scope_id,
                    child: scope_id,
                },
            )
            .insert(Relations::inputs_EveryScope, scope_id);

        DatalogScope {
            datalog: self.datalog,
            scope_id,
        }
    }

    pub fn scope(&self) -> DatalogScope<'ddlog> {
        let scope_id = self.datalog.inc_scope();
        self.datalog
            .insert(
                Relations::inputs_InputScope,
                InputScope {
                    parent: scope_id,
                    child: scope_id,
                },
            )
            .insert(Relations::inputs_EveryScope, scope_id);

        DatalogScope {
            datalog: self.datalog,
            scope_id,
        }
    }

    // TODO: Fully integrate global info into ddlog
    fn implicit_global(&self, file: FileId, global: &JsGlobal) -> GlobalId {
        let id = self.datalog.inc_global();
        self.datalog.insert(
            Relations::inputs_ImplicitGlobal,
            ImplicitGlobal {
                id: GlobalId { id: id.id, file },
                name: Intern::new(global.name.to_string()),
            },
        );

        id
    }
}

pub trait DatalogBuilder<'ddlog> {
    fn scope_id(&self) -> ScopeId;

    fn datalog(&self) -> &'ddlog DatalogInner;

    fn scope(&self) -> DatalogScope<'ddlog> {
        let parent = self.scope_id();
        let child = self.datalog().inc_scope();
        debug_assert_ne!(parent, child);

        self.datalog()
            .insert(Relations::inputs_InputScope, InputScope { parent, child })
            .insert(Relations::inputs_EveryScope, child);

        DatalogScope {
            datalog: self.datalog(),
            scope_id: child,
        }
    }

    fn next_function_id(&self) -> FuncId {
        self.datalog().inc_function()
    }

    fn next_expr_id(&self) -> ExprId {
        self.datalog().inc_expression()
    }

    // TODO: Fully integrate global info into ddlog
    fn implicit_global(&self, file: FileId, global: &JsGlobal) -> GlobalId {
        let id = self.datalog().inc_global();
        self.datalog().insert(
            Relations::inputs_ImplicitGlobal,
            ImplicitGlobal {
                id: GlobalId { id: id.id, file },
                name: Intern::new(global.name.to_string()),
            },
        );

        id
    }

    fn decl_function(
        &self,
        id: FuncId,
        name: Option<Spanned<Name>>,
        exported: bool,
    ) -> (DatalogFunction<'ddlog>, DatalogScope<'ddlog>) {
        let body = self.scope();
        self.datalog().insert(
            Relations::inputs_Function,
            Function {
                id,
                name: name.into(),
                scope: self.scope_id(),
                body: body.scope_id(),
                exported,
            },
        );

        (
            DatalogFunction {
                datalog: self.datalog(),
                func_id: id,
                scope_id: body.scope_id(),
            },
            body,
        )
    }

    fn decl_let(
        &self,
        pattern: Option<Intern<Pattern>>,
        value: Option<ExprId>,
        span: TextRange,
        exported: bool,
    ) -> (StmtId, DatalogScope<'ddlog>) {
        let scope = self.scope();
        let stmt_id = {
            let datalog = scope.datalog();
            let stmt_id = datalog.inc_statement();

            datalog
                .insert(
                    Relations::inputs_LetDecl,
                    LetDecl {
                        stmt_id,
                        pattern: pattern.into(),
                        value: value.into(),
                        exported,
                    },
                )
                .insert(
                    Relations::inputs_Statement,
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
        exported: bool,
    ) -> (StmtId, DatalogScope<'ddlog>) {
        let scope = self.scope();
        let stmt_id = {
            let datalog = scope.datalog();
            let stmt_id = datalog.inc_statement();

            datalog
                .insert(
                    Relations::inputs_ConstDecl,
                    ConstDecl {
                        stmt_id,
                        pattern: pattern.into(),
                        value: value.into(),
                        exported,
                    },
                )
                .insert(
                    Relations::inputs_Statement,
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
        exported: bool,
    ) -> (StmtId, DatalogScope<'ddlog>) {
        let scope = self.scope();
        let stmt_id = {
            let datalog = scope.datalog();
            let stmt_id = datalog.inc_statement();

            datalog
                .insert(
                    Relations::inputs_VarDecl,
                    VarDecl {
                        stmt_id,
                        pattern: pattern.into(),
                        value: value.into(),
                        exported,
                    },
                )
                .insert(
                    Relations::inputs_Statement,
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

    fn ret(&self, value: Option<ExprId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::inputs_Return,
                Return {
                    stmt_id,
                    value: value.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
                Relations::inputs_If,
                If {
                    stmt_id,
                    cond: cond.into(),
                    if_body: if_body.into(),
                    else_body: else_body.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtIf,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn brk(&self, label: Option<Name>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::inputs_Break,
                Break {
                    stmt_id,
                    label: label.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
                Relations::inputs_DoWhile,
                DoWhile {
                    stmt_id,
                    body: body.into(),
                    cond: cond.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
                Relations::inputs_While,
                While {
                    stmt_id,
                    cond: cond.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
                Relations::inputs_For,
                For {
                    stmt_id,
                    init: init.into(),
                    test: test.into(),
                    update: update.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
                Relations::inputs_ForIn,
                ForIn {
                    stmt_id,
                    elem: elem.into(),
                    collection: collection.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtForIn,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn cont(&self, label: Option<Name>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::inputs_Continue,
                Continue {
                    stmt_id,
                    label: label.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
                Relations::inputs_With,
                With {
                    stmt_id,
                    cond: cond.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtWith,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        stmt_id
    }

    fn label(&self, name: Option<Spanned<Name>>, body: Option<StmtId>, span: TextRange) -> StmtId {
        let datalog = self.datalog();
        let stmt_id = datalog.inc_statement();

        datalog
            .insert(
                Relations::inputs_Label,
                Label {
                    stmt_id,
                    name: name.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
                Relations::inputs_Switch,
                Switch {
                    stmt_id,
                    test: test.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
                Statement {
                    id: stmt_id,
                    kind: StmtKind::StmtSwitch,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        for (case, body) in cases {
            datalog.insert(
                Relations::inputs_SwitchCase,
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
                Relations::inputs_Throw,
                Throw {
                    stmt_id,
                    exception: exception.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
                Relations::inputs_Try,
                Try {
                    stmt_id,
                    body: body.into(),
                    handler,
                    finalizer: finalizer.into(),
                },
            )
            .insert(
                Relations::inputs_Statement,
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
            Relations::inputs_Statement,
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
            Relations::inputs_Statement,
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
            Relations::inputs_Statement,
            Statement {
                id: stmt_id,
                kind: StmtKind::StmtEmpty,
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        stmt_id
    }

    fn number(&self, number: f64, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_ExprNumber,
                ExprNumber {
                    expr_id,
                    value: number.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprLit {
                        kind: LitKind::LitNumber,
                    },
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn bigint(&self, bigint: BigInt, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_ExprNumber,
                ExprBigInt {
                    expr_id,
                    value: Int::from_bigint(bigint),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprLit {
                        kind: LitKind::LitBigInt,
                    },
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn string(&self, value: Name, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(Relations::inputs_ExprString, ExprString { expr_id, value })
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprLit {
                        kind: LitKind::LitString,
                    },
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn null(&self, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprLit {
                    kind: LitKind::LitNull,
                },
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        expr_id
    }

    fn boolean(&self, boolean: bool, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_ExprBool,
                ExprBool {
                    expr_id,
                    value: boolean,
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprLit {
                        kind: LitKind::LitBool,
                    },
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    // TODO: Do we need to take in the regex literal?
    fn regex(&self, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprLit {
                    kind: LitKind::LitRegex,
                },
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        expr_id
    }

    fn name_ref(&self, value: Name, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(Relations::inputs_NameRef, NameRef { expr_id, value })
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprNameRef,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn yield_expr(&self, value: Option<ExprId>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_Yield,
                Yield {
                    expr_id,
                    value: value.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprYield,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn await_expr(&self, value: Option<ExprId>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_Await,
                Await {
                    expr_id,
                    value: value.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprAwait,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn arrow(
        &self,
        body: Option<Either<ExprId, StmtId>>,
        params: Vec<IPattern>,
        span: TextRange,
    ) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_Arrow,
                Arrow {
                    expr_id,
                    body: body.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprArrow,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        for param in params {
            datalog.insert(Relations::inputs_ArrowParam, ArrowParam { expr_id, param });
        }

        expr_id
    }

    fn unary(&self, op: Option<UnaryOperand>, expr: Option<ExprId>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_UnaryOp,
                UnaryOp {
                    expr_id,
                    op: op.into(),
                    expr: expr.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprUnaryOp,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn bin(
        &self,
        op: Option<BinOperand>,
        lhs: Option<ExprId>,
        rhs: Option<ExprId>,
        span: TextRange,
    ) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_BinOp,
                BinOp {
                    expr_id,
                    op: op.into(),
                    lhs: lhs.into(),
                    rhs: rhs.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprBinOp,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn ternary(
        &self,
        test: Option<ExprId>,
        true_val: Option<ExprId>,
        false_val: Option<ExprId>,
        span: TextRange,
    ) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_Ternary,
                Ternary {
                    expr_id,
                    test: test.into(),
                    true_val: true_val.into(),
                    false_val: false_val.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprTernary,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn this(&self, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprThis,
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        expr_id
    }

    fn template(&self, tag: Option<ExprId>, elements: Vec<ExprId>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_Template,
                Template {
                    expr_id,
                    tag: tag.into(),
                    elements: elements.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprTemplate,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn array(&self, elements: Vec<ArrayElement>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_Array,
                Array {
                    expr_id,
                    elements: elements.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprArray,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn object(
        &self,
        properties: Vec<(Option<PropertyKey>, PropertyVal)>,
        span: TextRange,
    ) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        for (key, val) in properties {
            datalog.insert(
                Relations::inputs_Property,
                Property {
                    expr_id,
                    key: key.into(),
                    val: Some(val).into(),
                },
            );
        }

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprObject,
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        expr_id
    }

    fn grouping(&self, inner: Option<ExprId>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprGrouping {
                    inner: inner.into(),
                },
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        expr_id
    }

    fn bracket(&self, object: Option<ExprId>, prop: Option<ExprId>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_BracketAccess,
                BracketAccess {
                    expr_id,
                    object: object.into(),
                    prop: prop.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprBracket,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn dot(&self, object: Option<ExprId>, prop: Option<Spanned<Name>>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_DotAccess,
                DotAccess {
                    expr_id,
                    object: object.into(),
                    prop: prop.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprDot,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn new(&self, object: Option<ExprId>, args: Option<Vec<ExprId>>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_New,
                New {
                    expr_id,
                    object: object.into(),
                    args: args.map(Into::into).into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprNew,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn call(&self, callee: Option<ExprId>, args: Option<Vec<ExprId>>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_Call,
                Call {
                    expr_id,
                    callee: callee.into(),
                    args: args.map(Into::into).into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprCall,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn assign(
        &self,
        lhs: Option<Either<IPattern, ExprId>>,
        rhs: Option<ExprId>,
        op: Option<AssignOperand>,
        span: TextRange,
    ) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_Assign,
                Assign {
                    expr_id,
                    lhs: lhs.into(),
                    rhs: rhs.into(),
                    op: op.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprAssign,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn sequence(&self, exprs: Vec<ExprId>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprSequence {
                    exprs: exprs.into(),
                },
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        expr_id
    }

    fn new_target(&self, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprNewTarget,
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        expr_id
    }

    fn import_meta(&self, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprImportMeta,
                scope: self.scope_id(),
                span: span.into(),
            },
        );

        expr_id
    }

    fn fn_expr(
        &self,
        name: Option<Spanned<Name>>,
        params: Vec<IPattern>,
        body: Option<StmtId>,
        span: TextRange,
    ) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_InlineFunc,
                InlineFunc {
                    expr_id,
                    name: name.into(),
                    body: body.into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprInlineFunc,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        for param in params {
            datalog.insert(
                Relations::inputs_InlineFuncParam,
                InlineFuncParam { expr_id, param },
            );
        }

        expr_id
    }

    fn super_call(&self, args: Option<Vec<ExprId>>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprSuperCall {
                    args: args.map(Into::into).into(),
                },
                scope: self.scope_id(),
                span: span.into(),
            },
        );
        expr_id
    }

    fn import_call(&self, arg: Option<ExprId>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog.insert(
            Relations::inputs_Expression,
            Expression {
                id: expr_id,
                kind: ExprKind::ExprImportCall { arg: arg.into() },
                scope: self.scope_id(),
                span: span.into(),
            },
        );
        expr_id
    }

    fn class_expr(&self, elements: Option<Vec<IClassElement>>, span: TextRange) -> ExprId {
        let datalog = self.datalog();
        let expr_id = datalog.inc_expression();

        datalog
            .insert(
                Relations::inputs_ClassExpr,
                ClassExpr {
                    expr_id,
                    elements: elements.map(Into::into).into(),
                },
            )
            .insert(
                Relations::inputs_Expression,
                Expression {
                    id: expr_id,
                    kind: ExprKind::ExprClass,
                    scope: self.scope_id(),
                    span: span.into(),
                },
            );

        expr_id
    }

    fn class_decl(
        &self,
        name: Option<Spanned<Name>>,
        parent: Option<ExprId>,
        elements: Option<Vec<IClassElement>>,
        exported: bool,
    ) -> (ClassId, DatalogScope<'ddlog>) {
        let scope = self.scope();
        let id = {
            let datalog = self.datalog();
            let id = datalog.inc_class();

            datalog.insert(
                Relations::inputs_Class,
                Class {
                    id,
                    name: name.into(),
                    parent: parent.into(),
                    elements: elements.map(Into::into).into(),
                    scope: self.scope_id(),
                    exported,
                },
            );

            id
        };

        (id, scope)
    }

    fn import_decl(&self, clauses: Vec<ImportClause>) {
        let datalog = self.datalog();
        let id = datalog.inc_import();

        for clause in clauses {
            datalog.insert(Relations::inputs_ImportDecl, ImportDecl { id, clause });
        }
    }

    fn export_named(&self, name: Option<Spanned<Name>>, alias: Option<Spanned<Name>>) {
        let datalog = self.datalog();

        datalog.insert(
            Relations::inputs_FileExport,
            FileExport {
                file: datalog.file_id(),
                export: ExportKind::NamedExport {
                    name: name.into(),
                    alias: alias.into(),
                },
                scope: self.scope_id(),
            },
        );
    }
}

#[derive(Clone)]
#[must_use]
pub struct DatalogFunction<'ddlog> {
    datalog: &'ddlog DatalogInner,
    func_id: FuncId,
    scope_id: ScopeId,
}

impl<'ddlog> DatalogFunction<'ddlog> {
    pub fn func_id(&self) -> FuncId {
        self.func_id
    }

    pub fn argument(&self, pattern: IPattern, implicit: bool) {
        self.datalog.insert(
            Relations::inputs_FunctionArg,
            FunctionArg {
                parent_func: self.func_id(),
                pattern,
                implicit,
            },
        );
    }
}

impl<'ddlog> DatalogBuilder<'ddlog> for DatalogFunction<'ddlog> {
    fn datalog(&self) -> &'ddlog DatalogInner {
        self.datalog
    }

    fn scope_id(&self) -> ScopeId {
        self.scope_id
    }
}

#[derive(Clone)]
#[must_use]
pub struct DatalogScope<'ddlog> {
    datalog: &'ddlog DatalogInner,
    scope_id: ScopeId,
}

impl<'ddlog> DatalogScope<'ddlog> {
    pub fn scope_id(&self) -> ScopeId {
        self.scope_id
    }
}

impl<'ddlog> DatalogBuilder<'ddlog> for DatalogScope<'ddlog> {
    fn datalog(&self) -> &'ddlog DatalogInner {
        self.datalog
    }

    fn scope_id(&self) -> ScopeId {
        self.scope_id
    }
}

#[cfg(debug_assertions)]
#[allow(dead_code)]
fn dump_delta(delta: &DeltaMap<DDValue>) {
    for (rel, changes) in delta.iter() {
        println!(
            "Changes to relation {}",
            rslint_scoping_ddlog::relid2name(*rel).unwrap()
        );

        for (val, weight) in changes.iter() {
            if *weight == 1 {
                println!(">> {} {:+}", val, weight);
            }
        }

        if !changes.is_empty() {
            println!();
        }
    }
}
