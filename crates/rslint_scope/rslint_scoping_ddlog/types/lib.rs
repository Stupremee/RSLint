#![allow(
    path_statements,
    unused_imports,
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

//use ::serde::de::DeserializeOwned;
use ::differential_datalog::record::FromRecord;
use ::differential_datalog::record::IntoRecord;
use ::differential_datalog::record::Mutator;
use ::serde::Deserialize;
use ::serde::Serialize;

// `usize` and `isize` are builtin Rust types; we therefore declare an alias to DDlog's `usize` and
// `isize`.
pub type std_usize = u64;
pub type std_isize = i64;

mod ddlog_log;
pub use ddlog_log::*;

pub mod closure;

/* FlatBuffers code generated by `flatc` */
#[cfg(feature = "flatbuf")]
mod flatbuf_generated;

/* `FromFlatBuffer`, `ToFlatBuffer`, etc, trait declarations. */
#[cfg(feature = "flatbuf")]
pub mod flatbuf;

pub trait Val:
    Default
    + Eq
    + Ord
    + Clone
    + ::std::hash::Hash
    + PartialEq
    + PartialOrd
    + Serialize
    + ::serde::de::DeserializeOwned
    + 'static
{
}

impl<T> Val for T where
    T: Default
        + Eq
        + Ord
        + Clone
        + ::std::hash::Hash
        + PartialEq
        + PartialOrd
        + Serialize
        + ::serde::de::DeserializeOwned
        + 'static
{
}

pub fn string_append_str(mut s1: String, s2: &str) -> String {
    s1.push_str(s2);
    s1
}

#[allow(clippy::ptr_arg)]
pub fn string_append(mut s1: String, s2: &String) -> String {
    s1.push_str(s2.as_str());
    s1
}

#[macro_export]
macro_rules! deserialize_map_from_array {
    ( $modname:ident, $ktype:ty, $vtype:ty, $kfunc:path ) => {
        mod $modname {
            use super::*;
            use serde::de::{Deserialize, Deserializer};
            use serde::ser::Serializer;
            use std::collections::BTreeMap;

            pub fn serialize<S>(
                map: &crate::ddlog_std::Map<$ktype, $vtype>,
                serializer: S,
            ) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.collect_seq(map.x.values())
            }

            pub fn deserialize<'de, D>(
                deserializer: D,
            ) -> Result<crate::ddlog_std::Map<$ktype, $vtype>, D::Error>
            where
                D: Deserializer<'de>,
            {
                let v = Vec::<$vtype>::deserialize(deserializer)?;
                Ok(v.into_iter().map(|item| ($kfunc(&item), item)).collect())
            }
        }
    };
}


