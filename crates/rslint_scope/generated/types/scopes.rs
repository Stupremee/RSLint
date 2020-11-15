#![allow(
    path_statements,
    //unused_imports,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused_parens,
    non_shorthand_field_patterns,
    dead_code,
    overflowing_literals,
    unreachable_patterns,
    unused_variables,
    clippy::unknown_clippy_lints,
    clippy::missing_safety_doc,
    clippy::match_single_binding
)]

// Required for #[derive(Serialize, Deserialize)].
use ::serde::Deserialize;
use ::serde::Serialize;
use ::differential_datalog::record::FromRecord;
use ::differential_datalog::record::IntoRecord;
use ::differential_datalog::record::Mutator;

use crate::string_append_str;
use crate::string_append;
use crate::std_usize;
use crate::closure;

//
// use crate::ddlog_std;

#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct ChildScope {
    pub parent: crate::ast::ScopeId,
    pub child: crate::ast::ScopeId,
    pub file: crate::ast::FileId
}
impl abomonation::Abomonation for ChildScope{}
::differential_datalog::decl_struct_from_record!(ChildScope["scopes::ChildScope"]<>, ["scopes::ChildScope"][3]{[0]parent["parent"]: crate::ast::ScopeId, [1]child["child"]: crate::ast::ScopeId, [2]file["file"]: crate::ast::FileId});
::differential_datalog::decl_struct_into_record!(ChildScope, ["scopes::ChildScope"]<>, parent, child, file);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(ChildScope, <>, parent: crate::ast::ScopeId, child: crate::ast::ScopeId, file: crate::ast::FileId);
impl ::std::fmt::Display for ChildScope {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::scopes::ChildScope{parent,child,file} => {
                __formatter.write_str("scopes::ChildScope{")?;
                ::std::fmt::Debug::fmt(parent, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(child, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(file, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for ChildScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct FunctionLevelScope {
    pub scope: crate::ast::ScopeId,
    pub nearest: crate::ast::ScopeId,
    pub file: crate::ast::FileId,
    pub id: crate::ast::AnyId
}
impl abomonation::Abomonation for FunctionLevelScope{}
::differential_datalog::decl_struct_from_record!(FunctionLevelScope["scopes::FunctionLevelScope"]<>, ["scopes::FunctionLevelScope"][4]{[0]scope["scope"]: crate::ast::ScopeId, [1]nearest["nearest"]: crate::ast::ScopeId, [2]file["file"]: crate::ast::FileId, [3]id["id"]: crate::ast::AnyId});
::differential_datalog::decl_struct_into_record!(FunctionLevelScope, ["scopes::FunctionLevelScope"]<>, scope, nearest, file, id);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(FunctionLevelScope, <>, scope: crate::ast::ScopeId, nearest: crate::ast::ScopeId, file: crate::ast::FileId, id: crate::ast::AnyId);
impl ::std::fmt::Display for FunctionLevelScope {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::scopes::FunctionLevelScope{scope,nearest,file,id} => {
                __formatter.write_str("scopes::FunctionLevelScope{")?;
                ::std::fmt::Debug::fmt(scope, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(nearest, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(file, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(id, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for FunctionLevelScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}