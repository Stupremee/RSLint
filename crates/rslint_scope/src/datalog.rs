use differential_datalog::{
    ddval::{DDValConvert, DDValue},
    int::Int,
    program::{RelId, Update},
    record::Record,
    DDlog, DeltaMap,
};
use rslint_core::rule_prelude::BigInt;
use rslint_scoping_ddlog::{api::HDDlog, relid2name, Relations, RELIDMAP};
use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    mem,
    sync::{Arc, Mutex, MutexGuard},
};
use types::{internment::Intern, *};

pub type DatalogResult<T> = Result<T, String>;

#[derive(Debug, Clone)]
pub struct Datalog {
    datalog: Arc<Mutex<DatalogInner>>,
}

impl Datalog {
    pub fn new() -> DatalogResult<Self> {
        let (hddlog, init_state) = HDDlog::run(2, false, |_: usize, _: &Record, _: isize| {})?;
        let mut this = Self {
            datalog: Arc::new(Mutex::new(DatalogInner {
                hddlog,
                updates: Vec::with_capacity(100),
                scope_id: 0,
                function_id: 0,
                statement_id: 0,
                expression_id: 0,
                accum: RELIDMAP
                    .iter()
                    .map(|(rel, _)| (*rel as RelId, BTreeSet::new()))
                    .collect(),
            })),
        };
        this.update(init_state);

        Ok(this)
    }

    pub fn transaction<F>(&mut self, transaction: F) -> DatalogResult<()>
    where
        F: for<'trans> FnOnce(&mut DatalogTransaction<'trans>) -> DatalogResult<()>,
    {
        let mut trans = DatalogTransaction::new(self.datalog.clone())?;
        transaction(&mut trans)?;
        let delta = trans.commit()?;
        self.update(delta);

        Ok(())
    }

    fn update(&mut self, delta: DeltaMap<DDValue>) {
        // let out_of_scope_vars = delta.clear_rel(Relations::OutOfScopeVar as RelId);
        // for (var, weight) in out_of_scope_vars {
        //     let var = unsafe { Value::OutOfScopeVar::from_ddvalue(var).0 };
        //
        //     match weight {
        //         1 => self.out_of_scope_vars.push(var),
        //         -1 => {
        //             let idx = self
        //                 .out_of_scope_vars
        //                 .iter()
        //                 .position(|v| v == &var)
        //                 .unwrap();
        //
        //             self.out_of_scope_vars.remove(idx);
        //         }
        //
        //         weight => unreachable!("invalid weight: {}", weight),
        //     }
        // }

        let mut datalog = self.datalog.lock().unwrap();
        for (rel, updates) in delta {
            let rel = datalog.accum.get_mut(&rel).unwrap();

            for (value, weight) in updates {
                match weight {
                    1 => {
                        rel.insert(value);
                    }
                    -1 => {
                        rel.remove(&value);
                    }

                    _ => unreachable!("invalid weight: {}", weight),
                }
            }
        }

        #[cfg(debug_assertions)]
        {
            println!("==   current db state    ==");
            for (rel, values) in datalog.accum.iter() {
                let name = relid2name(*rel).unwrap();

                if !values.is_empty() {
                    println!("Relation {}", name);

                    for val in values.iter() {
                        println!(">> {}", val);
                    }

                    println!();
                }
            }
            println!("== end current db state  ==\n\n");
        }
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub struct DatalogInner {
    hddlog: HDDlog,
    updates: Vec<Update<DDValue>>,
    scope_id: u32,
    function_id: u32,
    statement_id: u32,
    expression_id: u32,
    // Only for testing, keeps a record of every single ddlog-sided value
    accum: BTreeMap<RelId, BTreeSet<DDValue>>,
}

impl DatalogInner {
    fn inc_scope(&mut self) -> u32 {
        let temp = self.scope_id;
        self.scope_id += 1;
        temp
    }

    fn inc_function(&mut self) -> u32 {
        let temp = self.function_id;
        self.function_id += 1;
        temp
    }

    fn inc_statement(&mut self) -> u32 {
        let temp = self.statement_id;
        self.statement_id += 1;
        temp
    }

    fn inc_expression(&mut self) -> u32 {
        let temp = self.expression_id;
        self.expression_id += 1;
        temp
    }

    fn push_scope(&mut self, scope: InputScope) {
        self.updates.push(Update::Insert {
            relid: Relations::InputScope as RelId,
            v: scope.into_ddvalue(),
        });
    }

    fn insert<V>(&mut self, relation: RelId, val: V) -> &mut Self
    where
        V: DDValConvert,
    {
        self.updates.push(Update::Insert {
            relid: relation,
            v: val.into_ddvalue(),
        });

        self
    }
}

pub struct DatalogTransaction<'ddlog> {
    datalog: Arc<Mutex<DatalogInner>>,
    __lifetime: PhantomData<&'ddlog ()>,
}

impl<'ddlog> DatalogTransaction<'ddlog> {
    fn new(datalog: Arc<Mutex<DatalogInner>>) -> DatalogResult<Self> {
        datalog.lock().unwrap().hddlog.transaction_start()?;

        Ok(Self {
            datalog,
            __lifetime: PhantomData,
        })
    }

    pub fn scope(&self) -> DatalogScope<'_> {
        let mut datalog = self.datalog.lock().unwrap();

        let parent = datalog.scope_id;
        let scope_id = datalog.inc_scope();
        datalog.push_scope(InputScope {
            parent,
            child: scope_id,
        });