pub mod ddlog_std;
pub mod internment;
pub mod debug;
pub mod log;
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct ConstDecl {
    pub expr_id: crate::ExprId,
    pub scope: crate::Scope,
    pub pattern: crate::ddlog_std::Option<crate::IPattern>,
    pub value: crate::ddlog_std::Option<crate::ExprId>
}
impl abomonation::Abomonation for ConstDecl{}
::differential_datalog::decl_struct_from_record!(ConstDecl["ConstDecl"]<>, ["ConstDecl"][4]{[0]expr_id["expr_id"]: crate::ExprId, [1]scope["scope"]: crate::Scope, [2]pattern["pattern"]: crate::ddlog_std::Option<crate::IPattern>, [3]value["value"]: crate::ddlog_std::Option<crate::ExprId>});
::differential_datalog::decl_struct_into_record!(ConstDecl, ["ConstDecl"]<>, expr_id, scope, pattern, value);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(ConstDecl, <>, expr_id: crate::ExprId, scope: crate::Scope, pattern: crate::ddlog_std::Option<crate::IPattern>, value: crate::ddlog_std::Option<crate::ExprId>);
impl ::std::fmt::Display for ConstDecl {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::ConstDecl{expr_id,scope,pattern,value} => {
                __formatter.write_str("ConstDecl{")?;
                ::std::fmt::Debug::fmt(expr_id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(scope, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(pattern, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(value, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for ConstDecl {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct ExprBigInt {
    pub id: crate::ExprId,
    pub value: ::differential_datalog::int::Int
}
impl abomonation::Abomonation for ExprBigInt{}
::differential_datalog::decl_struct_from_record!(ExprBigInt["ExprBigInt"]<>, ["ExprBigInt"][2]{[0]id["id"]: crate::ExprId, [1]value["value"]: ::differential_datalog::int::Int});
::differential_datalog::decl_struct_into_record!(ExprBigInt, ["ExprBigInt"]<>, id, value);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(ExprBigInt, <>, id: crate::ExprId, value: ::differential_datalog::int::Int);
impl ::std::fmt::Display for ExprBigInt {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::ExprBigInt{id,value} => {
                __formatter.write_str("ExprBigInt{")?;
                ::std::fmt::Debug::fmt(id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(value, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for ExprBigInt {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct ExprBool {
    pub id: crate::ExprId,
    pub value: bool
}
impl abomonation::Abomonation for ExprBool{}
::differential_datalog::decl_struct_from_record!(ExprBool["ExprBool"]<>, ["ExprBool"][2]{[0]id["id"]: crate::ExprId, [1]value["value"]: bool});
::differential_datalog::decl_struct_into_record!(ExprBool, ["ExprBool"]<>, id, value);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(ExprBool, <>, id: crate::ExprId, value: bool);
impl ::std::fmt::Display for ExprBool {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::ExprBool{id,value} => {
                __formatter.write_str("ExprBool{")?;
                ::std::fmt::Debug::fmt(id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(value, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for ExprBool {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
pub type ExprId = u32;
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ExprKind {
    ExprLit {
        kind: crate::LitKind
    },
    NameRef
}
impl abomonation::Abomonation for ExprKind{}
::differential_datalog::decl_enum_from_record!(ExprKind["ExprKind"]<>, ExprLit["ExprLit"][1]{[0]kind["kind"]: crate::LitKind}, NameRef["NameRef"][0]{});
::differential_datalog::decl_enum_into_record!(ExprKind<>, ExprLit["ExprLit"]{kind}, NameRef["NameRef"]{});
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_enum!(ExprKind<>, ExprLit{kind: crate::LitKind}, NameRef{});
impl ::std::fmt::Display for ExprKind {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::ExprKind::ExprLit{kind} => {
                __formatter.write_str("ExprLit{")?;
                ::std::fmt::Debug::fmt(kind, __formatter)?;
                __formatter.write_str("}")
            },
            crate::ExprKind::NameRef{} => {
                __formatter.write_str("NameRef{")?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for ExprKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
impl ::std::default::Default for ExprKind {
    fn default() -> Self {
        crate::ExprKind::ExprLit{kind : ::std::default::Default::default()}
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct ExprNameRef {
    pub id: crate::ExprId,
    pub value: crate::Name
}
impl abomonation::Abomonation for ExprNameRef{}
::differential_datalog::decl_struct_from_record!(ExprNameRef["ExprNameRef"]<>, ["ExprNameRef"][2]{[0]id["id"]: crate::ExprId, [1]value["value"]: crate::Name});
::differential_datalog::decl_struct_into_record!(ExprNameRef, ["ExprNameRef"]<>, id, value);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(ExprNameRef, <>, id: crate::ExprId, value: crate::Name);
impl ::std::fmt::Display for ExprNameRef {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::ExprNameRef{id,value} => {
                __formatter.write_str("ExprNameRef{")?;
                ::std::fmt::Debug::fmt(id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(value, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for ExprNameRef {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct ExprNumber {
    pub id: crate::ExprId,
    pub value: ::ordered_float::OrderedFloat<f64>
}
impl abomonation::Abomonation for ExprNumber{}
::differential_datalog::decl_struct_from_record!(ExprNumber["ExprNumber"]<>, ["ExprNumber"][2]{[0]id["id"]: crate::ExprId, [1]value["value"]: ::ordered_float::OrderedFloat<f64>});
::differential_datalog::decl_struct_into_record!(ExprNumber, ["ExprNumber"]<>, id, value);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(ExprNumber, <>, id: crate::ExprId, value: ::ordered_float::OrderedFloat<f64>);
impl ::std::fmt::Display for ExprNumber {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::ExprNumber{id,value} => {
                __formatter.write_str("ExprNumber{")?;
                ::std::fmt::Debug::fmt(id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(value, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for ExprNumber {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct ExprString {
    pub id: crate::ExprId,
    pub value: crate::internment::istring
}
impl abomonation::Abomonation for ExprString{}
::differential_datalog::decl_struct_from_record!(ExprString["ExprString"]<>, ["ExprString"][2]{[0]id["id"]: crate::ExprId, [1]value["value"]: crate::internment::istring});
::differential_datalog::decl_struct_into_record!(ExprString, ["ExprString"]<>, id, value);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(ExprString, <>, id: crate::ExprId, value: crate::internment::istring);
impl ::std::fmt::Display for ExprString {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::ExprString{id,value} => {
                __formatter.write_str("ExprString{")?;
                ::std::fmt::Debug::fmt(id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(value, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for ExprString {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct Expression {
    pub id: crate::ExprId,
    pub kind: crate::ExprKind
}
impl abomonation::Abomonation for Expression{}
::differential_datalog::decl_struct_from_record!(Expression["Expression"]<>, ["Expression"][2]{[0]id["id"]: crate::ExprId, [1]kind["kind"]: crate::ExprKind});
::differential_datalog::decl_struct_into_record!(Expression, ["Expression"]<>, id, kind);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(Expression, <>, id: crate::ExprId, kind: crate::ExprKind);
impl ::std::fmt::Display for Expression {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::Expression{id,kind} => {
                __formatter.write_str("Expression{")?;
                ::std::fmt::Debug::fmt(id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(kind, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for Expression {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
pub type FuncId = u32;
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct Function {
    pub id: crate::FuncId,
    pub name: crate::ddlog_std::Option<crate::Name>,
    pub scope: crate::Scope
}
impl abomonation::Abomonation for Function{}
::differential_datalog::decl_struct_from_record!(Function["Function"]<>, ["Function"][3]{[0]id["id"]: crate::FuncId, [1]name["name"]: crate::ddlog_std::Option<crate::Name>, [2]scope["scope"]: crate::Scope});
::differential_datalog::decl_struct_into_record!(Function, ["Function"]<>, id, name, scope);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(Function, <>, id: crate::FuncId, name: crate::ddlog_std::Option<crate::Name>, scope: crate::Scope);
impl ::std::fmt::Display for Function {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::Function{id,name,scope} => {
                __formatter.write_str("Function{")?;
                ::std::fmt::Debug::fmt(id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(name, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(scope, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for Function {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct FunctionArg {
    pub parent_func: crate::FuncId,
    pub pattern: crate::IPattern
}
impl abomonation::Abomonation for FunctionArg{}
::differential_datalog::decl_struct_from_record!(FunctionArg["FunctionArg"]<>, ["FunctionArg"][2]{[0]parent_func["parent_func"]: crate::FuncId, [1]pattern["pattern"]: crate::IPattern});
::differential_datalog::decl_struct_into_record!(FunctionArg, ["FunctionArg"]<>, parent_func, pattern);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(FunctionArg, <>, parent_func: crate::FuncId, pattern: crate::IPattern);
impl ::std::fmt::Display for FunctionArg {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::FunctionArg{parent_func,pattern} => {
                __formatter.write_str("FunctionArg{")?;
                ::std::fmt::Debug::fmt(parent_func, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(pattern, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for FunctionArg {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
pub type IPattern = crate::internment::Intern<crate::Pattern>;
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct InputScope {
    pub parent: crate::Scope,
    pub child: crate::Scope
}
impl abomonation::Abomonation for InputScope{}
::differential_datalog::decl_struct_from_record!(InputScope["InputScope"]<>, ["InputScope"][2]{[0]parent["parent"]: crate::Scope, [1]child["child"]: crate::Scope});
::differential_datalog::decl_struct_into_record!(InputScope, ["InputScope"]<>, parent, child);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(InputScope, <>, parent: crate::Scope, child: crate::Scope);
impl ::std::fmt::Display for InputScope {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::InputScope{parent,child} => {
                __formatter.write_str("InputScope{")?;
                ::std::fmt::Debug::fmt(parent, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(child, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for InputScope {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct LetDecl {
    pub expr_id: crate::ExprId,
    pub scope: crate::Scope,
    pub pattern: crate::ddlog_std::Option<crate::IPattern>,
    pub value: crate::ddlog_std::Option<crate::ExprId>
}
impl abomonation::Abomonation for LetDecl{}
::differential_datalog::decl_struct_from_record!(LetDecl["LetDecl"]<>, ["LetDecl"][4]{[0]expr_id["expr_id"]: crate::ExprId, [1]scope["scope"]: crate::Scope, [2]pattern["pattern"]: crate::ddlog_std::Option<crate::IPattern>, [3]value["value"]: crate::ddlog_std::Option<crate::ExprId>});
::differential_datalog::decl_struct_into_record!(LetDecl, ["LetDecl"]<>, expr_id, scope, pattern, value);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(LetDecl, <>, expr_id: crate::ExprId, scope: crate::Scope, pattern: crate::ddlog_std::Option<crate::IPattern>, value: crate::ddlog_std::Option<crate::ExprId>);
impl ::std::fmt::Display for LetDecl {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::LetDecl{expr_id,scope,pattern,value} => {
                __formatter.write_str("LetDecl{")?;
                ::std::fmt::Debug::fmt(expr_id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(scope, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(pattern, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(value, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for LetDecl {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum LitKind {
    LitNumber,
    LitBigInt,
    LitString,
    LitNull,
    LitBool
}
impl abomonation::Abomonation for LitKind{}
::differential_datalog::decl_enum_from_record!(LitKind["LitKind"]<>, LitNumber["LitNumber"][0]{}, LitBigInt["LitBigInt"][0]{}, LitString["LitString"][0]{}, LitNull["LitNull"][0]{}, LitBool["LitBool"][0]{});
::differential_datalog::decl_enum_into_record!(LitKind<>, LitNumber["LitNumber"]{}, LitBigInt["LitBigInt"]{}, LitString["LitString"]{}, LitNull["LitNull"]{}, LitBool["LitBool"]{});
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_enum!(LitKind<>, LitNumber{}, LitBigInt{}, LitString{}, LitNull{}, LitBool{});
impl ::std::fmt::Display for LitKind {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::LitKind::LitNumber{} => {
                __formatter.write_str("LitNumber{")?;
                __formatter.write_str("}")
            },
            crate::LitKind::LitBigInt{} => {
                __formatter.write_str("LitBigInt{")?;
                __formatter.write_str("}")
            },
            crate::LitKind::LitString{} => {
                __formatter.write_str("LitString{")?;
                __formatter.write_str("}")
            },
            crate::LitKind::LitNull{} => {
                __formatter.write_str("LitNull{")?;
                __formatter.write_str("}")
            },
            crate::LitKind::LitBool{} => {
                __formatter.write_str("LitBool{")?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for LitKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
impl ::std::default::Default for LitKind {
    fn default() -> Self {
        crate::LitKind::LitNumber{}
    }
}
pub type Name = crate::internment::istring;
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct Pattern {
    pub name: crate::Name
}
impl abomonation::Abomonation for Pattern{}
::differential_datalog::decl_struct_from_record!(Pattern["Pattern"]<>, ["SinglePattern"][1]{[0]name["name"]: crate::Name});
::differential_datalog::decl_struct_into_record!(Pattern, ["Pattern"]<>, name);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(Pattern, <>, name: crate::Name);
impl ::std::fmt::Display for Pattern {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::Pattern{name} => {
                __formatter.write_str("SinglePattern{")?;
                ::std::fmt::Debug::fmt(name, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for Pattern {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
pub type Scope = u32;
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct VarDecl {
    pub expr_id: crate::ExprId,
    pub effective_scope: crate::Scope,
    pub decl_scope: crate::Scope,
    pub pattern: crate::ddlog_std::Option<crate::IPattern>,
    pub value: crate::ddlog_std::Option<crate::ExprId>
}
impl abomonation::Abomonation for VarDecl{}
::differential_datalog::decl_struct_from_record!(VarDecl["VarDecl"]<>, ["VarDecl"][5]{[0]expr_id["expr_id"]: crate::ExprId, [1]effective_scope["effective_scope"]: crate::Scope, [2]decl_scope["decl_scope"]: crate::Scope, [3]pattern["pattern"]: crate::ddlog_std::Option<crate::IPattern>, [4]value["value"]: crate::ddlog_std::Option<crate::ExprId>});
::differential_datalog::decl_struct_into_record!(VarDecl, ["VarDecl"]<>, expr_id, effective_scope, decl_scope, pattern, value);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(VarDecl, <>, expr_id: crate::ExprId, effective_scope: crate::Scope, decl_scope: crate::Scope, pattern: crate::ddlog_std::Option<crate::IPattern>, value: crate::ddlog_std::Option<crate::ExprId>);
impl ::std::fmt::Display for VarDecl {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::VarDecl{expr_id,effective_scope,decl_scope,pattern,value} => {
                __formatter.write_str("VarDecl{")?;
                ::std::fmt::Debug::fmt(expr_id, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(effective_scope, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(decl_scope, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(pattern, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(value, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for VarDecl {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
::differential_datalog::decl_ddval_convert!{crate::ConstDecl}
::differential_datalog::decl_ddval_convert!{crate::ExprBigInt}
::differential_datalog::decl_ddval_convert!{crate::ExprBool}
::differential_datalog::decl_ddval_convert!{crate::ExprNameRef}
::differential_datalog::decl_ddval_convert!{crate::ExprNumber}
::differential_datalog::decl_ddval_convert!{crate::ExprString}
::differential_datalog::decl_ddval_convert!{crate::Expression}
::differential_datalog::decl_ddval_convert!{crate::Function}
::differential_datalog::decl_ddval_convert!{crate::FunctionArg}
::differential_datalog::decl_ddval_convert!{crate::InputScope}
::differential_datalog::decl_ddval_convert!{crate::LetDecl}
::differential_datalog::decl_ddval_convert!{crate::VarDecl}