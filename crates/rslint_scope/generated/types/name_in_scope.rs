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
pub struct NameInScope {
    pub file: crate::ast::FileId,
    pub name: crate::ast::Name,
    pub scope: crate::ast::ScopeId,
    pub span: crate::ddlog_std::Option<crate::ast::Span>,
    pub declared_in: crate::ast::AnyId,
    pub implicit: bool
}
impl abomonation::Abomonation for NameInScope{}
::differential_datalog::decl_struct_from_record!(NameInScope["name_in_scope::NameInScope"]<>, ["name_in_scope::NameInScope"][6]{[0]file["file"]: crate::ast::FileId, [1]name["name"]: crate::ast::Name, [2]scope["scope"]: crate::ast::ScopeId, [3]span["span"]: crate::ddlog_std::Option<crate::ast::Span>, [4]declared_in["declared_in"]: crate::ast::AnyId, [5]implicit["implicit"]: bool});
::differential_datalog::decl_struct_into_record!(NameInScope, ["name_in_scope::NameInScope"]<>, file, name, scope, span, declared_in, implicit);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(NameInScope, <>, file: crate::ast::FileId, name: crate::ast::Name, scope: crate::ast::ScopeId, span: crate::ddlog_std::Option<crate::ast::Span>, declared_in: crate::ast::AnyId, implicit: bool);
impl ::std::fmt::Display for NameInScope {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::name_in_scope::NameInScope{file,name,scope,span,declared_in,implicit} => {
                __formatter.write_str("name_in_scope::NameInScope{")?;
                ::std::fmt::Debug::fmt(file, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(name, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(scope, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(span, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(declared_in, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(implicit, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for NameInScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}