        DatalogScope {
            datalog: self.datalog.clone(),
            scope_id,
            __lifetime: PhantomData,
        }
    }

    pub fn commit(self) -> DatalogResult<DeltaMap<DDValue>> {
        let mut datalog = self.datalog.lock().unwrap();

        let updates = mem::take(&mut datalog.updates);
        datalog.hddlog.apply_valupdates(updates.into_iter())?;

        let delta = datalog.hddlog.transaction_commit_dump_changes()?;

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
    fn current_id(&self) -> u32;

    fn datalog(&self) -> &Arc<Mutex<DatalogInner>>;

    fn datalog_mut(&self) -> MutexGuard<'_, DatalogInner> {
        self.datalog().lock().unwrap()
    }

    fn scope(&self) -> DatalogScope<'ddlog> {
        let mut datalog = self.datalog_mut();
        let new_scope_id = datalog.inc_scope();
        datalog.push_scope(InputScope {
            parent: self.current_id(),
            child: new_scope_id,
        });

        DatalogScope {
            datalog: self.datalog().clone(),
            scope_id: new_scope_id,
            __lifetime: PhantomData,
        }
    }

    fn next_function_id(&self) -> u32 {
        self.datalog_mut().inc_function()
    }

    fn next_expr_id(&self) -> u32 {
        self.datalog_mut().inc_expression()
    }

    fn decl_function(&self, id: u32, name: Option<Intern<String>>) -> DatalogFunction<'ddlog> {
        self.datalog_mut().insert(
            Relations::Function as RelId,
            Function {
                id,
                name: name.into(),
                scope: self.current_id(),
            },
        );

        DatalogFunction {
            datalog: self.datalog().clone(),
            func_id: id,
            __lifetime: PhantomData,
        }
    }

    fn decl_let(
        &self,
        pattern: Option<Intern<Pattern>>,
        value: Option<ExprId>,
    ) -> DatalogScope<'ddlog> {
        let scope = self.scope();
        {
            let mut datalog = scope.datalog_mut();
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
                        scope: self.current_id(),
                    },
                );
        }

        scope
    }

    fn decl_const(
        &self,
        pattern: Option<Intern<Pattern>>,
        value: Option<ExprId>,
    ) -> DatalogScope<'ddlog> {
        let scope = self.scope();
        {
            let mut datalog = scope.datalog_mut();
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
                        scope: self.current_id(),
                    },
                );
        }

        scope
    }

    fn decl_var(
        &self,
        pattern: Option<Intern<Pattern>>,
        value: Option<ExprId>,
    ) -> DatalogScope<'ddlog> {
        let scope = self.scope();
        {
            let mut datalog = scope.datalog_mut();
            let stmt_id = datalog.inc_statement();

            datalog
                .insert(
                    Relations::VarDecl as RelId,
                    VarDecl {
                        stmt_id,
                        // TODO: Carry along the id of the closest var scoping
                        //       terminator through scopes/functions? This may
                        //       be trivial to do within ddlog itself
                        effective_scope: scope.current_id(),
                        pattern: pattern.into(),
                        value: value.into(),
                    },
                )
                .insert(
                    Relations::Statement as RelId,
                    Statement {
                        id: stmt_id,
                        kind: StmtKind::StmtVarDecl,
                        scope: self.current_id(),
                    },
                );
        }

        scope
    }

    fn number(&self, number: f64) -> ExprId {
        let mut datalog = self.datalog_mut();
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
                    scope: self.current_id(),
                },
            );

        id
    }

    fn bigint(&self, bigint: BigInt) -> ExprId {
        let mut datalog = self.datalog_mut();
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
                    scope: self.current_id(),
                },
            );

        id
    }

    fn string(&self, string: String) -> ExprId {
        let mut datalog = self.datalog_mut();
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
                    scope: self.current_id(),
                },
            );

        id
    }

    fn null(&self) -> ExprId {
        let mut datalog = self.datalog_mut();
        let id = datalog.inc_expression();

        datalog.insert(
            Relations::Expression as RelId,
            Expression {
                id,
                kind: ExprKind::ExprLit {
                    kind: LitKind::LitNull,
                },
                scope: self.current_id(),
            },
        );

        id
    }

    fn boolean(&self, boolean: bool) -> ExprId {
        let mut datalog = self.datalog_mut();
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
                    scope: self.current_id(),
                },
            );

        id
    }

    fn name_ref(&self, name: String) -> ExprId {
        let mut datalog = self.datalog_mut();
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
                    scope: self.current_id(),
                },
            );

        id
    }
}

#[derive(Clone)]
pub struct DatalogFunction<'ddlog> {
    datalog: Arc<Mutex<DatalogInner>>,
    func_id: u32,
    __lifetime: PhantomData<&'ddlog ()>,
}

impl<'ddlog> DatalogFunction<'ddlog> {
    pub fn func_id(&self) -> u32 {
        self.func_id
    }

    pub fn argument(&self, pattern: Intern<Pattern>) {
        self.datalog_mut().insert(
            Relations::FunctionArg as RelId,
            FunctionArg {
                parent_func: self.func_id(),
                pattern,
            },
        );
    }
}

impl<'ddlog> DatalogBuilder<'ddlog> for DatalogFunction<'ddlog> {
    fn datalog(&self) -> &Arc<Mutex<DatalogInner>> {
        &self.datalog
    }

    fn current_id(&self) -> u32 {
        self.func_id()
    }
}

#[derive(Clone)]
pub struct DatalogScope<'ddlog> {
    datalog: Arc<Mutex<DatalogInner>>,
    scope_id: u32,
    __lifetime: PhantomData<&'ddlog ()>,
}

impl<'ddlog> DatalogScope<'ddlog> {
    pub fn scope_id(&self) -> u32 {
        self.scope_id
    }
}

impl<'ddlog> DatalogBuilder<'ddlog> for DatalogScope<'ddlog> {
    fn datalog(&self) -> &Arc<Mutex<DatalogInner>> {
        &self.datalog
    }

    fn current_id(&self) -> u32 {
        self.scope_id()
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
