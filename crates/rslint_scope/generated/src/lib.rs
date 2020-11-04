#![allow(
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
    clippy::toplevel_ref_arg
)]

use num::bigint::BigInt;
use std::convert::TryFrom;
use std::hash::Hash;
use std::ops::Deref;
use std::ptr;
use std::result;
use std::sync;

use ordered_float::*;

use differential_dataflow::collection;
use timely::communication;
use timely::dataflow::scopes;
use timely::worker;

use differential_datalog::ddval::*;
use differential_datalog::int::*;
use differential_datalog::program::*;
use differential_datalog::record;
use differential_datalog::record::FromRecord;
use differential_datalog::record::IntoRecord;
use differential_datalog::record::RelIdentifier;
use differential_datalog::record::UpdCmd;
use differential_datalog::uint::*;
use differential_datalog::DDlogConvert;
use num_traits::cast::FromPrimitive;
use num_traits::identities::One;
use once_cell::sync::Lazy;

use fnv::FnvHashMap;

pub mod api;
pub mod ovsdb_api;
pub mod update_handler;

use crate::api::updcmd2upd;
use ::types::closure;
use ::types::string_append;
use ::types::string_append_str;

use serde::ser::SerializeTuple;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

/// A default implementation of `DDlogConvert` that just forwards calls
/// to generated functions of equal name.
#[derive(Debug)]
pub struct DDlogConverter {}

impl DDlogConvert for DDlogConverter {
    fn relid2name(relId: RelId) -> Option<&'static str> {
        relid2name(relId)
    }

    fn indexid2name(idxId: IdxId) -> Option<&'static str> {
        indexid2name(idxId)
    }

    fn updcmd2upd(upd_cmd: &UpdCmd) -> ::std::result::Result<Update<DDValue>, String> {
        updcmd2upd(upd_cmd)
    }
}

/* Wrapper around `Update<DDValue>` type that implements `Serialize` and `Deserialize`
 * traits.  It is currently only used by the distributed_ddlog crate in order to
 * serialize updates before sending them over the network and deserializing them on the
 * way back.  In other scenarios, the user either creates a `Update<DDValue>` type
 * themselves (when using the strongly typed DDlog API) or deserializes `Update<DDValue>`
 * from `Record` using `DDlogConvert::updcmd2upd()`.
 *
 * Why use a wrapper instead of implementing the traits for `Update<DDValue>` directly?
 * `Update<>` and `DDValue` types are both declared in the `differential_datalog` crate,
 * whereas the `Deserialize` implementation is program-specific and must be in one of the
 * generated crates, so we need a wrapper to avoid creating an orphan `impl`.
 *
 * Serialized representation: we currently only serialize `Insert` and `DeleteValue`
 * commands, represented in serialized form as (polarity, relid, value) tuple.  This way
 * the deserializer first reads relid and uses it to decide which value to deserialize
 * next.
 *
 * `impl Serialize` - serializes the value by forwarding `serialize` call to the `DDValue`
 * object (in fact, it is generic and could be in the `differential_datalog` crate, but we
 * keep it here to make it easier to keep it in sync with `Deserialize`).
 *
 * `impl Deserialize` - gets generated in `Compile.hs` using the macro below.  The macro
 * takes a list of `(relid, type)` and generates a match statement that uses type-specific
 * `Deserialize` for each `relid`.
 */
#[derive(Debug)]
pub struct UpdateSerializer(Update<DDValue>);

impl From<Update<DDValue>> for UpdateSerializer {
    fn from(u: Update<DDValue>) -> Self {
        UpdateSerializer(u)
    }
}
impl From<UpdateSerializer> for Update<DDValue> {
    fn from(u: UpdateSerializer) -> Self {
        u.0
    }
}

impl Serialize for UpdateSerializer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tup = serializer.serialize_tuple(3)?;
        match &self.0 {
            Update::Insert { relid, v } => {
                tup.serialize_element(&true)?;
                tup.serialize_element(relid)?;
                tup.serialize_element(v)?;
            }
            Update::DeleteValue { relid, v } => {
                tup.serialize_element(&false)?;
                tup.serialize_element(relid)?;
                tup.serialize_element(v)?;
            }
            _ => panic!("Cannot serialize InsertOrUpdate/Modify/DeleteKey update"),
        };
        tup.end()
    }
}

#[macro_export]
macro_rules! decl_update_deserializer {
    ( $n:ty, $(($rel:expr, $typ:ty)),* ) => {
        impl<'de> ::serde::Deserialize<'de> for $n {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {

                struct UpdateVisitor;

                impl<'de> ::serde::de::Visitor<'de> for UpdateVisitor {
                    type Value = $n;

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                        formatter.write_str("(polarity, relid, value) tuple")
                    }

                    fn visit_seq<A>(self, mut seq: A) -> ::std::result::Result<Self::Value, A::Error>
                    where A: ::serde::de::SeqAccess<'de> {
                        let polarity = seq.next_element::<bool>()?.ok_or_else(|| <A::Error as ::serde::de::Error>::custom("Missing polarity"))?;
                        let relid = seq.next_element::<RelId>()?.ok_or_else(|| <A::Error as ::serde::de::Error>::custom("Missing relation id"))?;
                        match relid {
                            $(
                                $rel => {
                                    let v = seq.next_element::<$typ>()?.ok_or_else(|| <A::Error as ::serde::de::Error>::custom("Missing value"))?.into_ddvalue();
                                    if polarity {
                                        Ok(UpdateSerializer(Update::Insert{relid, v}))
                                    } else {
                                        Ok(UpdateSerializer(Update::DeleteValue{relid, v}))
                                    }
                                },
                            )*
                            _ => {
                                ::std::result::Result::Err(<A::Error as ::serde::de::Error>::custom(format!("Unknown input relation id {}", relid)))
                            }
                        }
                    }
                }

                deserializer.deserialize_tuple(3, UpdateVisitor)
            }
        }
    };
}

/* FlatBuffers bindings generated by `ddlog` */
#[cfg(feature = "flatbuf")]
pub mod flatbuf;

impl TryFrom<&RelIdentifier> for Relations {
    type Error = ();

    fn try_from(rel_id: &RelIdentifier) -> ::std::result::Result<Self, ()> {
        match rel_id {
            RelIdentifier::RelName(rname) => Relations::try_from(rname.as_ref()),
            RelIdentifier::RelId(id) => Relations::try_from(*id),
        }
    }
}


decl_update_deserializer!(UpdateSerializer,(0, ::types::Array), (1, ::types::Arrow), (2, ::types::ArrowParam), (3, ::types::Await), (4, ::types::BinOp), (5, ::types::BracketAccess), (6, ::types::Break), (7, ::types::ChildScope), (8, ::types::ClosestFunction), (9, ::types::ConstDecl), (10, ::types::Continue), (11, ::types::DoWhile), (12, ::types::DotAccess), (13, ::types::EveryScope), (14, ::types::ExprBigInt), (15, ::types::ExprBool), (16, ::types::ExprNumber), (17, ::types::ExprString), (18, ::types::Expression), (19, ::types::For), (20, ::types::ForIn), (21, ::types::Function), (22, ::types::FunctionArg), (23, ::types::Array), (24, ::types::Arrow), (25, ::types::ArrowParam), (26, ::types::Await), (27, ::types::BinOp), (28, ::types::BracketAccess), (29, ::types::Break), (30, ::types::ConstDecl), (31, ::types::Continue), (32, ::types::DoWhile), (33, ::types::DotAccess), (34, ::types::EveryScope), (35, ::types::ExprBigInt), (36, ::types::ExprBool), (37, ::types::ExprNumber), (38, ::types::ExprString), (39, ::types::Expression), (40, ::types::For), (41, ::types::ForIn), (42, ::types::Function), (43, ::types::FunctionArg), (44, ::types::If), (45, ::types::ImplicitGlobal), (46, ::types::InputScope), (47, ::types::Label), (48, ::types::LetDecl), (49, ::types::NameRef), (50, ::types::Property), (51, ::types::Return), (52, ::types::Statement), (53, ::types::Switch), (54, ::types::SwitchCase), (55, ::types::Template), (56, ::types::Ternary), (57, ::types::Throw), (58, ::types::Try), (59, ::types::UnaryOp), (60, ::types::VarDecl), (61, ::types::While), (62, ::types::With), (63, ::types::Yield), (64, ::types::If), (65, ::types::ImplicitGlobal), (66, ::types::InputScope), (67, ::types::InvalidNameUse), (68, ::types::Label), (69, ::types::LetDecl), (70, ::types::NameInScope), (71, ::types::NameRef), (72, ::types::Property), (73, ::types::Return), (74, ::types::Statement), (75, ::types::Switch), (76, ::types::SwitchCase), (77, ::types::Template), (78, ::types::Ternary), (79, ::types::Throw), (80, ::types::Try), (81, ::types::UnaryOp), (82, ::types::VarDecl), (83, ::types::VarUseBeforeDeclaration), (84, ::types::While), (85, ::types::With), (86, ::types::Yield));
impl TryFrom<&str> for Relations {
    type Error = ();
    fn try_from(rname: &str) -> ::std::result::Result<Self, ()> {
         match rname {
        "Array" => Ok(Relations::Array),
        "Arrow" => Ok(Relations::Arrow),
        "ArrowParam" => Ok(Relations::ArrowParam),
        "Await" => Ok(Relations::Await),
        "BinOp" => Ok(Relations::BinOp),
        "BracketAccess" => Ok(Relations::BracketAccess),
        "Break" => Ok(Relations::Break),
        "ChildScope" => Ok(Relations::ChildScope),
        "ClosestFunction" => Ok(Relations::ClosestFunction),
        "ConstDecl" => Ok(Relations::ConstDecl),
        "Continue" => Ok(Relations::Continue),
        "DoWhile" => Ok(Relations::DoWhile),
        "DotAccess" => Ok(Relations::DotAccess),
        "EveryScope" => Ok(Relations::EveryScope),
        "ExprBigInt" => Ok(Relations::ExprBigInt),
        "ExprBool" => Ok(Relations::ExprBool),
        "ExprNumber" => Ok(Relations::ExprNumber),
        "ExprString" => Ok(Relations::ExprString),
        "Expression" => Ok(Relations::Expression),
        "For" => Ok(Relations::For),
        "ForIn" => Ok(Relations::ForIn),
        "Function" => Ok(Relations::Function),
        "FunctionArg" => Ok(Relations::FunctionArg),
        "INPUT_Array" => Ok(Relations::INPUT_Array),
        "INPUT_Arrow" => Ok(Relations::INPUT_Arrow),
        "INPUT_ArrowParam" => Ok(Relations::INPUT_ArrowParam),
        "INPUT_Await" => Ok(Relations::INPUT_Await),
        "INPUT_BinOp" => Ok(Relations::INPUT_BinOp),
        "INPUT_BracketAccess" => Ok(Relations::INPUT_BracketAccess),
        "INPUT_Break" => Ok(Relations::INPUT_Break),
        "INPUT_ConstDecl" => Ok(Relations::INPUT_ConstDecl),
        "INPUT_Continue" => Ok(Relations::INPUT_Continue),
        "INPUT_DoWhile" => Ok(Relations::INPUT_DoWhile),
        "INPUT_DotAccess" => Ok(Relations::INPUT_DotAccess),
        "INPUT_EveryScope" => Ok(Relations::INPUT_EveryScope),
        "INPUT_ExprBigInt" => Ok(Relations::INPUT_ExprBigInt),
        "INPUT_ExprBool" => Ok(Relations::INPUT_ExprBool),
        "INPUT_ExprNumber" => Ok(Relations::INPUT_ExprNumber),
        "INPUT_ExprString" => Ok(Relations::INPUT_ExprString),
        "INPUT_Expression" => Ok(Relations::INPUT_Expression),
        "INPUT_For" => Ok(Relations::INPUT_For),
        "INPUT_ForIn" => Ok(Relations::INPUT_ForIn),
        "INPUT_Function" => Ok(Relations::INPUT_Function),
        "INPUT_FunctionArg" => Ok(Relations::INPUT_FunctionArg),
        "INPUT_If" => Ok(Relations::INPUT_If),
        "INPUT_ImplicitGlobal" => Ok(Relations::INPUT_ImplicitGlobal),
        "INPUT_InputScope" => Ok(Relations::INPUT_InputScope),
        "INPUT_Label" => Ok(Relations::INPUT_Label),
        "INPUT_LetDecl" => Ok(Relations::INPUT_LetDecl),
        "INPUT_NameRef" => Ok(Relations::INPUT_NameRef),
        "INPUT_Property" => Ok(Relations::INPUT_Property),
        "INPUT_Return" => Ok(Relations::INPUT_Return),
        "INPUT_Statement" => Ok(Relations::INPUT_Statement),
        "INPUT_Switch" => Ok(Relations::INPUT_Switch),
        "INPUT_SwitchCase" => Ok(Relations::INPUT_SwitchCase),
        "INPUT_Template" => Ok(Relations::INPUT_Template),
        "INPUT_Ternary" => Ok(Relations::INPUT_Ternary),
        "INPUT_Throw" => Ok(Relations::INPUT_Throw),
        "INPUT_Try" => Ok(Relations::INPUT_Try),
        "INPUT_UnaryOp" => Ok(Relations::INPUT_UnaryOp),
        "INPUT_VarDecl" => Ok(Relations::INPUT_VarDecl),
        "INPUT_While" => Ok(Relations::INPUT_While),
        "INPUT_With" => Ok(Relations::INPUT_With),
        "INPUT_Yield" => Ok(Relations::INPUT_Yield),
        "If" => Ok(Relations::If),
        "ImplicitGlobal" => Ok(Relations::ImplicitGlobal),
        "InputScope" => Ok(Relations::InputScope),
        "InvalidNameUse" => Ok(Relations::InvalidNameUse),
        "Label" => Ok(Relations::Label),
        "LetDecl" => Ok(Relations::LetDecl),
        "NameInScope" => Ok(Relations::NameInScope),
        "NameRef" => Ok(Relations::NameRef),
        "Property" => Ok(Relations::Property),
        "Return" => Ok(Relations::Return),
        "Statement" => Ok(Relations::Statement),
        "Switch" => Ok(Relations::Switch),
        "SwitchCase" => Ok(Relations::SwitchCase),
        "Template" => Ok(Relations::Template),
        "Ternary" => Ok(Relations::Ternary),
        "Throw" => Ok(Relations::Throw),
        "Try" => Ok(Relations::Try),
        "UnaryOp" => Ok(Relations::UnaryOp),
        "VarDecl" => Ok(Relations::VarDecl),
        "VarUseBeforeDeclaration" => Ok(Relations::VarUseBeforeDeclaration),
        "While" => Ok(Relations::While),
        "With" => Ok(Relations::With),
        "Yield" => Ok(Relations::Yield),
        "__Null" => Ok(Relations::__Null),
        "__Prefix_0" => Ok(Relations::__Prefix_0),
             _  => Err(())
         }
    }
}
impl Relations {
    pub fn is_output(&self) -> bool {
        match self {
        Relations::ChildScope => true,
        Relations::ClosestFunction => true,
        Relations::INPUT_Array => true,
        Relations::INPUT_Arrow => true,
        Relations::INPUT_ArrowParam => true,
        Relations::INPUT_Await => true,
        Relations::INPUT_BinOp => true,
        Relations::INPUT_BracketAccess => true,
        Relations::INPUT_Break => true,
        Relations::INPUT_ConstDecl => true,
        Relations::INPUT_Continue => true,
        Relations::INPUT_DoWhile => true,
        Relations::INPUT_DotAccess => true,
        Relations::INPUT_EveryScope => true,
        Relations::INPUT_ExprBigInt => true,
        Relations::INPUT_ExprBool => true,
        Relations::INPUT_ExprNumber => true,
        Relations::INPUT_ExprString => true,
        Relations::INPUT_Expression => true,
        Relations::INPUT_For => true,
        Relations::INPUT_ForIn => true,
        Relations::INPUT_Function => true,
        Relations::INPUT_FunctionArg => true,
        Relations::INPUT_If => true,
        Relations::INPUT_ImplicitGlobal => true,
        Relations::INPUT_InputScope => true,
        Relations::INPUT_Label => true,
        Relations::INPUT_LetDecl => true,
        Relations::INPUT_NameRef => true,
        Relations::INPUT_Property => true,
        Relations::INPUT_Return => true,
        Relations::INPUT_Statement => true,
        Relations::INPUT_Switch => true,
        Relations::INPUT_SwitchCase => true,
        Relations::INPUT_Template => true,
        Relations::INPUT_Ternary => true,
        Relations::INPUT_Throw => true,
        Relations::INPUT_Try => true,
        Relations::INPUT_UnaryOp => true,
        Relations::INPUT_VarDecl => true,
        Relations::INPUT_While => true,
        Relations::INPUT_With => true,
        Relations::INPUT_Yield => true,
        Relations::InvalidNameUse => true,
        Relations::NameInScope => true,
        Relations::VarUseBeforeDeclaration => true,
            _  => false
        }
    }
}
impl Relations {
    pub fn is_input(&self) -> bool {
        match self {
        Relations::Array => true,
        Relations::Arrow => true,
        Relations::ArrowParam => true,
        Relations::Await => true,
        Relations::BinOp => true,
        Relations::BracketAccess => true,
        Relations::Break => true,
        Relations::ConstDecl => true,
        Relations::Continue => true,
        Relations::DoWhile => true,
        Relations::DotAccess => true,
        Relations::EveryScope => true,
        Relations::ExprBigInt => true,
        Relations::ExprBool => true,
        Relations::ExprNumber => true,
        Relations::ExprString => true,
        Relations::Expression => true,
        Relations::For => true,
        Relations::ForIn => true,
        Relations::Function => true,
        Relations::FunctionArg => true,
        Relations::If => true,
        Relations::ImplicitGlobal => true,
        Relations::InputScope => true,
        Relations::Label => true,
        Relations::LetDecl => true,
        Relations::NameRef => true,
        Relations::Property => true,
        Relations::Return => true,
        Relations::Statement => true,
        Relations::Switch => true,
        Relations::SwitchCase => true,
        Relations::Template => true,
        Relations::Ternary => true,
        Relations::Throw => true,
        Relations::Try => true,
        Relations::UnaryOp => true,
        Relations::VarDecl => true,
        Relations::While => true,
        Relations::With => true,
        Relations::Yield => true,
            _  => false
        }
    }
}
impl TryFrom<RelId> for Relations {
    type Error = ();
    fn try_from(rid: RelId) -> ::std::result::Result<Self, ()> {
         match rid {
        0 => Ok(Relations::Array),
        1 => Ok(Relations::Arrow),
        2 => Ok(Relations::ArrowParam),
        3 => Ok(Relations::Await),
        4 => Ok(Relations::BinOp),
        5 => Ok(Relations::BracketAccess),
        6 => Ok(Relations::Break),
        7 => Ok(Relations::ChildScope),
        8 => Ok(Relations::ClosestFunction),
        9 => Ok(Relations::ConstDecl),
        10 => Ok(Relations::Continue),
        11 => Ok(Relations::DoWhile),
        12 => Ok(Relations::DotAccess),
        13 => Ok(Relations::EveryScope),
        14 => Ok(Relations::ExprBigInt),
        15 => Ok(Relations::ExprBool),
        16 => Ok(Relations::ExprNumber),
        17 => Ok(Relations::ExprString),
        18 => Ok(Relations::Expression),
        19 => Ok(Relations::For),
        20 => Ok(Relations::ForIn),
        21 => Ok(Relations::Function),
        22 => Ok(Relations::FunctionArg),
        23 => Ok(Relations::INPUT_Array),
        24 => Ok(Relations::INPUT_Arrow),
        25 => Ok(Relations::INPUT_ArrowParam),
        26 => Ok(Relations::INPUT_Await),
        27 => Ok(Relations::INPUT_BinOp),
        28 => Ok(Relations::INPUT_BracketAccess),
        29 => Ok(Relations::INPUT_Break),
        30 => Ok(Relations::INPUT_ConstDecl),
        31 => Ok(Relations::INPUT_Continue),
        32 => Ok(Relations::INPUT_DoWhile),
        33 => Ok(Relations::INPUT_DotAccess),
        34 => Ok(Relations::INPUT_EveryScope),
        35 => Ok(Relations::INPUT_ExprBigInt),
        36 => Ok(Relations::INPUT_ExprBool),
        37 => Ok(Relations::INPUT_ExprNumber),
        38 => Ok(Relations::INPUT_ExprString),
        39 => Ok(Relations::INPUT_Expression),
        40 => Ok(Relations::INPUT_For),
        41 => Ok(Relations::INPUT_ForIn),
        42 => Ok(Relations::INPUT_Function),
        43 => Ok(Relations::INPUT_FunctionArg),
        44 => Ok(Relations::INPUT_If),
        45 => Ok(Relations::INPUT_ImplicitGlobal),
        46 => Ok(Relations::INPUT_InputScope),
        47 => Ok(Relations::INPUT_Label),
        48 => Ok(Relations::INPUT_LetDecl),
        49 => Ok(Relations::INPUT_NameRef),
        50 => Ok(Relations::INPUT_Property),
        51 => Ok(Relations::INPUT_Return),
        52 => Ok(Relations::INPUT_Statement),
        53 => Ok(Relations::INPUT_Switch),
        54 => Ok(Relations::INPUT_SwitchCase),
        55 => Ok(Relations::INPUT_Template),
        56 => Ok(Relations::INPUT_Ternary),
        57 => Ok(Relations::INPUT_Throw),
        58 => Ok(Relations::INPUT_Try),
        59 => Ok(Relations::INPUT_UnaryOp),
        60 => Ok(Relations::INPUT_VarDecl),
        61 => Ok(Relations::INPUT_While),
        62 => Ok(Relations::INPUT_With),
        63 => Ok(Relations::INPUT_Yield),
        64 => Ok(Relations::If),
        65 => Ok(Relations::ImplicitGlobal),
        66 => Ok(Relations::InputScope),
        67 => Ok(Relations::InvalidNameUse),
        68 => Ok(Relations::Label),
        69 => Ok(Relations::LetDecl),
        70 => Ok(Relations::NameInScope),
        71 => Ok(Relations::NameRef),
        72 => Ok(Relations::Property),
        73 => Ok(Relations::Return),
        74 => Ok(Relations::Statement),
        75 => Ok(Relations::Switch),
        76 => Ok(Relations::SwitchCase),
        77 => Ok(Relations::Template),
        78 => Ok(Relations::Ternary),
        79 => Ok(Relations::Throw),
        80 => Ok(Relations::Try),
        81 => Ok(Relations::UnaryOp),
        82 => Ok(Relations::VarDecl),
        83 => Ok(Relations::VarUseBeforeDeclaration),
        84 => Ok(Relations::While),
        85 => Ok(Relations::With),
        86 => Ok(Relations::Yield),
        87 => Ok(Relations::__Null),
        88 => Ok(Relations::__Prefix_0),
             _  => Err(())
         }
    }
}
pub fn relid2name(rid: RelId) -> Option<&'static str> {
   match rid {
        0 => Some(&"Array"),
        1 => Some(&"Arrow"),
        2 => Some(&"ArrowParam"),
        3 => Some(&"Await"),
        4 => Some(&"BinOp"),
        5 => Some(&"BracketAccess"),
        6 => Some(&"Break"),
        7 => Some(&"ChildScope"),
        8 => Some(&"ClosestFunction"),
        9 => Some(&"ConstDecl"),
        10 => Some(&"Continue"),
        11 => Some(&"DoWhile"),
        12 => Some(&"DotAccess"),
        13 => Some(&"EveryScope"),
        14 => Some(&"ExprBigInt"),
        15 => Some(&"ExprBool"),
        16 => Some(&"ExprNumber"),
        17 => Some(&"ExprString"),
        18 => Some(&"Expression"),
        19 => Some(&"For"),
        20 => Some(&"ForIn"),
        21 => Some(&"Function"),
        22 => Some(&"FunctionArg"),
        23 => Some(&"INPUT_Array"),
        24 => Some(&"INPUT_Arrow"),
        25 => Some(&"INPUT_ArrowParam"),
        26 => Some(&"INPUT_Await"),
        27 => Some(&"INPUT_BinOp"),
        28 => Some(&"INPUT_BracketAccess"),
        29 => Some(&"INPUT_Break"),
        30 => Some(&"INPUT_ConstDecl"),
        31 => Some(&"INPUT_Continue"),
        32 => Some(&"INPUT_DoWhile"),
        33 => Some(&"INPUT_DotAccess"),
        34 => Some(&"INPUT_EveryScope"),
        35 => Some(&"INPUT_ExprBigInt"),
        36 => Some(&"INPUT_ExprBool"),
        37 => Some(&"INPUT_ExprNumber"),
        38 => Some(&"INPUT_ExprString"),
        39 => Some(&"INPUT_Expression"),
        40 => Some(&"INPUT_For"),
        41 => Some(&"INPUT_ForIn"),
        42 => Some(&"INPUT_Function"),
        43 => Some(&"INPUT_FunctionArg"),
        44 => Some(&"INPUT_If"),
        45 => Some(&"INPUT_ImplicitGlobal"),
        46 => Some(&"INPUT_InputScope"),
        47 => Some(&"INPUT_Label"),
        48 => Some(&"INPUT_LetDecl"),
        49 => Some(&"INPUT_NameRef"),
        50 => Some(&"INPUT_Property"),
        51 => Some(&"INPUT_Return"),
        52 => Some(&"INPUT_Statement"),
        53 => Some(&"INPUT_Switch"),
        54 => Some(&"INPUT_SwitchCase"),
        55 => Some(&"INPUT_Template"),
        56 => Some(&"INPUT_Ternary"),
        57 => Some(&"INPUT_Throw"),
        58 => Some(&"INPUT_Try"),
        59 => Some(&"INPUT_UnaryOp"),
        60 => Some(&"INPUT_VarDecl"),
        61 => Some(&"INPUT_While"),
        62 => Some(&"INPUT_With"),
        63 => Some(&"INPUT_Yield"),
        64 => Some(&"If"),
        65 => Some(&"ImplicitGlobal"),
        66 => Some(&"InputScope"),
        67 => Some(&"InvalidNameUse"),
        68 => Some(&"Label"),
        69 => Some(&"LetDecl"),
        70 => Some(&"NameInScope"),
        71 => Some(&"NameRef"),
        72 => Some(&"Property"),
        73 => Some(&"Return"),
        74 => Some(&"Statement"),
        75 => Some(&"Switch"),
        76 => Some(&"SwitchCase"),
        77 => Some(&"Template"),
        78 => Some(&"Ternary"),
        79 => Some(&"Throw"),
        80 => Some(&"Try"),
        81 => Some(&"UnaryOp"),
        82 => Some(&"VarDecl"),
        83 => Some(&"VarUseBeforeDeclaration"),
        84 => Some(&"While"),
        85 => Some(&"With"),
        86 => Some(&"Yield"),
        87 => Some(&"__Null"),
        88 => Some(&"__Prefix_0"),
       _  => None
   }
}
pub fn relid2cname(rid: RelId) -> Option<&'static ::std::ffi::CStr> {
    RELIDMAPC.get(&rid).copied()
}   /// A map of `RelId`s to their name as an `&'static str`
pub static RELIDMAP: ::once_cell::sync::Lazy<::fnv::FnvHashMap<Relations, &'static str>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(89, ::fnv::FnvBuildHasher::default());
        map.insert(Relations::Array, "Array");
        map.insert(Relations::Arrow, "Arrow");
        map.insert(Relations::ArrowParam, "ArrowParam");
        map.insert(Relations::Await, "Await");
        map.insert(Relations::BinOp, "BinOp");
        map.insert(Relations::BracketAccess, "BracketAccess");
        map.insert(Relations::Break, "Break");
        map.insert(Relations::ChildScope, "ChildScope");
        map.insert(Relations::ClosestFunction, "ClosestFunction");
        map.insert(Relations::ConstDecl, "ConstDecl");
        map.insert(Relations::Continue, "Continue");
        map.insert(Relations::DoWhile, "DoWhile");
        map.insert(Relations::DotAccess, "DotAccess");
        map.insert(Relations::EveryScope, "EveryScope");
        map.insert(Relations::ExprBigInt, "ExprBigInt");
        map.insert(Relations::ExprBool, "ExprBool");
        map.insert(Relations::ExprNumber, "ExprNumber");
        map.insert(Relations::ExprString, "ExprString");
        map.insert(Relations::Expression, "Expression");
        map.insert(Relations::For, "For");
        map.insert(Relations::ForIn, "ForIn");
        map.insert(Relations::Function, "Function");
        map.insert(Relations::FunctionArg, "FunctionArg");
        map.insert(Relations::INPUT_Array, "INPUT_Array");
        map.insert(Relations::INPUT_Arrow, "INPUT_Arrow");
        map.insert(Relations::INPUT_ArrowParam, "INPUT_ArrowParam");
        map.insert(Relations::INPUT_Await, "INPUT_Await");
        map.insert(Relations::INPUT_BinOp, "INPUT_BinOp");
        map.insert(Relations::INPUT_BracketAccess, "INPUT_BracketAccess");
        map.insert(Relations::INPUT_Break, "INPUT_Break");
        map.insert(Relations::INPUT_ConstDecl, "INPUT_ConstDecl");
        map.insert(Relations::INPUT_Continue, "INPUT_Continue");
        map.insert(Relations::INPUT_DoWhile, "INPUT_DoWhile");
        map.insert(Relations::INPUT_DotAccess, "INPUT_DotAccess");
        map.insert(Relations::INPUT_EveryScope, "INPUT_EveryScope");
        map.insert(Relations::INPUT_ExprBigInt, "INPUT_ExprBigInt");
        map.insert(Relations::INPUT_ExprBool, "INPUT_ExprBool");
        map.insert(Relations::INPUT_ExprNumber, "INPUT_ExprNumber");
        map.insert(Relations::INPUT_ExprString, "INPUT_ExprString");
        map.insert(Relations::INPUT_Expression, "INPUT_Expression");
        map.insert(Relations::INPUT_For, "INPUT_For");
        map.insert(Relations::INPUT_ForIn, "INPUT_ForIn");
        map.insert(Relations::INPUT_Function, "INPUT_Function");
        map.insert(Relations::INPUT_FunctionArg, "INPUT_FunctionArg");
        map.insert(Relations::INPUT_If, "INPUT_If");
        map.insert(Relations::INPUT_ImplicitGlobal, "INPUT_ImplicitGlobal");
        map.insert(Relations::INPUT_InputScope, "INPUT_InputScope");
        map.insert(Relations::INPUT_Label, "INPUT_Label");
        map.insert(Relations::INPUT_LetDecl, "INPUT_LetDecl");
        map.insert(Relations::INPUT_NameRef, "INPUT_NameRef");
        map.insert(Relations::INPUT_Property, "INPUT_Property");
        map.insert(Relations::INPUT_Return, "INPUT_Return");
        map.insert(Relations::INPUT_Statement, "INPUT_Statement");
        map.insert(Relations::INPUT_Switch, "INPUT_Switch");
        map.insert(Relations::INPUT_SwitchCase, "INPUT_SwitchCase");
        map.insert(Relations::INPUT_Template, "INPUT_Template");
        map.insert(Relations::INPUT_Ternary, "INPUT_Ternary");
        map.insert(Relations::INPUT_Throw, "INPUT_Throw");
        map.insert(Relations::INPUT_Try, "INPUT_Try");
        map.insert(Relations::INPUT_UnaryOp, "INPUT_UnaryOp");
        map.insert(Relations::INPUT_VarDecl, "INPUT_VarDecl");
        map.insert(Relations::INPUT_While, "INPUT_While");
        map.insert(Relations::INPUT_With, "INPUT_With");
        map.insert(Relations::INPUT_Yield, "INPUT_Yield");
        map.insert(Relations::If, "If");
        map.insert(Relations::ImplicitGlobal, "ImplicitGlobal");
        map.insert(Relations::InputScope, "InputScope");
        map.insert(Relations::InvalidNameUse, "InvalidNameUse");
        map.insert(Relations::Label, "Label");
        map.insert(Relations::LetDecl, "LetDecl");
        map.insert(Relations::NameInScope, "NameInScope");
        map.insert(Relations::NameRef, "NameRef");
        map.insert(Relations::Property, "Property");
        map.insert(Relations::Return, "Return");
        map.insert(Relations::Statement, "Statement");
        map.insert(Relations::Switch, "Switch");
        map.insert(Relations::SwitchCase, "SwitchCase");
        map.insert(Relations::Template, "Template");
        map.insert(Relations::Ternary, "Ternary");
        map.insert(Relations::Throw, "Throw");
        map.insert(Relations::Try, "Try");
        map.insert(Relations::UnaryOp, "UnaryOp");
        map.insert(Relations::VarDecl, "VarDecl");
        map.insert(Relations::VarUseBeforeDeclaration, "VarUseBeforeDeclaration");
        map.insert(Relations::While, "While");
        map.insert(Relations::With, "With");
        map.insert(Relations::Yield, "Yield");
        map.insert(Relations::__Null, "__Null");
        map.insert(Relations::__Prefix_0, "__Prefix_0");
        map
    });
    /// A map of `RelId`s to their name as an `&'static CStr`
pub static RELIDMAPC: ::once_cell::sync::Lazy<::fnv::FnvHashMap<RelId, &'static ::std::ffi::CStr>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(89, ::fnv::FnvBuildHasher::default());
        map.insert(0, ::std::ffi::CStr::from_bytes_with_nul(b"Array\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(1, ::std::ffi::CStr::from_bytes_with_nul(b"Arrow\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(2, ::std::ffi::CStr::from_bytes_with_nul(b"ArrowParam\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(3, ::std::ffi::CStr::from_bytes_with_nul(b"Await\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(4, ::std::ffi::CStr::from_bytes_with_nul(b"BinOp\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(5, ::std::ffi::CStr::from_bytes_with_nul(b"BracketAccess\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(6, ::std::ffi::CStr::from_bytes_with_nul(b"Break\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(7, ::std::ffi::CStr::from_bytes_with_nul(b"ChildScope\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(8, ::std::ffi::CStr::from_bytes_with_nul(b"ClosestFunction\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(9, ::std::ffi::CStr::from_bytes_with_nul(b"ConstDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(10, ::std::ffi::CStr::from_bytes_with_nul(b"Continue\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(11, ::std::ffi::CStr::from_bytes_with_nul(b"DoWhile\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(12, ::std::ffi::CStr::from_bytes_with_nul(b"DotAccess\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(13, ::std::ffi::CStr::from_bytes_with_nul(b"EveryScope\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(14, ::std::ffi::CStr::from_bytes_with_nul(b"ExprBigInt\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(15, ::std::ffi::CStr::from_bytes_with_nul(b"ExprBool\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(16, ::std::ffi::CStr::from_bytes_with_nul(b"ExprNumber\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(17, ::std::ffi::CStr::from_bytes_with_nul(b"ExprString\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(18, ::std::ffi::CStr::from_bytes_with_nul(b"Expression\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(19, ::std::ffi::CStr::from_bytes_with_nul(b"For\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(20, ::std::ffi::CStr::from_bytes_with_nul(b"ForIn\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(21, ::std::ffi::CStr::from_bytes_with_nul(b"Function\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(22, ::std::ffi::CStr::from_bytes_with_nul(b"FunctionArg\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(23, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Array\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(24, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Arrow\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(25, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ArrowParam\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(26, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Await\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(27, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_BinOp\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(28, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_BracketAccess\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(29, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Break\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(30, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ConstDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(31, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Continue\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(32, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_DoWhile\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(33, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_DotAccess\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(34, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_EveryScope\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(35, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprBigInt\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(36, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprBool\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(37, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprNumber\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(38, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprString\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(39, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Expression\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(40, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_For\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(41, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ForIn\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(42, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Function\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(43, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_FunctionArg\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(44, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_If\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(45, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ImplicitGlobal\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(46, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_InputScope\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(47, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Label\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(48, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_LetDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(49, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_NameRef\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(50, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Property\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(51, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Return\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(52, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Statement\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(53, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Switch\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(54, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_SwitchCase\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(55, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Template\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(56, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Ternary\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(57, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Throw\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(58, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Try\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(59, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_UnaryOp\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(60, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_VarDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(61, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_While\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(62, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_With\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(63, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Yield\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(64, ::std::ffi::CStr::from_bytes_with_nul(b"If\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(65, ::std::ffi::CStr::from_bytes_with_nul(b"ImplicitGlobal\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(66, ::std::ffi::CStr::from_bytes_with_nul(b"InputScope\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(67, ::std::ffi::CStr::from_bytes_with_nul(b"InvalidNameUse\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(68, ::std::ffi::CStr::from_bytes_with_nul(b"Label\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(69, ::std::ffi::CStr::from_bytes_with_nul(b"LetDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(70, ::std::ffi::CStr::from_bytes_with_nul(b"NameInScope\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(71, ::std::ffi::CStr::from_bytes_with_nul(b"NameRef\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(72, ::std::ffi::CStr::from_bytes_with_nul(b"Property\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(73, ::std::ffi::CStr::from_bytes_with_nul(b"Return\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(74, ::std::ffi::CStr::from_bytes_with_nul(b"Statement\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(75, ::std::ffi::CStr::from_bytes_with_nul(b"Switch\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(76, ::std::ffi::CStr::from_bytes_with_nul(b"SwitchCase\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(77, ::std::ffi::CStr::from_bytes_with_nul(b"Template\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(78, ::std::ffi::CStr::from_bytes_with_nul(b"Ternary\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(79, ::std::ffi::CStr::from_bytes_with_nul(b"Throw\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(80, ::std::ffi::CStr::from_bytes_with_nul(b"Try\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(81, ::std::ffi::CStr::from_bytes_with_nul(b"UnaryOp\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(82, ::std::ffi::CStr::from_bytes_with_nul(b"VarDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(83, ::std::ffi::CStr::from_bytes_with_nul(b"VarUseBeforeDeclaration\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(84, ::std::ffi::CStr::from_bytes_with_nul(b"While\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(85, ::std::ffi::CStr::from_bytes_with_nul(b"With\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(86, ::std::ffi::CStr::from_bytes_with_nul(b"Yield\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(87, ::std::ffi::CStr::from_bytes_with_nul(b"__Null\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(88, ::std::ffi::CStr::from_bytes_with_nul(b"__Prefix_0\0").expect("Unreachable: A null byte was specifically inserted"));
        map
    });
    /// A map of input `Relations`s to their name as an `&'static str`
pub static INPUT_RELIDMAP: ::once_cell::sync::Lazy<::fnv::FnvHashMap<Relations, &'static str>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(41, ::fnv::FnvBuildHasher::default());
        map.insert(Relations::Array, "Array");
        map.insert(Relations::Arrow, "Arrow");
        map.insert(Relations::ArrowParam, "ArrowParam");
        map.insert(Relations::Await, "Await");
        map.insert(Relations::BinOp, "BinOp");
        map.insert(Relations::BracketAccess, "BracketAccess");
        map.insert(Relations::Break, "Break");
        map.insert(Relations::ConstDecl, "ConstDecl");
        map.insert(Relations::Continue, "Continue");
        map.insert(Relations::DoWhile, "DoWhile");
        map.insert(Relations::DotAccess, "DotAccess");
        map.insert(Relations::EveryScope, "EveryScope");
        map.insert(Relations::ExprBigInt, "ExprBigInt");
        map.insert(Relations::ExprBool, "ExprBool");
        map.insert(Relations::ExprNumber, "ExprNumber");
        map.insert(Relations::ExprString, "ExprString");
        map.insert(Relations::Expression, "Expression");
        map.insert(Relations::For, "For");
        map.insert(Relations::ForIn, "ForIn");
        map.insert(Relations::Function, "Function");
        map.insert(Relations::FunctionArg, "FunctionArg");
        map.insert(Relations::If, "If");
        map.insert(Relations::ImplicitGlobal, "ImplicitGlobal");
        map.insert(Relations::InputScope, "InputScope");
        map.insert(Relations::Label, "Label");
        map.insert(Relations::LetDecl, "LetDecl");
        map.insert(Relations::NameRef, "NameRef");
        map.insert(Relations::Property, "Property");
        map.insert(Relations::Return, "Return");
        map.insert(Relations::Statement, "Statement");
        map.insert(Relations::Switch, "Switch");
        map.insert(Relations::SwitchCase, "SwitchCase");
        map.insert(Relations::Template, "Template");
        map.insert(Relations::Ternary, "Ternary");
        map.insert(Relations::Throw, "Throw");
        map.insert(Relations::Try, "Try");
        map.insert(Relations::UnaryOp, "UnaryOp");
        map.insert(Relations::VarDecl, "VarDecl");
        map.insert(Relations::While, "While");
        map.insert(Relations::With, "With");
        map.insert(Relations::Yield, "Yield");
        map
    });
    /// A map of output `Relations`s to their name as an `&'static str`
pub static OUTPUT_RELIDMAP: ::once_cell::sync::Lazy<::fnv::FnvHashMap<Relations, &'static str>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(46, ::fnv::FnvBuildHasher::default());
        map.insert(Relations::ChildScope, "ChildScope");
        map.insert(Relations::ClosestFunction, "ClosestFunction");
        map.insert(Relations::INPUT_Array, "INPUT_Array");
        map.insert(Relations::INPUT_Arrow, "INPUT_Arrow");
        map.insert(Relations::INPUT_ArrowParam, "INPUT_ArrowParam");
        map.insert(Relations::INPUT_Await, "INPUT_Await");
        map.insert(Relations::INPUT_BinOp, "INPUT_BinOp");
        map.insert(Relations::INPUT_BracketAccess, "INPUT_BracketAccess");
        map.insert(Relations::INPUT_Break, "INPUT_Break");
        map.insert(Relations::INPUT_ConstDecl, "INPUT_ConstDecl");
        map.insert(Relations::INPUT_Continue, "INPUT_Continue");
        map.insert(Relations::INPUT_DoWhile, "INPUT_DoWhile");
        map.insert(Relations::INPUT_DotAccess, "INPUT_DotAccess");
        map.insert(Relations::INPUT_EveryScope, "INPUT_EveryScope");
        map.insert(Relations::INPUT_ExprBigInt, "INPUT_ExprBigInt");
        map.insert(Relations::INPUT_ExprBool, "INPUT_ExprBool");
        map.insert(Relations::INPUT_ExprNumber, "INPUT_ExprNumber");
        map.insert(Relations::INPUT_ExprString, "INPUT_ExprString");
        map.insert(Relations::INPUT_Expression, "INPUT_Expression");
        map.insert(Relations::INPUT_For, "INPUT_For");
        map.insert(Relations::INPUT_ForIn, "INPUT_ForIn");
        map.insert(Relations::INPUT_Function, "INPUT_Function");
        map.insert(Relations::INPUT_FunctionArg, "INPUT_FunctionArg");
        map.insert(Relations::INPUT_If, "INPUT_If");
        map.insert(Relations::INPUT_ImplicitGlobal, "INPUT_ImplicitGlobal");
        map.insert(Relations::INPUT_InputScope, "INPUT_InputScope");
        map.insert(Relations::INPUT_Label, "INPUT_Label");
        map.insert(Relations::INPUT_LetDecl, "INPUT_LetDecl");
        map.insert(Relations::INPUT_NameRef, "INPUT_NameRef");
        map.insert(Relations::INPUT_Property, "INPUT_Property");
        map.insert(Relations::INPUT_Return, "INPUT_Return");
        map.insert(Relations::INPUT_Statement, "INPUT_Statement");
        map.insert(Relations::INPUT_Switch, "INPUT_Switch");
        map.insert(Relations::INPUT_SwitchCase, "INPUT_SwitchCase");
        map.insert(Relations::INPUT_Template, "INPUT_Template");
        map.insert(Relations::INPUT_Ternary, "INPUT_Ternary");
        map.insert(Relations::INPUT_Throw, "INPUT_Throw");
        map.insert(Relations::INPUT_Try, "INPUT_Try");
        map.insert(Relations::INPUT_UnaryOp, "INPUT_UnaryOp");
        map.insert(Relations::INPUT_VarDecl, "INPUT_VarDecl");
        map.insert(Relations::INPUT_While, "INPUT_While");
        map.insert(Relations::INPUT_With, "INPUT_With");
        map.insert(Relations::INPUT_Yield, "INPUT_Yield");
        map.insert(Relations::InvalidNameUse, "InvalidNameUse");
        map.insert(Relations::NameInScope, "NameInScope");
        map.insert(Relations::VarUseBeforeDeclaration, "VarUseBeforeDeclaration");
        map
    });
impl TryFrom<&str> for Indexes {
    type Error = ();
    fn try_from(iname: &str) -> ::std::result::Result<Self, ()> {
         match iname {
        "__Null_by_none" => Ok(Indexes::__Null_by_none),
             _  => Err(())
         }
    }
}
impl TryFrom<IdxId> for Indexes {
    type Error = ();
    fn try_from(iid: IdxId) -> ::core::result::Result<Self, ()> {
         match iid {
        0 => Ok(Indexes::__Null_by_none),
             _  => Err(())
         }
    }
}
pub fn indexid2name(iid: IdxId) -> Option<&'static str> {
   match iid {
        0 => Some(&"__Null_by_none"),
       _  => None
   }
}
pub fn indexid2cname(iid: IdxId) -> Option<&'static ::std::ffi::CStr> {
    IDXIDMAPC.get(&iid).copied()
}   /// A map of `Indexes` to their name as an `&'static str`
pub static IDXIDMAP: ::once_cell::sync::Lazy<::fnv::FnvHashMap<Indexes, &'static str>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(1, ::fnv::FnvBuildHasher::default());
        map.insert(Indexes::__Null_by_none, "__Null_by_none");
        map
    });
    /// A map of `IdxId`s to their name as an `&'static CStr`
pub static IDXIDMAPC: ::once_cell::sync::Lazy<::fnv::FnvHashMap<IdxId, &'static ::std::ffi::CStr>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(1, ::fnv::FnvBuildHasher::default());
        map.insert(0, ::std::ffi::CStr::from_bytes_with_nul(b"__Null_by_none\0").expect("Unreachable: A null byte was specifically inserted"));
        map
    });
pub fn relval_from_record(rel: Relations, _rec: &differential_datalog::record::Record) -> ::std::result::Result<DDValue, String> {
    match rel {
        Relations::Array => {
            Ok(<::types::Array>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Arrow => {
            Ok(<::types::Arrow>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ArrowParam => {
            Ok(<::types::ArrowParam>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Await => {
            Ok(<::types::Await>::from_record(_rec)?.into_ddvalue())
        },
        Relations::BinOp => {
            Ok(<::types::BinOp>::from_record(_rec)?.into_ddvalue())
        },
        Relations::BracketAccess => {
            Ok(<::types::BracketAccess>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Break => {
            Ok(<::types::Break>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ChildScope => {
            Ok(<::types::ChildScope>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ClosestFunction => {
            Ok(<::types::ClosestFunction>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ConstDecl => {
            Ok(<::types::ConstDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Continue => {
            Ok(<::types::Continue>::from_record(_rec)?.into_ddvalue())
        },
        Relations::DoWhile => {
            Ok(<::types::DoWhile>::from_record(_rec)?.into_ddvalue())
        },
        Relations::DotAccess => {
            Ok(<::types::DotAccess>::from_record(_rec)?.into_ddvalue())
        },
        Relations::EveryScope => {
            Ok(<::types::EveryScope>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ExprBigInt => {
            Ok(<::types::ExprBigInt>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ExprBool => {
            Ok(<::types::ExprBool>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ExprNumber => {
            Ok(<::types::ExprNumber>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ExprString => {
            Ok(<::types::ExprString>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Expression => {
            Ok(<::types::Expression>::from_record(_rec)?.into_ddvalue())
        },
        Relations::For => {
            Ok(<::types::For>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ForIn => {
            Ok(<::types::ForIn>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Function => {
            Ok(<::types::Function>::from_record(_rec)?.into_ddvalue())
        },
        Relations::FunctionArg => {
            Ok(<::types::FunctionArg>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Array => {
            Ok(<::types::Array>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Arrow => {
            Ok(<::types::Arrow>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ArrowParam => {
            Ok(<::types::ArrowParam>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Await => {
            Ok(<::types::Await>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_BinOp => {
            Ok(<::types::BinOp>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_BracketAccess => {
            Ok(<::types::BracketAccess>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Break => {
            Ok(<::types::Break>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ConstDecl => {
            Ok(<::types::ConstDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Continue => {
            Ok(<::types::Continue>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_DoWhile => {
            Ok(<::types::DoWhile>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_DotAccess => {
            Ok(<::types::DotAccess>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_EveryScope => {
            Ok(<::types::EveryScope>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ExprBigInt => {
            Ok(<::types::ExprBigInt>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ExprBool => {
            Ok(<::types::ExprBool>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ExprNumber => {
            Ok(<::types::ExprNumber>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ExprString => {
            Ok(<::types::ExprString>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Expression => {
            Ok(<::types::Expression>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_For => {
            Ok(<::types::For>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ForIn => {
            Ok(<::types::ForIn>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Function => {
            Ok(<::types::Function>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_FunctionArg => {
            Ok(<::types::FunctionArg>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_If => {
            Ok(<::types::If>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ImplicitGlobal => {
            Ok(<::types::ImplicitGlobal>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_InputScope => {
            Ok(<::types::InputScope>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Label => {
            Ok(<::types::Label>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_LetDecl => {
            Ok(<::types::LetDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_NameRef => {
            Ok(<::types::NameRef>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Property => {
            Ok(<::types::Property>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Return => {
            Ok(<::types::Return>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Statement => {
            Ok(<::types::Statement>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Switch => {
            Ok(<::types::Switch>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_SwitchCase => {
            Ok(<::types::SwitchCase>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Template => {
            Ok(<::types::Template>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Ternary => {
            Ok(<::types::Ternary>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Throw => {
            Ok(<::types::Throw>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Try => {
            Ok(<::types::Try>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_UnaryOp => {
            Ok(<::types::UnaryOp>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_VarDecl => {
            Ok(<::types::VarDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_While => {
            Ok(<::types::While>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_With => {
            Ok(<::types::With>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Yield => {
            Ok(<::types::Yield>::from_record(_rec)?.into_ddvalue())
        },
        Relations::If => {
            Ok(<::types::If>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ImplicitGlobal => {
            Ok(<::types::ImplicitGlobal>::from_record(_rec)?.into_ddvalue())
        },
        Relations::InputScope => {
            Ok(<::types::InputScope>::from_record(_rec)?.into_ddvalue())
        },
        Relations::InvalidNameUse => {
            Ok(<::types::InvalidNameUse>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Label => {
            Ok(<::types::Label>::from_record(_rec)?.into_ddvalue())
        },
        Relations::LetDecl => {
            Ok(<::types::LetDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::NameInScope => {
            Ok(<::types::NameInScope>::from_record(_rec)?.into_ddvalue())
        },
        Relations::NameRef => {
            Ok(<::types::NameRef>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Property => {
            Ok(<::types::Property>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Return => {
            Ok(<::types::Return>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Statement => {
            Ok(<::types::Statement>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Switch => {
            Ok(<::types::Switch>::from_record(_rec)?.into_ddvalue())
        },
        Relations::SwitchCase => {
            Ok(<::types::SwitchCase>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Template => {
            Ok(<::types::Template>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Ternary => {
            Ok(<::types::Ternary>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Throw => {
            Ok(<::types::Throw>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Try => {
            Ok(<::types::Try>::from_record(_rec)?.into_ddvalue())
        },
        Relations::UnaryOp => {
            Ok(<::types::UnaryOp>::from_record(_rec)?.into_ddvalue())
        },
        Relations::VarDecl => {
            Ok(<::types::VarDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::VarUseBeforeDeclaration => {
            Ok(<::types::VarUseBeforeDeclaration>::from_record(_rec)?.into_ddvalue())
        },
        Relations::While => {
            Ok(<::types::While>::from_record(_rec)?.into_ddvalue())
        },
        Relations::With => {
            Ok(<::types::With>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Yield => {
            Ok(<::types::Yield>::from_record(_rec)?.into_ddvalue())
        },
        Relations::__Null => {
            Ok(<()>::from_record(_rec)?.into_ddvalue())
        },
        Relations::__Prefix_0 => {
            Ok(<::types::ddlog_std::tuple3<::types::ExprId, ::types::internment::Intern<::types::Pattern>, ::types::internment::Intern<String>>>::from_record(_rec)?.into_ddvalue())
        }
    }
}
pub fn relkey_from_record(rel: Relations, _rec: &differential_datalog::record::Record) -> ::std::result::Result<DDValue, String> {
    match rel {
        _ => Err(format!("relation {:?} does not have a primary key", rel))
    }
}
pub fn idxkey_from_record(idx: Indexes, _rec: &differential_datalog::record::Record) -> ::std::result::Result<DDValue, String> {
    match idx {
        Indexes::__Null_by_none => {
            Ok(<()>::from_record(_rec)?.into_ddvalue())
        }
    }
}
pub fn indexes2arrid(idx: Indexes) -> ArrId {
    match idx {
        Indexes::__Null_by_none => ( 87, 0),
    }
}
#[derive(Copy,Clone,Debug,PartialEq,Eq,Hash)]
pub enum Relations {
    Array = 0,
    Arrow = 1,
    ArrowParam = 2,
    Await = 3,
    BinOp = 4,
    BracketAccess = 5,
    Break = 6,
    ChildScope = 7,
    ClosestFunction = 8,
    ConstDecl = 9,
    Continue = 10,
    DoWhile = 11,
    DotAccess = 12,
    EveryScope = 13,
    ExprBigInt = 14,
    ExprBool = 15,
    ExprNumber = 16,
    ExprString = 17,
    Expression = 18,
    For = 19,
    ForIn = 20,
    Function = 21,
    FunctionArg = 22,
    INPUT_Array = 23,
    INPUT_Arrow = 24,
    INPUT_ArrowParam = 25,
    INPUT_Await = 26,
    INPUT_BinOp = 27,
    INPUT_BracketAccess = 28,
    INPUT_Break = 29,
    INPUT_ConstDecl = 30,
    INPUT_Continue = 31,
    INPUT_DoWhile = 32,
    INPUT_DotAccess = 33,
    INPUT_EveryScope = 34,
    INPUT_ExprBigInt = 35,
    INPUT_ExprBool = 36,
    INPUT_ExprNumber = 37,
    INPUT_ExprString = 38,
    INPUT_Expression = 39,
    INPUT_For = 40,
    INPUT_ForIn = 41,
    INPUT_Function = 42,
    INPUT_FunctionArg = 43,
    INPUT_If = 44,
    INPUT_ImplicitGlobal = 45,
    INPUT_InputScope = 46,
    INPUT_Label = 47,
    INPUT_LetDecl = 48,
    INPUT_NameRef = 49,
    INPUT_Property = 50,
    INPUT_Return = 51,
    INPUT_Statement = 52,
    INPUT_Switch = 53,
    INPUT_SwitchCase = 54,
    INPUT_Template = 55,
    INPUT_Ternary = 56,
    INPUT_Throw = 57,
    INPUT_Try = 58,
    INPUT_UnaryOp = 59,
    INPUT_VarDecl = 60,
    INPUT_While = 61,
    INPUT_With = 62,
    INPUT_Yield = 63,
    If = 64,
    ImplicitGlobal = 65,
    InputScope = 66,
    InvalidNameUse = 67,
    Label = 68,
    LetDecl = 69,
    NameInScope = 70,
    NameRef = 71,
    Property = 72,
    Return = 73,
    Statement = 74,
    Switch = 75,
    SwitchCase = 76,
    Template = 77,
    Ternary = 78,
    Throw = 79,
    Try = 80,
    UnaryOp = 81,
    VarDecl = 82,
    VarUseBeforeDeclaration = 83,
    While = 84,
    With = 85,
    Yield = 86,
    __Null = 87,
    __Prefix_0 = 88
}
#[derive(Copy,Clone,Debug,PartialEq,Eq,Hash)]
pub enum Indexes {
    __Null_by_none = 0
}
pub fn prog(__update_cb: Box<dyn CBFn>) -> Program {
    let Array = Relation {
                    name:         "Array".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::Array as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_Array = Relation {
                          name:         "INPUT_Array".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_Array as RelId,
                          rules:        vec![
                              /* INPUT_Array[x] :- Array[(x: Array)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_Array[x] :- Array[(x: Array)].".to_string(),
                                  rel: Relations::Array as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_Array[x] :- Array[(x: Array)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::Array>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let Arrow = Relation {
                    name:         "Arrow".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::Arrow as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        Arrangement::Map{
                           name: r###"(Arrow{.expr_id=(_0: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Left{.l=(_: ExprId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow) /*join*/"###.to_string(),
                            afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                            {
                                let __cloned = __v.clone();
                                match unsafe {< ::types::Arrow>::from_ddvalue(__v) } {
                                    ::types::Arrow{expr_id: ref _0, body: ::types::ddlog_std::Option::Some{x: ::types::ddlog_std::Either::Left{l: _}}} => Some(((*_0).clone()).into_ddvalue()),
                                    _ => None
                                }.map(|x|(x,__cloned))
                            }
                            __f},
                            queryable: false
                        },
                        Arrangement::Map{
                           name: r###"(Arrow{.expr_id=(_0: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Right{.r=(_: StmtId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow) /*join*/"###.to_string(),
                            afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                            {
                                let __cloned = __v.clone();
                                match unsafe {< ::types::Arrow>::from_ddvalue(__v) } {
                                    ::types::Arrow{expr_id: ref _0, body: ::types::ddlog_std::Option::Some{x: ::types::ddlog_std::Either::Right{r: _}}} => Some(((*_0).clone()).into_ddvalue()),
                                    _ => None
                                }.map(|x|(x,__cloned))
                            }
                            __f},
                            queryable: false
                        }],
                    change_cb:    None
                };
    let INPUT_Arrow = Relation {
                          name:         "INPUT_Arrow".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_Arrow as RelId,
                          rules:        vec![
                              /* INPUT_Arrow[x] :- Arrow[(x: Arrow)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_Arrow[x] :- Arrow[(x: Arrow)].".to_string(),
                                  rel: Relations::Arrow as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_Arrow[x] :- Arrow[(x: Arrow)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::Arrow>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let ArrowParam = Relation {
                         name:         "ArrowParam".to_string(),
                         input:        true,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::ArrowParam as RelId,
                         rules:        vec![
                             ],
                         arrangements: vec![
                             ],
                         change_cb:    None
                     };
    let INPUT_ArrowParam = Relation {
                               name:         "INPUT_ArrowParam".to_string(),
                               input:        false,
                               distinct:     false,
                               caching_mode: CachingMode::Set,
                               key_func:     None,
                               id:           Relations::INPUT_ArrowParam as RelId,
                               rules:        vec![
                                   /* INPUT_ArrowParam[x] :- ArrowParam[(x: ArrowParam)]. */
                                   Rule::CollectionRule {
                                       description: "INPUT_ArrowParam[x] :- ArrowParam[(x: ArrowParam)].".to_string(),
                                       rel: Relations::ArrowParam as RelId,
                                       xform: Some(XFormCollection::FilterMap{
                                                       description: "head of INPUT_ArrowParam[x] :- ArrowParam[(x: ArrowParam)]." .to_string(),
                                                       fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                       {
                                                           let ref x = match *unsafe {<::types::ArrowParam>::from_ddvalue_ref(&__v) } {
                                                               ref x => (*x).clone(),
                                                               _ => return None
                                                           };
                                                           Some(((*x).clone()).into_ddvalue())
                                                       }
                                                       __f},
                                                       next: Box::new(None)
                                                   })
                                   }],
                               arrangements: vec![
                                   ],
                               change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                           };
    let __Prefix_0 = Relation {
                         name:         "__Prefix_0".to_string(),
                         input:        false,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::__Prefix_0 as RelId,
                         rules:        vec![
                             /* __Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))] :- ArrowParam[(ArrowParam{.expr_id=(expr: ExprId), .param=(pat: internment::Intern<Pattern>)}: ArrowParam)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))). */
                             Rule::CollectionRule {
                                 description: "__Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))] :- ArrowParam[(ArrowParam{.expr_id=(expr: ExprId), .param=(pat: internment::Intern<Pattern>)}: ArrowParam)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))).".to_string(),
                                 rel: Relations::ArrowParam as RelId,
                                 xform: Some(XFormCollection::FilterMap{
                                                 description: "head of __Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))] :- ArrowParam[(ArrowParam{.expr_id=(expr: ExprId), .param=(pat: internment::Intern<Pattern>)}: ArrowParam)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat)))." .to_string(),
                                                 fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                 {
                                                     let (ref expr, ref pat) = match *unsafe {<::types::ArrowParam>::from_ddvalue_ref(&__v) } {
                                                         ::types::ArrowParam{expr_id: ref expr, param: ref pat} => ((*expr).clone(), (*pat).clone()),
                                                         _ => return None
                                                     };
                                                     let ref name: ::types::internment::Intern<String> = match (*::types::internment::ival(pat)).clone() {
                                                         ::types::Pattern{name: name} => name,
                                                         _ => return None
                                                     };
                                                     Some((::types::ddlog_std::tuple3((*expr).clone(), (*pat).clone(), (*name).clone())).into_ddvalue())
                                                 }
                                                 __f},
                                                 next: Box::new(None)
                                             })
                             }],
                         arrangements: vec![
                             Arrangement::Map{
                                name: r###"((_0: ExprId), (_: internment::Intern<Pattern>), (_: internment::Intern<string>)) /*join*/"###.to_string(),
                                 afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                 {
                                     let __cloned = __v.clone();
                                     match unsafe {< ::types::ddlog_std::tuple3<::types::ExprId, ::types::internment::Intern<::types::Pattern>, ::types::internment::Intern<String>>>::from_ddvalue(__v) } {
                                         ::types::ddlog_std::tuple3(ref _0, _, _) => Some(((*_0).clone()).into_ddvalue()),
                                         _ => None
                                     }.map(|x|(x,__cloned))
                                 }
                                 __f},
                                 queryable: false
                             }],
                         change_cb:    None
                     };
    let Await = Relation {
                    name:         "Await".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::Await as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_Await = Relation {
                          name:         "INPUT_Await".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_Await as RelId,
                          rules:        vec![
                              /* INPUT_Await[x] :- Await[(x: Await)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_Await[x] :- Await[(x: Await)].".to_string(),
                                  rel: Relations::Await as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_Await[x] :- Await[(x: Await)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::Await>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let BinOp = Relation {
                    name:         "BinOp".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::BinOp as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_BinOp = Relation {
                          name:         "INPUT_BinOp".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_BinOp as RelId,
                          rules:        vec![
                              /* INPUT_BinOp[x] :- BinOp[(x: BinOp)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_BinOp[x] :- BinOp[(x: BinOp)].".to_string(),
                                  rel: Relations::BinOp as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_BinOp[x] :- BinOp[(x: BinOp)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::BinOp>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let BracketAccess = Relation {
                            name:         "BracketAccess".to_string(),
                            input:        true,
                            distinct:     false,
                            caching_mode: CachingMode::Set,
                            key_func:     None,
                            id:           Relations::BracketAccess as RelId,
                            rules:        vec![
                                ],
                            arrangements: vec![
                                ],
                            change_cb:    None
                        };
    let INPUT_BracketAccess = Relation {
                                  name:         "INPUT_BracketAccess".to_string(),
                                  input:        false,
                                  distinct:     false,
                                  caching_mode: CachingMode::Set,
                                  key_func:     None,
                                  id:           Relations::INPUT_BracketAccess as RelId,
                                  rules:        vec![
                                      /* INPUT_BracketAccess[x] :- BracketAccess[(x: BracketAccess)]. */
                                      Rule::CollectionRule {
                                          description: "INPUT_BracketAccess[x] :- BracketAccess[(x: BracketAccess)].".to_string(),
                                          rel: Relations::BracketAccess as RelId,
                                          xform: Some(XFormCollection::FilterMap{
                                                          description: "head of INPUT_BracketAccess[x] :- BracketAccess[(x: BracketAccess)]." .to_string(),
                                                          fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                          {
                                                              let ref x = match *unsafe {<::types::BracketAccess>::from_ddvalue_ref(&__v) } {
                                                                  ref x => (*x).clone(),
                                                                  _ => return None
                                                              };
                                                              Some(((*x).clone()).into_ddvalue())
                                                          }
                                                          __f},
                                                          next: Box::new(None)
                                                      })
                                      }],
                                  arrangements: vec![
                                      ],
                                  change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                              };
    let Break = Relation {
                    name:         "Break".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::Break as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_Break = Relation {
                          name:         "INPUT_Break".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_Break as RelId,
                          rules:        vec![
                              /* INPUT_Break[x] :- Break[(x: Break)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_Break[x] :- Break[(x: Break)].".to_string(),
                                  rel: Relations::Break as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_Break[x] :- Break[(x: Break)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::Break>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let ConstDecl = Relation {
                        name:         "ConstDecl".to_string(),
                        input:        true,
                        distinct:     false,
                        caching_mode: CachingMode::Set,
                        key_func:     None,
                        id:           Relations::ConstDecl as RelId,
                        rules:        vec![
                            ],
                        arrangements: vec![
                            ],
                        change_cb:    None
                    };
    let INPUT_ConstDecl = Relation {
                              name:         "INPUT_ConstDecl".to_string(),
                              input:        false,
                              distinct:     false,
                              caching_mode: CachingMode::Set,
                              key_func:     None,
                              id:           Relations::INPUT_ConstDecl as RelId,
                              rules:        vec![
                                  /* INPUT_ConstDecl[x] :- ConstDecl[(x: ConstDecl)]. */
                                  Rule::CollectionRule {
                                      description: "INPUT_ConstDecl[x] :- ConstDecl[(x: ConstDecl)].".to_string(),
                                      rel: Relations::ConstDecl as RelId,
                                      xform: Some(XFormCollection::FilterMap{
                                                      description: "head of INPUT_ConstDecl[x] :- ConstDecl[(x: ConstDecl)]." .to_string(),
                                                      fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                      {
                                                          let ref x = match *unsafe {<::types::ConstDecl>::from_ddvalue_ref(&__v) } {
                                                              ref x => (*x).clone(),
                                                              _ => return None
                                                          };
                                                          Some(((*x).clone()).into_ddvalue())
                                                      }
                                                      __f},
                                                      next: Box::new(None)
                                                  })
                                  }],
                              arrangements: vec![
                                  ],
                              change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                          };
    let Continue = Relation {
                       name:         "Continue".to_string(),
                       input:        true,
                       distinct:     false,
                       caching_mode: CachingMode::Set,
                       key_func:     None,
                       id:           Relations::Continue as RelId,
                       rules:        vec![
                           ],
                       arrangements: vec![
                           ],
                       change_cb:    None
                   };
    let INPUT_Continue = Relation {
                             name:         "INPUT_Continue".to_string(),
                             input:        false,
                             distinct:     false,
                             caching_mode: CachingMode::Set,
                             key_func:     None,
                             id:           Relations::INPUT_Continue as RelId,
                             rules:        vec![
                                 /* INPUT_Continue[x] :- Continue[(x: Continue)]. */
                                 Rule::CollectionRule {
                                     description: "INPUT_Continue[x] :- Continue[(x: Continue)].".to_string(),
                                     rel: Relations::Continue as RelId,
                                     xform: Some(XFormCollection::FilterMap{
                                                     description: "head of INPUT_Continue[x] :- Continue[(x: Continue)]." .to_string(),
                                                     fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                     {
                                                         let ref x = match *unsafe {<::types::Continue>::from_ddvalue_ref(&__v) } {
                                                             ref x => (*x).clone(),
                                                             _ => return None
                                                         };
                                                         Some(((*x).clone()).into_ddvalue())
                                                     }
                                                     __f},
                                                     next: Box::new(None)
                                                 })
                                 }],
                             arrangements: vec![
                                 ],
                             change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                         };
    let DoWhile = Relation {
                      name:         "DoWhile".to_string(),
                      input:        true,
                      distinct:     false,
                      caching_mode: CachingMode::Set,
                      key_func:     None,
                      id:           Relations::DoWhile as RelId,
                      rules:        vec![
                          ],
                      arrangements: vec![
                          ],
                      change_cb:    None
                  };
    let INPUT_DoWhile = Relation {
                            name:         "INPUT_DoWhile".to_string(),
                            input:        false,
                            distinct:     false,
                            caching_mode: CachingMode::Set,
                            key_func:     None,
                            id:           Relations::INPUT_DoWhile as RelId,
                            rules:        vec![
                                /* INPUT_DoWhile[x] :- DoWhile[(x: DoWhile)]. */
                                Rule::CollectionRule {
                                    description: "INPUT_DoWhile[x] :- DoWhile[(x: DoWhile)].".to_string(),
                                    rel: Relations::DoWhile as RelId,
                                    xform: Some(XFormCollection::FilterMap{
                                                    description: "head of INPUT_DoWhile[x] :- DoWhile[(x: DoWhile)]." .to_string(),
                                                    fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                    {
                                                        let ref x = match *unsafe {<::types::DoWhile>::from_ddvalue_ref(&__v) } {
                                                            ref x => (*x).clone(),
                                                            _ => return None
                                                        };
                                                        Some(((*x).clone()).into_ddvalue())
                                                    }
                                                    __f},
                                                    next: Box::new(None)
                                                })
                                }],
                            arrangements: vec![
                                ],
                            change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                        };
    let DotAccess = Relation {
                        name:         "DotAccess".to_string(),
                        input:        true,
                        distinct:     false,
                        caching_mode: CachingMode::Set,
                        key_func:     None,
                        id:           Relations::DotAccess as RelId,
                        rules:        vec![
                            ],
                        arrangements: vec![
                            ],
                        change_cb:    None
                    };
    let INPUT_DotAccess = Relation {
                              name:         "INPUT_DotAccess".to_string(),
                              input:        false,
                              distinct:     false,
                              caching_mode: CachingMode::Set,
                              key_func:     None,
                              id:           Relations::INPUT_DotAccess as RelId,
                              rules:        vec![
                                  /* INPUT_DotAccess[x] :- DotAccess[(x: DotAccess)]. */
                                  Rule::CollectionRule {
                                      description: "INPUT_DotAccess[x] :- DotAccess[(x: DotAccess)].".to_string(),
                                      rel: Relations::DotAccess as RelId,
                                      xform: Some(XFormCollection::FilterMap{
                                                      description: "head of INPUT_DotAccess[x] :- DotAccess[(x: DotAccess)]." .to_string(),
                                                      fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                      {
                                                          let ref x = match *unsafe {<::types::DotAccess>::from_ddvalue_ref(&__v) } {
                                                              ref x => (*x).clone(),
                                                              _ => return None
                                                          };
                                                          Some(((*x).clone()).into_ddvalue())
                                                      }
                                                      __f},
                                                      next: Box::new(None)
                                                  })
                                  }],
                              arrangements: vec![
                                  ],
                              change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                          };
    let EveryScope = Relation {
                         name:         "EveryScope".to_string(),
                         input:        true,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::EveryScope as RelId,
                         rules:        vec![
                             ],
                         arrangements: vec![
                             Arrangement::Map{
                                name: r###"(EveryScope{.scope=(_: Scope)}: EveryScope) /*join*/"###.to_string(),
                                 afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                 {
                                     let __cloned = __v.clone();
                                     match unsafe {< ::types::EveryScope>::from_ddvalue(__v) } {
                                         ::types::EveryScope{scope: _} => Some((()).into_ddvalue()),
                                         _ => None
                                     }.map(|x|(x,__cloned))
                                 }
                                 __f},
                                 queryable: false
                             }],
                         change_cb:    None
                     };
    let INPUT_EveryScope = Relation {
                               name:         "INPUT_EveryScope".to_string(),
                               input:        false,
                               distinct:     false,
                               caching_mode: CachingMode::Set,
                               key_func:     None,
                               id:           Relations::INPUT_EveryScope as RelId,
                               rules:        vec![
                                   /* INPUT_EveryScope[x] :- EveryScope[(x: EveryScope)]. */
                                   Rule::CollectionRule {
                                       description: "INPUT_EveryScope[x] :- EveryScope[(x: EveryScope)].".to_string(),
                                       rel: Relations::EveryScope as RelId,
                                       xform: Some(XFormCollection::FilterMap{
                                                       description: "head of INPUT_EveryScope[x] :- EveryScope[(x: EveryScope)]." .to_string(),
                                                       fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                       {
                                                           let ref x = match *unsafe {<::types::EveryScope>::from_ddvalue_ref(&__v) } {
                                                               ref x => (*x).clone(),
                                                               _ => return None
                                                           };
                                                           Some(((*x).clone()).into_ddvalue())
                                                       }
                                                       __f},
                                                       next: Box::new(None)
                                                   })
                                   }],
                               arrangements: vec![
                                   ],
                               change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                           };
    let ExprBigInt = Relation {
                         name:         "ExprBigInt".to_string(),
                         input:        true,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::ExprBigInt as RelId,
                         rules:        vec![
                             ],
                         arrangements: vec![
                             ],
                         change_cb:    None
                     };
    let INPUT_ExprBigInt = Relation {
                               name:         "INPUT_ExprBigInt".to_string(),
                               input:        false,
                               distinct:     false,
                               caching_mode: CachingMode::Set,
                               key_func:     None,
                               id:           Relations::INPUT_ExprBigInt as RelId,
                               rules:        vec![
                                   /* INPUT_ExprBigInt[x] :- ExprBigInt[(x: ExprBigInt)]. */
                                   Rule::CollectionRule {
                                       description: "INPUT_ExprBigInt[x] :- ExprBigInt[(x: ExprBigInt)].".to_string(),
                                       rel: Relations::ExprBigInt as RelId,
                                       xform: Some(XFormCollection::FilterMap{
                                                       description: "head of INPUT_ExprBigInt[x] :- ExprBigInt[(x: ExprBigInt)]." .to_string(),
                                                       fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                       {
                                                           let ref x = match *unsafe {<::types::ExprBigInt>::from_ddvalue_ref(&__v) } {
                                                               ref x => (*x).clone(),
                                                               _ => return None
                                                           };
                                                           Some(((*x).clone()).into_ddvalue())
                                                       }
                                                       __f},
                                                       next: Box::new(None)
                                                   })
                                   }],
                               arrangements: vec![
                                   ],
                               change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                           };
    let ExprBool = Relation {
                       name:         "ExprBool".to_string(),
                       input:        true,
                       distinct:     false,
                       caching_mode: CachingMode::Set,
                       key_func:     None,
                       id:           Relations::ExprBool as RelId,
                       rules:        vec![
                           ],
                       arrangements: vec![
                           ],
                       change_cb:    None
                   };
    let INPUT_ExprBool = Relation {
                             name:         "INPUT_ExprBool".to_string(),
                             input:        false,
                             distinct:     false,
                             caching_mode: CachingMode::Set,
                             key_func:     None,
                             id:           Relations::INPUT_ExprBool as RelId,
                             rules:        vec![
                                 /* INPUT_ExprBool[x] :- ExprBool[(x: ExprBool)]. */
                                 Rule::CollectionRule {
                                     description: "INPUT_ExprBool[x] :- ExprBool[(x: ExprBool)].".to_string(),
                                     rel: Relations::ExprBool as RelId,
                                     xform: Some(XFormCollection::FilterMap{
                                                     description: "head of INPUT_ExprBool[x] :- ExprBool[(x: ExprBool)]." .to_string(),
                                                     fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                     {
                                                         let ref x = match *unsafe {<::types::ExprBool>::from_ddvalue_ref(&__v) } {
                                                             ref x => (*x).clone(),
                                                             _ => return None
                                                         };
                                                         Some(((*x).clone()).into_ddvalue())
                                                     }
                                                     __f},
                                                     next: Box::new(None)
                                                 })
                                 }],
                             arrangements: vec![
                                 ],
                             change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                         };
    let ExprNumber = Relation {
                         name:         "ExprNumber".to_string(),
                         input:        true,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::ExprNumber as RelId,
                         rules:        vec![
                             ],
                         arrangements: vec![
                             ],
                         change_cb:    None
                     };
    let INPUT_ExprNumber = Relation {
                               name:         "INPUT_ExprNumber".to_string(),
                               input:        false,
                               distinct:     false,
                               caching_mode: CachingMode::Set,
                               key_func:     None,
                               id:           Relations::INPUT_ExprNumber as RelId,
                               rules:        vec![
                                   /* INPUT_ExprNumber[x] :- ExprNumber[(x: ExprNumber)]. */
                                   Rule::CollectionRule {
                                       description: "INPUT_ExprNumber[x] :- ExprNumber[(x: ExprNumber)].".to_string(),
                                       rel: Relations::ExprNumber as RelId,
                                       xform: Some(XFormCollection::FilterMap{
                                                       description: "head of INPUT_ExprNumber[x] :- ExprNumber[(x: ExprNumber)]." .to_string(),
                                                       fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                       {
                                                           let ref x = match *unsafe {<::types::ExprNumber>::from_ddvalue_ref(&__v) } {
                                                               ref x => (*x).clone(),
                                                               _ => return None
                                                           };
                                                           Some(((*x).clone()).into_ddvalue())
                                                       }
                                                       __f},
                                                       next: Box::new(None)
                                                   })
                                   }],
                               arrangements: vec![
                                   ],
                               change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                           };
    let ExprString = Relation {
                         name:         "ExprString".to_string(),
                         input:        true,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::ExprString as RelId,
                         rules:        vec![
                             ],
                         arrangements: vec![
                             ],
                         change_cb:    None
                     };
    let INPUT_ExprString = Relation {
                               name:         "INPUT_ExprString".to_string(),
                               input:        false,
                               distinct:     false,
                               caching_mode: CachingMode::Set,
                               key_func:     None,
                               id:           Relations::INPUT_ExprString as RelId,
                               rules:        vec![
                                   /* INPUT_ExprString[x] :- ExprString[(x: ExprString)]. */
                                   Rule::CollectionRule {
                                       description: "INPUT_ExprString[x] :- ExprString[(x: ExprString)].".to_string(),
                                       rel: Relations::ExprString as RelId,
                                       xform: Some(XFormCollection::FilterMap{
                                                       description: "head of INPUT_ExprString[x] :- ExprString[(x: ExprString)]." .to_string(),
                                                       fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                       {
                                                           let ref x = match *unsafe {<::types::ExprString>::from_ddvalue_ref(&__v) } {
                                                               ref x => (*x).clone(),
                                                               _ => return None
                                                           };
                                                           Some(((*x).clone()).into_ddvalue())
                                                       }
                                                       __f},
                                                       next: Box::new(None)
                                                   })
                                   }],
                               arrangements: vec![
                                   ],
                               change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                           };
    let Expression = Relation {
                         name:         "Expression".to_string(),
                         input:        true,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::Expression as RelId,
                         rules:        vec![
                             ],
                         arrangements: vec![
                             Arrangement::Map{
                                name: r###"(Expression{.id=(_0: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(_: Scope), .span=(_: Span)}: Expression) /*join*/"###.to_string(),
                                 afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                 {
                                     let __cloned = __v.clone();
                                     match unsafe {< ::types::Expression>::from_ddvalue(__v) } {
                                         ::types::Expression{id: ref _0, kind: ::types::ExprKind::ExprNameRef{}, scope: _, span: _} => Some(((*_0).clone()).into_ddvalue()),
                                         _ => None
                                     }.map(|x|(x,__cloned))
                                 }
                                 __f},
                                 queryable: false
                             },
                             Arrangement::Map{
                                name: r###"(Expression{.id=(_0: ExprId), .kind=(_: ExprKind), .scope=(_: Scope), .span=(_: Span)}: Expression) /*join*/"###.to_string(),
                                 afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                 {
                                     let __cloned = __v.clone();
                                     match unsafe {< ::types::Expression>::from_ddvalue(__v) } {
                                         ::types::Expression{id: ref _0, kind: _, scope: _, span: _} => Some(((*_0).clone()).into_ddvalue()),
                                         _ => None
                                     }.map(|x|(x,__cloned))
                                 }
                                 __f},
                                 queryable: false
                             }],
                         change_cb:    None
                     };
    let INPUT_Expression = Relation {
                               name:         "INPUT_Expression".to_string(),
                               input:        false,
                               distinct:     false,
                               caching_mode: CachingMode::Set,
                               key_func:     None,
                               id:           Relations::INPUT_Expression as RelId,
                               rules:        vec![
                                   /* INPUT_Expression[x] :- Expression[(x: Expression)]. */
                                   Rule::CollectionRule {
                                       description: "INPUT_Expression[x] :- Expression[(x: Expression)].".to_string(),
                                       rel: Relations::Expression as RelId,
                                       xform: Some(XFormCollection::FilterMap{
                                                       description: "head of INPUT_Expression[x] :- Expression[(x: Expression)]." .to_string(),
                                                       fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                       {
                                                           let ref x = match *unsafe {<::types::Expression>::from_ddvalue_ref(&__v) } {
                                                               ref x => (*x).clone(),
                                                               _ => return None
                                                           };
                                                           Some(((*x).clone()).into_ddvalue())
                                                       }
                                                       __f},
                                                       next: Box::new(None)
                                                   })
                                   }],
                               arrangements: vec![
                                   ],
                               change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                           };
    let For = Relation {
                  name:         "For".to_string(),
                  input:        true,
                  distinct:     false,
                  caching_mode: CachingMode::Set,
                  key_func:     None,
                  id:           Relations::For as RelId,
                  rules:        vec![
                      ],
                  arrangements: vec![
                      ],
                  change_cb:    None
              };
    let INPUT_For = Relation {
                        name:         "INPUT_For".to_string(),
                        input:        false,
                        distinct:     false,
                        caching_mode: CachingMode::Set,
                        key_func:     None,
                        id:           Relations::INPUT_For as RelId,
                        rules:        vec![
                            /* INPUT_For[x] :- For[(x: For)]. */
                            Rule::CollectionRule {
                                description: "INPUT_For[x] :- For[(x: For)].".to_string(),
                                rel: Relations::For as RelId,
                                xform: Some(XFormCollection::FilterMap{
                                                description: "head of INPUT_For[x] :- For[(x: For)]." .to_string(),
                                                fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                {
                                                    let ref x = match *unsafe {<::types::For>::from_ddvalue_ref(&__v) } {
                                                        ref x => (*x).clone(),
                                                        _ => return None
                                                    };
                                                    Some(((*x).clone()).into_ddvalue())
                                                }
                                                __f},
                                                next: Box::new(None)
                                            })
                            }],
                        arrangements: vec![
                            ],
                        change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                    };
    let ForIn = Relation {
                    name:         "ForIn".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::ForIn as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_ForIn = Relation {
                          name:         "INPUT_ForIn".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_ForIn as RelId,
                          rules:        vec![
                              /* INPUT_ForIn[x] :- ForIn[(x: ForIn)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_ForIn[x] :- ForIn[(x: ForIn)].".to_string(),
                                  rel: Relations::ForIn as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_ForIn[x] :- ForIn[(x: ForIn)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::ForIn>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let Function = Relation {
                       name:         "Function".to_string(),
                       input:        true,
                       distinct:     false,
                       caching_mode: CachingMode::Set,
                       key_func:     None,
                       id:           Relations::Function as RelId,
                       rules:        vec![
                           ],
                       arrangements: vec![
                           Arrangement::Map{
                              name: r###"(Function{.id=(_: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(_0: Scope)}: Function) /*join*/"###.to_string(),
                               afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                               {
                                   let __cloned = __v.clone();
                                   match unsafe {< ::types::Function>::from_ddvalue(__v) } {
                                       ::types::Function{id: _, name: _, scope: ref _0} => Some(((*_0).clone()).into_ddvalue()),
                                       _ => None
                                   }.map(|x|(x,__cloned))
                               }
                               __f},
                               queryable: false
                           },
                           Arrangement::Map{
                              name: r###"(Function{.id=(_0: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(_: Scope)}: Function) /*join*/"###.to_string(),
                               afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                               {
                                   let __cloned = __v.clone();
                                   match unsafe {< ::types::Function>::from_ddvalue(__v) } {
                                       ::types::Function{id: ref _0, name: _, scope: _} => Some(((*_0).clone()).into_ddvalue()),
                                       _ => None
                                   }.map(|x|(x,__cloned))
                               }
                               __f},
                               queryable: false
                           }],
                       change_cb:    None
                   };
    let INPUT_Function = Relation {
                             name:         "INPUT_Function".to_string(),
                             input:        false,
                             distinct:     false,
                             caching_mode: CachingMode::Set,
                             key_func:     None,
                             id:           Relations::INPUT_Function as RelId,
                             rules:        vec![
                                 /* INPUT_Function[x] :- Function[(x: Function)]. */
                                 Rule::CollectionRule {
                                     description: "INPUT_Function[x] :- Function[(x: Function)].".to_string(),
                                     rel: Relations::Function as RelId,
                                     xform: Some(XFormCollection::FilterMap{
                                                     description: "head of INPUT_Function[x] :- Function[(x: Function)]." .to_string(),
                                                     fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                     {
                                                         let ref x = match *unsafe {<::types::Function>::from_ddvalue_ref(&__v) } {
                                                             ref x => (*x).clone(),
                                                             _ => return None
                                                         };
                                                         Some(((*x).clone()).into_ddvalue())
                                                     }
                                                     __f},
                                                     next: Box::new(None)
                                                 })
                                 }],
                             arrangements: vec![
                                 ],
                             change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                         };
    let FunctionArg = Relation {
                          name:         "FunctionArg".to_string(),
                          input:        true,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::FunctionArg as RelId,
                          rules:        vec![
                              ],
                          arrangements: vec![
                              ],
                          change_cb:    None
                      };
    let INPUT_FunctionArg = Relation {
                                name:         "INPUT_FunctionArg".to_string(),
                                input:        false,
                                distinct:     false,
                                caching_mode: CachingMode::Set,
                                key_func:     None,
                                id:           Relations::INPUT_FunctionArg as RelId,
                                rules:        vec![
                                    /* INPUT_FunctionArg[x] :- FunctionArg[(x: FunctionArg)]. */
                                    Rule::CollectionRule {
                                        description: "INPUT_FunctionArg[x] :- FunctionArg[(x: FunctionArg)].".to_string(),
                                        rel: Relations::FunctionArg as RelId,
                                        xform: Some(XFormCollection::FilterMap{
                                                        description: "head of INPUT_FunctionArg[x] :- FunctionArg[(x: FunctionArg)]." .to_string(),
                                                        fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                        {
                                                            let ref x = match *unsafe {<::types::FunctionArg>::from_ddvalue_ref(&__v) } {
                                                                ref x => (*x).clone(),
                                                                _ => return None
                                                            };
                                                            Some(((*x).clone()).into_ddvalue())
                                                        }
                                                        __f},
                                                        next: Box::new(None)
                                                    })
                                    }],
                                arrangements: vec![
                                    ],
                                change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                            };
    let If = Relation {
                 name:         "If".to_string(),
                 input:        true,
                 distinct:     false,
                 caching_mode: CachingMode::Set,
                 key_func:     None,
                 id:           Relations::If as RelId,
                 rules:        vec![
                     ],
                 arrangements: vec![
                     ],
                 change_cb:    None
             };
    let INPUT_If = Relation {
                       name:         "INPUT_If".to_string(),
                       input:        false,
                       distinct:     false,
                       caching_mode: CachingMode::Set,
                       key_func:     None,
                       id:           Relations::INPUT_If as RelId,
                       rules:        vec![
                           /* INPUT_If[x] :- If[(x: If)]. */
                           Rule::CollectionRule {
                               description: "INPUT_If[x] :- If[(x: If)].".to_string(),
                               rel: Relations::If as RelId,
                               xform: Some(XFormCollection::FilterMap{
                                               description: "head of INPUT_If[x] :- If[(x: If)]." .to_string(),
                                               fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                               {
                                                   let ref x = match *unsafe {<::types::If>::from_ddvalue_ref(&__v) } {
                                                       ref x => (*x).clone(),
                                                       _ => return None
                                                   };
                                                   Some(((*x).clone()).into_ddvalue())
                                               }
                                               __f},
                                               next: Box::new(None)
                                           })
                           }],
                       arrangements: vec![
                           ],
                       change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                   };
    let ImplicitGlobal = Relation {
                             name:         "ImplicitGlobal".to_string(),
                             input:        true,
                             distinct:     false,
                             caching_mode: CachingMode::Set,
                             key_func:     None,
                             id:           Relations::ImplicitGlobal as RelId,
                             rules:        vec![
                                 ],
                             arrangements: vec![
                                 Arrangement::Map{
                                    name: r###"(ImplicitGlobal{.id=(_: GlobalId), .name=(_: internment::Intern<string>)}: ImplicitGlobal) /*join*/"###.to_string(),
                                     afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                     {
                                         let __cloned = __v.clone();
                                         match unsafe {< ::types::ImplicitGlobal>::from_ddvalue(__v) } {
                                             ::types::ImplicitGlobal{id: _, name: _} => Some((()).into_ddvalue()),
                                             _ => None
                                         }.map(|x|(x,__cloned))
                                     }
                                     __f},
                                     queryable: false
                                 }],
                             change_cb:    None
                         };
    let INPUT_ImplicitGlobal = Relation {
                                   name:         "INPUT_ImplicitGlobal".to_string(),
                                   input:        false,
                                   distinct:     false,
                                   caching_mode: CachingMode::Set,
                                   key_func:     None,
                                   id:           Relations::INPUT_ImplicitGlobal as RelId,
                                   rules:        vec![
                                       /* INPUT_ImplicitGlobal[x] :- ImplicitGlobal[(x: ImplicitGlobal)]. */
                                       Rule::CollectionRule {
                                           description: "INPUT_ImplicitGlobal[x] :- ImplicitGlobal[(x: ImplicitGlobal)].".to_string(),
                                           rel: Relations::ImplicitGlobal as RelId,
                                           xform: Some(XFormCollection::FilterMap{
                                                           description: "head of INPUT_ImplicitGlobal[x] :- ImplicitGlobal[(x: ImplicitGlobal)]." .to_string(),
                                                           fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                           {
                                                               let ref x = match *unsafe {<::types::ImplicitGlobal>::from_ddvalue_ref(&__v) } {
                                                                   ref x => (*x).clone(),
                                                                   _ => return None
                                                               };
                                                               Some(((*x).clone()).into_ddvalue())
                                                           }
                                                           __f},
                                                           next: Box::new(None)
                                                       })
                                       }],
                                   arrangements: vec![
                                       ],
                                   change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                               };
    let InputScope = Relation {
                         name:         "InputScope".to_string(),
                         input:        true,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::InputScope as RelId,
                         rules:        vec![
                             ],
                         arrangements: vec![
                             Arrangement::Map{
                                name: r###"(InputScope{.parent=(_: Scope), .child=(_0: Scope)}: InputScope) /*join*/"###.to_string(),
                                 afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                 {
                                     let __cloned = __v.clone();
                                     match unsafe {< ::types::InputScope>::from_ddvalue(__v) } {
                                         ::types::InputScope{parent: _, child: ref _0} => Some(((*_0).clone()).into_ddvalue()),
                                         _ => None
                                     }.map(|x|(x,__cloned))
                                 }
                                 __f},
                                 queryable: false
                             },
                             Arrangement::Map{
                                name: r###"(InputScope{.parent=(_0: Scope), .child=(_: Scope)}: InputScope) /*join*/"###.to_string(),
                                 afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                 {
                                     let __cloned = __v.clone();
                                     match unsafe {< ::types::InputScope>::from_ddvalue(__v) } {
                                         ::types::InputScope{parent: ref _0, child: _} => Some(((*_0).clone()).into_ddvalue()),
                                         _ => None
                                     }.map(|x|(x,__cloned))
                                 }
                                 __f},
                                 queryable: false
                             }],
                         change_cb:    None
                     };
    let ChildScope = Relation {
                         name:         "ChildScope".to_string(),
                         input:        false,
                         distinct:     true,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::ChildScope as RelId,
                         rules:        vec![
                             /* ChildScope[(ChildScope{.parent=parent, .child=child}: ChildScope)] :- InputScope[(InputScope{.parent=(parent: Scope), .child=(child: Scope)}: InputScope)]. */
                             Rule::CollectionRule {
                                 description: "ChildScope[(ChildScope{.parent=parent, .child=child}: ChildScope)] :- InputScope[(InputScope{.parent=(parent: Scope), .child=(child: Scope)}: InputScope)].".to_string(),
                                 rel: Relations::InputScope as RelId,
                                 xform: Some(XFormCollection::FilterMap{
                                                 description: "head of ChildScope[(ChildScope{.parent=parent, .child=child}: ChildScope)] :- InputScope[(InputScope{.parent=(parent: Scope), .child=(child: Scope)}: InputScope)]." .to_string(),
                                                 fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                 {
                                                     let (ref parent, ref child) = match *unsafe {<::types::InputScope>::from_ddvalue_ref(&__v) } {
                                                         ::types::InputScope{parent: ref parent, child: ref child} => ((*parent).clone(), (*child).clone()),
                                                         _ => return None
                                                     };
                                                     Some(((::types::ChildScope{parent: (*parent).clone(), child: (*child).clone()})).into_ddvalue())
                                                 }
                                                 __f},
                                                 next: Box::new(None)
                                             })
                             },
                             /* ChildScope[(ChildScope{.parent=parent, .child=child}: ChildScope)] :- InputScope[(InputScope{.parent=(parent: Scope), .child=(interum: Scope)}: InputScope)], InputScope[(InputScope{.parent=(interum: Scope), .child=(child: Scope)}: InputScope)]. */
                             Rule::ArrangementRule {
                                 description: "ChildScope[(ChildScope{.parent=parent, .child=child}: ChildScope)] :- InputScope[(InputScope{.parent=(parent: Scope), .child=(interum: Scope)}: InputScope)], InputScope[(InputScope{.parent=(interum: Scope), .child=(child: Scope)}: InputScope)].".to_string(),
                                 arr: ( Relations::InputScope as RelId, 0),
                                 xform: XFormArrangement::Join{
                                            description: "InputScope[(InputScope{.parent=(parent: Scope), .child=(interum: Scope)}: InputScope)], InputScope[(InputScope{.parent=(interum: Scope), .child=(child: Scope)}: InputScope)]".to_string(),
                                            ffun: None,
                                            arrangement: (Relations::InputScope as RelId,1),
                                            jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                            {
                                                let (ref parent, ref interum) = match *unsafe {<::types::InputScope>::from_ddvalue_ref(__v1) } {
                                                    ::types::InputScope{parent: ref parent, child: ref interum} => ((*parent).clone(), (*interum).clone()),
                                                    _ => return None
                                                };
                                                let ref child = match *unsafe {<::types::InputScope>::from_ddvalue_ref(__v2) } {
                                                    ::types::InputScope{parent: _, child: ref child} => (*child).clone(),
                                                    _ => return None
                                                };
                                                Some(((::types::ChildScope{parent: (*parent).clone(), child: (*child).clone()})).into_ddvalue())
                                            }
                                            __f},
                                            next: Box::new(None)
                                        }
                             }],
                         arrangements: vec![
                             Arrangement::Map{
                                name: r###"(ChildScope{.parent=(_0: Scope), .child=(_: Scope)}: ChildScope) /*join*/"###.to_string(),
                                 afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                 {
                                     let __cloned = __v.clone();
                                     match unsafe {< ::types::ChildScope>::from_ddvalue(__v) } {
                                         ::types::ChildScope{parent: ref _0, child: _} => Some(((*_0).clone()).into_ddvalue()),
                                         _ => None
                                     }.map(|x|(x,__cloned))
                                 }
                                 __f},
                                 queryable: false
                             },
                             Arrangement::Set{
                                 name: r###"(ChildScope{.parent=(_0: Scope), .child=(_1: Scope)}: ChildScope) /*semijoin*/"###.to_string(),
                                 fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                 {
                                     match unsafe {< ::types::ChildScope>::from_ddvalue(__v) } {
                                         ::types::ChildScope{parent: ref _0, child: ref _1} => Some((::types::ddlog_std::tuple2((*_0).clone(), (*_1).clone())).into_ddvalue()),
                                         _ => None
                                     }
                                 }
                                 __f},
                                 distinct: false
                             }],
                         change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                     };
    let ClosestFunction = Relation {
                              name:         "ClosestFunction".to_string(),
                              input:        false,
                              distinct:     true,
                              caching_mode: CachingMode::Set,
                              key_func:     None,
                              id:           Relations::ClosestFunction as RelId,
                              rules:        vec![
                                  /* ClosestFunction[(ClosestFunction{.scope=scope, .func=func}: ClosestFunction)] :- Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)]. */
                                  Rule::CollectionRule {
                                      description: "ClosestFunction[(ClosestFunction{.scope=scope, .func=func}: ClosestFunction)] :- Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)].".to_string(),
                                      rel: Relations::Function as RelId,
                                      xform: Some(XFormCollection::FilterMap{
                                                      description: "head of ClosestFunction[(ClosestFunction{.scope=scope, .func=func}: ClosestFunction)] :- Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)]." .to_string(),
                                                      fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                      {
                                                          let (ref func, ref scope) = match *unsafe {<::types::Function>::from_ddvalue_ref(&__v) } {
                                                              ::types::Function{id: ref func, name: _, scope: ref scope} => ((*func).clone(), (*scope).clone()),
                                                              _ => return None
                                                          };
                                                          Some(((::types::ClosestFunction{scope: (*scope).clone(), func: (*func).clone()})).into_ddvalue())
                                                      }
                                                      __f},
                                                      next: Box::new(None)
                                                  })
                                  },
                                  /* ClosestFunction[(ClosestFunction{.scope=scope, .func=func}: ClosestFunction)] :- Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(func_scope: Scope)}: Function)], ChildScope[(ChildScope{.parent=(func_scope: Scope), .child=(scope: Scope)}: ChildScope)]. */
                                  Rule::ArrangementRule {
                                      description: "ClosestFunction[(ClosestFunction{.scope=scope, .func=func}: ClosestFunction)] :- Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(func_scope: Scope)}: Function)], ChildScope[(ChildScope{.parent=(func_scope: Scope), .child=(scope: Scope)}: ChildScope)].".to_string(),
                                      arr: ( Relations::Function as RelId, 0),
                                      xform: XFormArrangement::Join{
                                                 description: "Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(func_scope: Scope)}: Function)], ChildScope[(ChildScope{.parent=(func_scope: Scope), .child=(scope: Scope)}: ChildScope)]".to_string(),
                                                 ffun: None,
                                                 arrangement: (Relations::ChildScope as RelId,0),
                                                 jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                 {
                                                     let (ref func, ref func_scope) = match *unsafe {<::types::Function>::from_ddvalue_ref(__v1) } {
                                                         ::types::Function{id: ref func, name: _, scope: ref func_scope} => ((*func).clone(), (*func_scope).clone()),
                                                         _ => return None
                                                     };
                                                     let ref scope = match *unsafe {<::types::ChildScope>::from_ddvalue_ref(__v2) } {
                                                         ::types::ChildScope{parent: _, child: ref scope} => (*scope).clone(),
                                                         _ => return None
                                                     };
                                                     Some(((::types::ClosestFunction{scope: (*scope).clone(), func: (*func).clone()})).into_ddvalue())
                                                 }
                                                 __f},
                                                 next: Box::new(None)
                                             }
                                  }],
                              arrangements: vec![
                                  Arrangement::Map{
                                     name: r###"(ClosestFunction{.scope=(_0: Scope), .func=(_: FuncId)}: ClosestFunction) /*join*/"###.to_string(),
                                      afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                      {
                                          let __cloned = __v.clone();
                                          match unsafe {< ::types::ClosestFunction>::from_ddvalue(__v) } {
                                              ::types::ClosestFunction{scope: ref _0, func: _} => Some(((*_0).clone()).into_ddvalue()),
                                              _ => None
                                          }.map(|x|(x,__cloned))
                                      }
                                      __f},
                                      queryable: false
                                  }],
                              change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                          };
    let INPUT_InputScope = Relation {
                               name:         "INPUT_InputScope".to_string(),
                               input:        false,
                               distinct:     false,
                               caching_mode: CachingMode::Set,
                               key_func:     None,
                               id:           Relations::INPUT_InputScope as RelId,
                               rules:        vec![
                                   /* INPUT_InputScope[x] :- InputScope[(x: InputScope)]. */
                                   Rule::CollectionRule {
                                       description: "INPUT_InputScope[x] :- InputScope[(x: InputScope)].".to_string(),
                                       rel: Relations::InputScope as RelId,
                                       xform: Some(XFormCollection::FilterMap{
                                                       description: "head of INPUT_InputScope[x] :- InputScope[(x: InputScope)]." .to_string(),
                                                       fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                       {
                                                           let ref x = match *unsafe {<::types::InputScope>::from_ddvalue_ref(&__v) } {
                                                               ref x => (*x).clone(),
                                                               _ => return None
                                                           };
                                                           Some(((*x).clone()).into_ddvalue())
                                                       }
                                                       __f},
                                                       next: Box::new(None)
                                                   })
                                   }],
                               arrangements: vec![
                                   ],
                               change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                           };
    let Label = Relation {
                    name:         "Label".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::Label as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_Label = Relation {
                          name:         "INPUT_Label".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_Label as RelId,
                          rules:        vec![
                              /* INPUT_Label[x] :- Label[(x: Label)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_Label[x] :- Label[(x: Label)].".to_string(),
                                  rel: Relations::Label as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_Label[x] :- Label[(x: Label)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::Label>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let LetDecl = Relation {
                      name:         "LetDecl".to_string(),
                      input:        true,
                      distinct:     false,
                      caching_mode: CachingMode::Set,
                      key_func:     None,
                      id:           Relations::LetDecl as RelId,
                      rules:        vec![
                          ],
                      arrangements: vec![
                          ],
                      change_cb:    None
                  };
    let INPUT_LetDecl = Relation {
                            name:         "INPUT_LetDecl".to_string(),
                            input:        false,
                            distinct:     false,
                            caching_mode: CachingMode::Set,
                            key_func:     None,
                            id:           Relations::INPUT_LetDecl as RelId,
                            rules:        vec![
                                /* INPUT_LetDecl[x] :- LetDecl[(x: LetDecl)]. */
                                Rule::CollectionRule {
                                    description: "INPUT_LetDecl[x] :- LetDecl[(x: LetDecl)].".to_string(),
                                    rel: Relations::LetDecl as RelId,
                                    xform: Some(XFormCollection::FilterMap{
                                                    description: "head of INPUT_LetDecl[x] :- LetDecl[(x: LetDecl)]." .to_string(),
                                                    fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                    {
                                                        let ref x = match *unsafe {<::types::LetDecl>::from_ddvalue_ref(&__v) } {
                                                            ref x => (*x).clone(),
                                                            _ => return None
                                                        };
                                                        Some(((*x).clone()).into_ddvalue())
                                                    }
                                                    __f},
                                                    next: Box::new(None)
                                                })
                                }],
                            arrangements: vec![
                                ],
                            change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                        };
    let NameRef = Relation {
                      name:         "NameRef".to_string(),
                      input:        true,
                      distinct:     false,
                      caching_mode: CachingMode::Set,
                      key_func:     None,
                      id:           Relations::NameRef as RelId,
                      rules:        vec![
                          ],
                      arrangements: vec![
                          Arrangement::Map{
                             name: r###"(NameRef{.expr_id=(_0: ExprId), .value=(_: internment::Intern<string>)}: NameRef) /*join*/"###.to_string(),
                              afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                              {
                                  let __cloned = __v.clone();
                                  match unsafe {< ::types::NameRef>::from_ddvalue(__v) } {
                                      ::types::NameRef{expr_id: ref _0, value: _} => Some(((*_0).clone()).into_ddvalue()),
                                      _ => None
                                  }.map(|x|(x,__cloned))
                              }
                              __f},
                              queryable: false
                          }],
                      change_cb:    None
                  };
    let INPUT_NameRef = Relation {
                            name:         "INPUT_NameRef".to_string(),
                            input:        false,
                            distinct:     false,
                            caching_mode: CachingMode::Set,
                            key_func:     None,
                            id:           Relations::INPUT_NameRef as RelId,
                            rules:        vec![
                                /* INPUT_NameRef[x] :- NameRef[(x: NameRef)]. */
                                Rule::CollectionRule {
                                    description: "INPUT_NameRef[x] :- NameRef[(x: NameRef)].".to_string(),
                                    rel: Relations::NameRef as RelId,
                                    xform: Some(XFormCollection::FilterMap{
                                                    description: "head of INPUT_NameRef[x] :- NameRef[(x: NameRef)]." .to_string(),
                                                    fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                    {
                                                        let ref x = match *unsafe {<::types::NameRef>::from_ddvalue_ref(&__v) } {
                                                            ref x => (*x).clone(),
                                                            _ => return None
                                                        };
                                                        Some(((*x).clone()).into_ddvalue())
                                                    }
                                                    __f},
                                                    next: Box::new(None)
                                                })
                                }],
                            arrangements: vec![
                                ],
                            change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                        };
    let Property = Relation {
                       name:         "Property".to_string(),
                       input:        true,
                       distinct:     false,
                       caching_mode: CachingMode::Set,
                       key_func:     None,
                       id:           Relations::Property as RelId,
                       rules:        vec![
                           ],
                       arrangements: vec![
                           ],
                       change_cb:    None
                   };
    let INPUT_Property = Relation {
                             name:         "INPUT_Property".to_string(),
                             input:        false,
                             distinct:     false,
                             caching_mode: CachingMode::Set,
                             key_func:     None,
                             id:           Relations::INPUT_Property as RelId,
                             rules:        vec![
                                 /* INPUT_Property[x] :- Property[(x: Property)]. */
                                 Rule::CollectionRule {
                                     description: "INPUT_Property[x] :- Property[(x: Property)].".to_string(),
                                     rel: Relations::Property as RelId,
                                     xform: Some(XFormCollection::FilterMap{
                                                     description: "head of INPUT_Property[x] :- Property[(x: Property)]." .to_string(),
                                                     fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                     {
                                                         let ref x = match *unsafe {<::types::Property>::from_ddvalue_ref(&__v) } {
                                                             ref x => (*x).clone(),
                                                             _ => return None
                                                         };
                                                         Some(((*x).clone()).into_ddvalue())
                                                     }
                                                     __f},
                                                     next: Box::new(None)
                                                 })
                                 }],
                             arrangements: vec![
                                 ],
                             change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                         };
    let Return = Relation {
                     name:         "Return".to_string(),
                     input:        true,
                     distinct:     false,
                     caching_mode: CachingMode::Set,
                     key_func:     None,
                     id:           Relations::Return as RelId,
                     rules:        vec![
                         ],
                     arrangements: vec![
                         ],
                     change_cb:    None
                 };
    let INPUT_Return = Relation {
                           name:         "INPUT_Return".to_string(),
                           input:        false,
                           distinct:     false,
                           caching_mode: CachingMode::Set,
                           key_func:     None,
                           id:           Relations::INPUT_Return as RelId,
                           rules:        vec![
                               /* INPUT_Return[x] :- Return[(x: Return)]. */
                               Rule::CollectionRule {
                                   description: "INPUT_Return[x] :- Return[(x: Return)].".to_string(),
                                   rel: Relations::Return as RelId,
                                   xform: Some(XFormCollection::FilterMap{
                                                   description: "head of INPUT_Return[x] :- Return[(x: Return)]." .to_string(),
                                                   fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                   {
                                                       let ref x = match *unsafe {<::types::Return>::from_ddvalue_ref(&__v) } {
                                                           ref x => (*x).clone(),
                                                           _ => return None
                                                       };
                                                       Some(((*x).clone()).into_ddvalue())
                                                   }
                                                   __f},
                                                   next: Box::new(None)
                                               })
                               }],
                           arrangements: vec![
                               ],
                           change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                       };
    let Statement = Relation {
                        name:         "Statement".to_string(),
                        input:        true,
                        distinct:     false,
                        caching_mode: CachingMode::Set,
                        key_func:     None,
                        id:           Relations::Statement as RelId,
                        rules:        vec![
                            ],
                        arrangements: vec![
                            Arrangement::Map{
                               name: r###"(Statement{.id=(_0: StmtId), .kind=(_: StmtKind), .scope=(_: Scope), .span=(_: Span)}: Statement) /*join*/"###.to_string(),
                                afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                {
                                    let __cloned = __v.clone();
                                    match unsafe {< ::types::Statement>::from_ddvalue(__v) } {
                                        ::types::Statement{id: ref _0, kind: _, scope: _, span: _} => Some(((*_0).clone()).into_ddvalue()),
                                        _ => None
                                    }.map(|x|(x,__cloned))
                                }
                                __f},
                                queryable: false
                            },
                            Arrangement::Map{
                               name: r###"(Statement{.id=(_0: StmtId), .kind=(StmtVarDecl{}: StmtKind), .scope=(_: Scope), .span=(_: Span)}: Statement) /*join*/"###.to_string(),
                                afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                {
                                    let __cloned = __v.clone();
                                    match unsafe {< ::types::Statement>::from_ddvalue(__v) } {
                                        ::types::Statement{id: ref _0, kind: ::types::StmtKind::StmtVarDecl{}, scope: _, span: _} => Some(((*_0).clone()).into_ddvalue()),
                                        _ => None
                                    }.map(|x|(x,__cloned))
                                }
                                __f},
                                queryable: false
                            }],
                        change_cb:    None
                    };
    let INPUT_Statement = Relation {
                              name:         "INPUT_Statement".to_string(),
                              input:        false,
                              distinct:     false,
                              caching_mode: CachingMode::Set,
                              key_func:     None,
                              id:           Relations::INPUT_Statement as RelId,
                              rules:        vec![
                                  /* INPUT_Statement[x] :- Statement[(x: Statement)]. */
                                  Rule::CollectionRule {
                                      description: "INPUT_Statement[x] :- Statement[(x: Statement)].".to_string(),
                                      rel: Relations::Statement as RelId,
                                      xform: Some(XFormCollection::FilterMap{
                                                      description: "head of INPUT_Statement[x] :- Statement[(x: Statement)]." .to_string(),
                                                      fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                      {
                                                          let ref x = match *unsafe {<::types::Statement>::from_ddvalue_ref(&__v) } {
                                                              ref x => (*x).clone(),
                                                              _ => return None
                                                          };
                                                          Some(((*x).clone()).into_ddvalue())
                                                      }
                                                      __f},
                                                      next: Box::new(None)
                                                  })
                                  }],
                              arrangements: vec![
                                  ],
                              change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                          };
    let Switch = Relation {
                     name:         "Switch".to_string(),
                     input:        true,
                     distinct:     false,
                     caching_mode: CachingMode::Set,
                     key_func:     None,
                     id:           Relations::Switch as RelId,
                     rules:        vec![
                         ],
                     arrangements: vec![
                         ],
                     change_cb:    None
                 };
    let INPUT_Switch = Relation {
                           name:         "INPUT_Switch".to_string(),
                           input:        false,
                           distinct:     false,
                           caching_mode: CachingMode::Set,
                           key_func:     None,
                           id:           Relations::INPUT_Switch as RelId,
                           rules:        vec![
                               /* INPUT_Switch[x] :- Switch[(x: Switch)]. */
                               Rule::CollectionRule {
                                   description: "INPUT_Switch[x] :- Switch[(x: Switch)].".to_string(),
                                   rel: Relations::Switch as RelId,
                                   xform: Some(XFormCollection::FilterMap{
                                                   description: "head of INPUT_Switch[x] :- Switch[(x: Switch)]." .to_string(),
                                                   fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                   {
                                                       let ref x = match *unsafe {<::types::Switch>::from_ddvalue_ref(&__v) } {
                                                           ref x => (*x).clone(),
                                                           _ => return None
                                                       };
                                                       Some(((*x).clone()).into_ddvalue())
                                                   }
                                                   __f},
                                                   next: Box::new(None)
                                               })
                               }],
                           arrangements: vec![
                               ],
                           change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                       };
    let SwitchCase = Relation {
                         name:         "SwitchCase".to_string(),
                         input:        true,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::SwitchCase as RelId,
                         rules:        vec![
                             ],
                         arrangements: vec![
                             ],
                         change_cb:    None
                     };
    let INPUT_SwitchCase = Relation {
                               name:         "INPUT_SwitchCase".to_string(),
                               input:        false,
                               distinct:     false,
                               caching_mode: CachingMode::Set,
                               key_func:     None,
                               id:           Relations::INPUT_SwitchCase as RelId,
                               rules:        vec![
                                   /* INPUT_SwitchCase[x] :- SwitchCase[(x: SwitchCase)]. */
                                   Rule::CollectionRule {
                                       description: "INPUT_SwitchCase[x] :- SwitchCase[(x: SwitchCase)].".to_string(),
                                       rel: Relations::SwitchCase as RelId,
                                       xform: Some(XFormCollection::FilterMap{
                                                       description: "head of INPUT_SwitchCase[x] :- SwitchCase[(x: SwitchCase)]." .to_string(),
                                                       fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                       {
                                                           let ref x = match *unsafe {<::types::SwitchCase>::from_ddvalue_ref(&__v) } {
                                                               ref x => (*x).clone(),
                                                               _ => return None
                                                           };
                                                           Some(((*x).clone()).into_ddvalue())
                                                       }
                                                       __f},
                                                       next: Box::new(None)
                                                   })
                                   }],
                               arrangements: vec![
                                   ],
                               change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                           };
    let Template = Relation {
                       name:         "Template".to_string(),
                       input:        true,
                       distinct:     false,
                       caching_mode: CachingMode::Set,
                       key_func:     None,
                       id:           Relations::Template as RelId,
                       rules:        vec![
                           ],
                       arrangements: vec![
                           ],
                       change_cb:    None
                   };
    let INPUT_Template = Relation {
                             name:         "INPUT_Template".to_string(),
                             input:        false,
                             distinct:     false,
                             caching_mode: CachingMode::Set,
                             key_func:     None,
                             id:           Relations::INPUT_Template as RelId,
                             rules:        vec![
                                 /* INPUT_Template[x] :- Template[(x: Template)]. */
                                 Rule::CollectionRule {
                                     description: "INPUT_Template[x] :- Template[(x: Template)].".to_string(),
                                     rel: Relations::Template as RelId,
                                     xform: Some(XFormCollection::FilterMap{
                                                     description: "head of INPUT_Template[x] :- Template[(x: Template)]." .to_string(),
                                                     fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                     {
                                                         let ref x = match *unsafe {<::types::Template>::from_ddvalue_ref(&__v) } {
                                                             ref x => (*x).clone(),
                                                             _ => return None
                                                         };
                                                         Some(((*x).clone()).into_ddvalue())
                                                     }
                                                     __f},
                                                     next: Box::new(None)
                                                 })
                                 }],
                             arrangements: vec![
                                 ],
                             change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                         };
    let Ternary = Relation {
                      name:         "Ternary".to_string(),
                      input:        true,
                      distinct:     false,
                      caching_mode: CachingMode::Set,
                      key_func:     None,
                      id:           Relations::Ternary as RelId,
                      rules:        vec![
                          ],
                      arrangements: vec![
                          ],
                      change_cb:    None
                  };
    let INPUT_Ternary = Relation {
                            name:         "INPUT_Ternary".to_string(),
                            input:        false,
                            distinct:     false,
                            caching_mode: CachingMode::Set,
                            key_func:     None,
                            id:           Relations::INPUT_Ternary as RelId,
                            rules:        vec![
                                /* INPUT_Ternary[x] :- Ternary[(x: Ternary)]. */
                                Rule::CollectionRule {
                                    description: "INPUT_Ternary[x] :- Ternary[(x: Ternary)].".to_string(),
                                    rel: Relations::Ternary as RelId,
                                    xform: Some(XFormCollection::FilterMap{
                                                    description: "head of INPUT_Ternary[x] :- Ternary[(x: Ternary)]." .to_string(),
                                                    fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                    {
                                                        let ref x = match *unsafe {<::types::Ternary>::from_ddvalue_ref(&__v) } {
                                                            ref x => (*x).clone(),
                                                            _ => return None
                                                        };
                                                        Some(((*x).clone()).into_ddvalue())
                                                    }
                                                    __f},
                                                    next: Box::new(None)
                                                })
                                }],
                            arrangements: vec![
                                ],
                            change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                        };
    let Throw = Relation {
                    name:         "Throw".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::Throw as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_Throw = Relation {
                          name:         "INPUT_Throw".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_Throw as RelId,
                          rules:        vec![
                              /* INPUT_Throw[x] :- Throw[(x: Throw)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_Throw[x] :- Throw[(x: Throw)].".to_string(),
                                  rel: Relations::Throw as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_Throw[x] :- Throw[(x: Throw)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::Throw>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let Try = Relation {
                  name:         "Try".to_string(),
                  input:        true,
                  distinct:     false,
                  caching_mode: CachingMode::Set,
                  key_func:     None,
                  id:           Relations::Try as RelId,
                  rules:        vec![
                      ],
                  arrangements: vec![
                      ],
                  change_cb:    None
              };
    let INPUT_Try = Relation {
                        name:         "INPUT_Try".to_string(),
                        input:        false,
                        distinct:     false,
                        caching_mode: CachingMode::Set,
                        key_func:     None,
                        id:           Relations::INPUT_Try as RelId,
                        rules:        vec![
                            /* INPUT_Try[x] :- Try[(x: Try)]. */
                            Rule::CollectionRule {
                                description: "INPUT_Try[x] :- Try[(x: Try)].".to_string(),
                                rel: Relations::Try as RelId,
                                xform: Some(XFormCollection::FilterMap{
                                                description: "head of INPUT_Try[x] :- Try[(x: Try)]." .to_string(),
                                                fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                {
                                                    let ref x = match *unsafe {<::types::Try>::from_ddvalue_ref(&__v) } {
                                                        ref x => (*x).clone(),
                                                        _ => return None
                                                    };
                                                    Some(((*x).clone()).into_ddvalue())
                                                }
                                                __f},
                                                next: Box::new(None)
                                            })
                            }],
                        arrangements: vec![
                            ],
                        change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                    };
    let UnaryOp = Relation {
                      name:         "UnaryOp".to_string(),
                      input:        true,
                      distinct:     false,
                      caching_mode: CachingMode::Set,
                      key_func:     None,
                      id:           Relations::UnaryOp as RelId,
                      rules:        vec![
                          ],
                      arrangements: vec![
                          ],
                      change_cb:    None
                  };
    let INPUT_UnaryOp = Relation {
                            name:         "INPUT_UnaryOp".to_string(),
                            input:        false,
                            distinct:     false,
                            caching_mode: CachingMode::Set,
                            key_func:     None,
                            id:           Relations::INPUT_UnaryOp as RelId,
                            rules:        vec![
                                /* INPUT_UnaryOp[x] :- UnaryOp[(x: UnaryOp)]. */
                                Rule::CollectionRule {
                                    description: "INPUT_UnaryOp[x] :- UnaryOp[(x: UnaryOp)].".to_string(),
                                    rel: Relations::UnaryOp as RelId,
                                    xform: Some(XFormCollection::FilterMap{
                                                    description: "head of INPUT_UnaryOp[x] :- UnaryOp[(x: UnaryOp)]." .to_string(),
                                                    fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                    {
                                                        let ref x = match *unsafe {<::types::UnaryOp>::from_ddvalue_ref(&__v) } {
                                                            ref x => (*x).clone(),
                                                            _ => return None
                                                        };
                                                        Some(((*x).clone()).into_ddvalue())
                                                    }
                                                    __f},
                                                    next: Box::new(None)
                                                })
                                }],
                            arrangements: vec![
                                ],
                            change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                        };
    let VarDecl = Relation {
                      name:         "VarDecl".to_string(),
                      input:        true,
                      distinct:     false,
                      caching_mode: CachingMode::Set,
                      key_func:     None,
                      id:           Relations::VarDecl as RelId,
                      rules:        vec![
                          ],
                      arrangements: vec![
                          ],
                      change_cb:    None
                  };
    let NameInScope = Relation {
                          name:         "NameInScope".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::NameInScope as RelId,
                          rules:        vec![
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdGlobal{.global=global}: AnyId)}: NameInScope)] :- ImplicitGlobal[(ImplicitGlobal{.id=(global: GlobalId), .name=(name: internment::Intern<string>)}: ImplicitGlobal)], EveryScope[(EveryScope{.scope=(scope: Scope)}: EveryScope)]. */
                              Rule::ArrangementRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdGlobal{.global=global}: AnyId)}: NameInScope)] :- ImplicitGlobal[(ImplicitGlobal{.id=(global: GlobalId), .name=(name: internment::Intern<string>)}: ImplicitGlobal)], EveryScope[(EveryScope{.scope=(scope: Scope)}: EveryScope)].".to_string(),
                                  arr: ( Relations::ImplicitGlobal as RelId, 0),
                                  xform: XFormArrangement::Join{
                                             description: "ImplicitGlobal[(ImplicitGlobal{.id=(global: GlobalId), .name=(name: internment::Intern<string>)}: ImplicitGlobal)], EveryScope[(EveryScope{.scope=(scope: Scope)}: EveryScope)]".to_string(),
                                             ffun: None,
                                             arrangement: (Relations::EveryScope as RelId,0),
                                             jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                             {
                                                 let (ref global, ref name) = match *unsafe {<::types::ImplicitGlobal>::from_ddvalue_ref(__v1) } {
                                                     ::types::ImplicitGlobal{id: ref global, name: ref name} => ((*global).clone(), (*name).clone()),
                                                     _ => return None
                                                 };
                                                 let ref scope = match *unsafe {<::types::EveryScope>::from_ddvalue_ref(__v2) } {
                                                     ::types::EveryScope{scope: ref scope} => (*scope).clone(),
                                                     _ => return None
                                                 };
                                                 Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (::types::AnyId::AnyIdGlobal{global: (*global).clone()})})).into_ddvalue())
                                             }
                                             __f},
                                             next: Box::new(None)
                                         }
                              },
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdStmt{.stmt=stmt}: AnyId)}: NameInScope)] :- LetDecl[(LetDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: LetDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)]. */
                              Rule::CollectionRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdStmt{.stmt=stmt}: AnyId)}: NameInScope)] :- LetDecl[(LetDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: LetDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)].".to_string(),
                                  rel: Relations::LetDecl as RelId,
                                  xform: Some(XFormCollection::Arrange {
                                                  description: "arrange LetDecl[(LetDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: LetDecl)] by (stmt)" .to_string(),
                                                  afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                  {
                                                      let (ref stmt, ref pat) = match *unsafe {<::types::LetDecl>::from_ddvalue_ref(&__v) } {
                                                          ::types::LetDecl{stmt_id: ref stmt, pattern: ::types::ddlog_std::Option::Some{x: ref pat}, value: _} => ((*stmt).clone(), (*pat).clone()),
                                                          _ => return None
                                                      };
                                                      let ref name: ::types::internment::Intern<String> = match (*::types::internment::ival(pat)).clone() {
                                                          ::types::Pattern{name: name} => name,
                                                          _ => return None
                                                      };
                                                      Some((((*stmt).clone()).into_ddvalue(), (::types::ddlog_std::tuple2((*stmt).clone(), (*name).clone())).into_ddvalue()))
                                                  }
                                                  __f},
                                                  next: Box::new(XFormArrangement::Join{
                                                                     description: "LetDecl[(LetDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: LetDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)]".to_string(),
                                                                     ffun: None,
                                                                     arrangement: (Relations::Statement as RelId,0),
                                                                     jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                     {
                                                                         let ::types::ddlog_std::tuple2(ref stmt, ref name) = *unsafe {<::types::ddlog_std::tuple2<::types::StmtId, ::types::internment::Intern<String>>>::from_ddvalue_ref( __v1 ) };
                                                                         let ref scope = match *unsafe {<::types::Statement>::from_ddvalue_ref(__v2) } {
                                                                             ::types::Statement{id: _, kind: _, scope: ref scope, span: _} => (*scope).clone(),
                                                                             _ => return None
                                                                         };
                                                                         Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (::types::AnyId::AnyIdStmt{stmt: (*stmt).clone()})})).into_ddvalue())
                                                                     }
                                                                     __f},
                                                                     next: Box::new(None)
                                                                 })
                                              })
                              },
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdStmt{.stmt=stmt}: AnyId)}: NameInScope)] :- ConstDecl[(ConstDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: ConstDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)]. */
                              Rule::CollectionRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdStmt{.stmt=stmt}: AnyId)}: NameInScope)] :- ConstDecl[(ConstDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: ConstDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)].".to_string(),
                                  rel: Relations::ConstDecl as RelId,
                                  xform: Some(XFormCollection::Arrange {
                                                  description: "arrange ConstDecl[(ConstDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: ConstDecl)] by (stmt)" .to_string(),
                                                  afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                  {
                                                      let (ref stmt, ref pat) = match *unsafe {<::types::ConstDecl>::from_ddvalue_ref(&__v) } {
                                                          ::types::ConstDecl{stmt_id: ref stmt, pattern: ::types::ddlog_std::Option::Some{x: ref pat}, value: _} => ((*stmt).clone(), (*pat).clone()),
                                                          _ => return None
                                                      };
                                                      let ref name: ::types::internment::Intern<String> = match (*::types::internment::ival(pat)).clone() {
                                                          ::types::Pattern{name: name} => name,
                                                          _ => return None
                                                      };
                                                      Some((((*stmt).clone()).into_ddvalue(), (::types::ddlog_std::tuple2((*stmt).clone(), (*name).clone())).into_ddvalue()))
                                                  }
                                                  __f},
                                                  next: Box::new(XFormArrangement::Join{
                                                                     description: "ConstDecl[(ConstDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: ConstDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)]".to_string(),
                                                                     ffun: None,
                                                                     arrangement: (Relations::Statement as RelId,0),
                                                                     jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                     {
                                                                         let ::types::ddlog_std::tuple2(ref stmt, ref name) = *unsafe {<::types::ddlog_std::tuple2<::types::StmtId, ::types::internment::Intern<String>>>::from_ddvalue_ref( __v1 ) };
                                                                         let ref scope = match *unsafe {<::types::Statement>::from_ddvalue_ref(__v2) } {
                                                                             ::types::Statement{id: _, kind: _, scope: ref scope, span: _} => (*scope).clone(),
                                                                             _ => return None
                                                                         };
                                                                         Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (::types::AnyId::AnyIdStmt{stmt: (*stmt).clone()})})).into_ddvalue())
                                                                     }
                                                                     __f},
                                                                     next: Box::new(None)
                                                                 })
                                              })
                              },
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdStmt{.stmt=stmt}: AnyId)}: NameInScope)] :- VarDecl[(VarDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: VarDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(decl_scope: Scope), .span=(_: Span)}: Statement)], ClosestFunction[(ClosestFunction{.scope=(decl_scope: Scope), .func=(func: FuncId)}: ClosestFunction)], Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)]. */
                              Rule::CollectionRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdStmt{.stmt=stmt}: AnyId)}: NameInScope)] :- VarDecl[(VarDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: VarDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(decl_scope: Scope), .span=(_: Span)}: Statement)], ClosestFunction[(ClosestFunction{.scope=(decl_scope: Scope), .func=(func: FuncId)}: ClosestFunction)], Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)].".to_string(),
                                  rel: Relations::VarDecl as RelId,
                                  xform: Some(XFormCollection::Arrange {
                                                  description: "arrange VarDecl[(VarDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: VarDecl)] by (stmt)" .to_string(),
                                                  afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                  {
                                                      let (ref stmt, ref pat) = match *unsafe {<::types::VarDecl>::from_ddvalue_ref(&__v) } {
                                                          ::types::VarDecl{stmt_id: ref stmt, pattern: ::types::ddlog_std::Option::Some{x: ref pat}, value: _} => ((*stmt).clone(), (*pat).clone()),
                                                          _ => return None
                                                      };
                                                      let ref name: ::types::internment::Intern<String> = match (*::types::internment::ival(pat)).clone() {
                                                          ::types::Pattern{name: name} => name,
                                                          _ => return None
                                                      };
                                                      Some((((*stmt).clone()).into_ddvalue(), (::types::ddlog_std::tuple2((*stmt).clone(), (*name).clone())).into_ddvalue()))
                                                  }
                                                  __f},
                                                  next: Box::new(XFormArrangement::Join{
                                                                     description: "VarDecl[(VarDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: VarDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(decl_scope: Scope), .span=(_: Span)}: Statement)]".to_string(),
                                                                     ffun: None,
                                                                     arrangement: (Relations::Statement as RelId,0),
                                                                     jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                     {
                                                                         let ::types::ddlog_std::tuple2(ref stmt, ref name) = *unsafe {<::types::ddlog_std::tuple2<::types::StmtId, ::types::internment::Intern<String>>>::from_ddvalue_ref( __v1 ) };
                                                                         let ref decl_scope = match *unsafe {<::types::Statement>::from_ddvalue_ref(__v2) } {
                                                                             ::types::Statement{id: _, kind: _, scope: ref decl_scope, span: _} => (*decl_scope).clone(),
                                                                             _ => return None
                                                                         };
                                                                         Some((::types::ddlog_std::tuple3((*stmt).clone(), (*name).clone(), (*decl_scope).clone())).into_ddvalue())
                                                                     }
                                                                     __f},
                                                                     next: Box::new(Some(XFormCollection::Arrange {
                                                                                             description: "arrange VarDecl[(VarDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: VarDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(decl_scope: Scope), .span=(_: Span)}: Statement)] by (decl_scope)" .to_string(),
                                                                                             afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                                                             {
                                                                                                 let ::types::ddlog_std::tuple3(ref stmt, ref name, ref decl_scope) = *unsafe {<::types::ddlog_std::tuple3<::types::StmtId, ::types::internment::Intern<String>, ::types::Scope>>::from_ddvalue_ref( &__v ) };
                                                                                                 Some((((*decl_scope).clone()).into_ddvalue(), (::types::ddlog_std::tuple2((*stmt).clone(), (*name).clone())).into_ddvalue()))
                                                                                             }
                                                                                             __f},
                                                                                             next: Box::new(XFormArrangement::Join{
                                                                                                                description: "VarDecl[(VarDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: VarDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(decl_scope: Scope), .span=(_: Span)}: Statement)], ClosestFunction[(ClosestFunction{.scope=(decl_scope: Scope), .func=(func: FuncId)}: ClosestFunction)]".to_string(),
                                                                                                                ffun: None,
                                                                                                                arrangement: (Relations::ClosestFunction as RelId,0),
                                                                                                                jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                                                                {
                                                                                                                    let ::types::ddlog_std::tuple2(ref stmt, ref name) = *unsafe {<::types::ddlog_std::tuple2<::types::StmtId, ::types::internment::Intern<String>>>::from_ddvalue_ref( __v1 ) };
                                                                                                                    let ref func = match *unsafe {<::types::ClosestFunction>::from_ddvalue_ref(__v2) } {
                                                                                                                        ::types::ClosestFunction{scope: _, func: ref func} => (*func).clone(),
                                                                                                                        _ => return None
                                                                                                                    };
                                                                                                                    Some((::types::ddlog_std::tuple3((*stmt).clone(), (*name).clone(), (*func).clone())).into_ddvalue())
                                                                                                                }
                                                                                                                __f},
                                                                                                                next: Box::new(Some(XFormCollection::Arrange {
                                                                                                                                        description: "arrange VarDecl[(VarDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: VarDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(decl_scope: Scope), .span=(_: Span)}: Statement)], ClosestFunction[(ClosestFunction{.scope=(decl_scope: Scope), .func=(func: FuncId)}: ClosestFunction)] by (func)" .to_string(),
                                                                                                                                        afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                                                                                                        {
                                                                                                                                            let ::types::ddlog_std::tuple3(ref stmt, ref name, ref func) = *unsafe {<::types::ddlog_std::tuple3<::types::StmtId, ::types::internment::Intern<String>, ::types::FuncId>>::from_ddvalue_ref( &__v ) };
                                                                                                                                            Some((((*func).clone()).into_ddvalue(), (::types::ddlog_std::tuple2((*stmt).clone(), (*name).clone())).into_ddvalue()))
                                                                                                                                        }
                                                                                                                                        __f},
                                                                                                                                        next: Box::new(XFormArrangement::Join{
                                                                                                                                                           description: "VarDecl[(VarDecl{.stmt_id=(stmt: StmtId), .pattern=(ddlog_std::Some{.x=(pat: internment::Intern<Pattern>)}: ddlog_std::Option<IPattern>), .value=(_: ddlog_std::Option<ExprId>)}: VarDecl)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Statement[(Statement{.id=(stmt: StmtId), .kind=(_: StmtKind), .scope=(decl_scope: Scope), .span=(_: Span)}: Statement)], ClosestFunction[(ClosestFunction{.scope=(decl_scope: Scope), .func=(func: FuncId)}: ClosestFunction)], Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)]".to_string(),
                                                                                                                                                           ffun: None,
                                                                                                                                                           arrangement: (Relations::Function as RelId,1),
                                                                                                                                                           jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                                                                                                           {
                                                                                                                                                               let ::types::ddlog_std::tuple2(ref stmt, ref name) = *unsafe {<::types::ddlog_std::tuple2<::types::StmtId, ::types::internment::Intern<String>>>::from_ddvalue_ref( __v1 ) };
                                                                                                                                                               let ref scope = match *unsafe {<::types::Function>::from_ddvalue_ref(__v2) } {
                                                                                                                                                                   ::types::Function{id: _, name: _, scope: ref scope} => (*scope).clone(),
                                                                                                                                                                   _ => return None
                                                                                                                                                               };
                                                                                                                                                               Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (::types::AnyId::AnyIdStmt{stmt: (*stmt).clone()})})).into_ddvalue())
                                                                                                                                                           }
                                                                                                                                                           __f},
                                                                                                                                                           next: Box::new(None)
                                                                                                                                                       })
                                                                                                                                    }))
                                                                                                            })
                                                                                         }))
                                                                 })
                                              })
                              },
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdFunc{.func=func}: AnyId)}: NameInScope)] :- Function[(Function{.id=(func: FuncId), .name=(ddlog_std::Some{.x=(name: internment::Intern<string>)}: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)]. */
                              Rule::CollectionRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdFunc{.func=func}: AnyId)}: NameInScope)] :- Function[(Function{.id=(func: FuncId), .name=(ddlog_std::Some{.x=(name: internment::Intern<string>)}: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)].".to_string(),
                                  rel: Relations::Function as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdFunc{.func=func}: AnyId)}: NameInScope)] :- Function[(Function{.id=(func: FuncId), .name=(ddlog_std::Some{.x=(name: internment::Intern<string>)}: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let (ref func, ref name, ref scope) = match *unsafe {<::types::Function>::from_ddvalue_ref(&__v) } {
                                                          ::types::Function{id: ref func, name: ::types::ddlog_std::Option::Some{x: ref name}, scope: ref scope} => ((*func).clone(), (*name).clone(), (*scope).clone()),
                                                          _ => return None
                                                      };
                                                      Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (::types::AnyId::AnyIdFunc{func: (*func).clone()})})).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              },
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdFunc{.func=func}: AnyId)}: NameInScope)] :- FunctionArg[(FunctionArg{.parent_func=(func: FuncId), .pattern=(pat: internment::Intern<Pattern>)}: FunctionArg)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)]. */
                              Rule::CollectionRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdFunc{.func=func}: AnyId)}: NameInScope)] :- FunctionArg[(FunctionArg{.parent_func=(func: FuncId), .pattern=(pat: internment::Intern<Pattern>)}: FunctionArg)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)].".to_string(),
                                  rel: Relations::FunctionArg as RelId,
                                  xform: Some(XFormCollection::Arrange {
                                                  description: "arrange FunctionArg[(FunctionArg{.parent_func=(func: FuncId), .pattern=(pat: internment::Intern<Pattern>)}: FunctionArg)] by (func)" .to_string(),
                                                  afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                  {
                                                      let (ref func, ref pat) = match *unsafe {<::types::FunctionArg>::from_ddvalue_ref(&__v) } {
                                                          ::types::FunctionArg{parent_func: ref func, pattern: ref pat} => ((*func).clone(), (*pat).clone()),
                                                          _ => return None
                                                      };
                                                      let ref name: ::types::internment::Intern<String> = match (*::types::internment::ival(pat)).clone() {
                                                          ::types::Pattern{name: name} => name,
                                                          _ => return None
                                                      };
                                                      Some((((*func).clone()).into_ddvalue(), (::types::ddlog_std::tuple2((*func).clone(), (*name).clone())).into_ddvalue()))
                                                  }
                                                  __f},
                                                  next: Box::new(XFormArrangement::Join{
                                                                     description: "FunctionArg[(FunctionArg{.parent_func=(func: FuncId), .pattern=(pat: internment::Intern<Pattern>)}: FunctionArg)], ((SinglePattern{.name=(var name: internment::Intern<string>)}: Pattern) = ((internment::ival: function(internment::Intern<Pattern>):Pattern)(pat))), Function[(Function{.id=(func: FuncId), .name=(_: ddlog_std::Option<Name>), .scope=(scope: Scope)}: Function)]".to_string(),
                                                                     ffun: None,
                                                                     arrangement: (Relations::Function as RelId,1),
                                                                     jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                     {
                                                                         let ::types::ddlog_std::tuple2(ref func, ref name) = *unsafe {<::types::ddlog_std::tuple2<::types::FuncId, ::types::internment::Intern<String>>>::from_ddvalue_ref( __v1 ) };
                                                                         let ref scope = match *unsafe {<::types::Function>::from_ddvalue_ref(__v2) } {
                                                                             ::types::Function{id: _, name: _, scope: ref scope} => (*scope).clone(),
                                                                             _ => return None
                                                                         };
                                                                         Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (::types::AnyId::AnyIdFunc{func: (*func).clone()})})).into_ddvalue())
                                                                     }
                                                                     __f},
                                                                     next: Box::new(None)
                                                                 })
                                              })
                              },
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdExpr{.expr=expr}: AnyId)}: NameInScope)] :- __Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Left{.l=(expr_body: ExprId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)], Expression[(Expression{.id=(expr_body: ExprId), .kind=(_: ExprKind), .scope=(scope: Scope), .span=(_: Span)}: Expression)]. */
                              Rule::ArrangementRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdExpr{.expr=expr}: AnyId)}: NameInScope)] :- __Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Left{.l=(expr_body: ExprId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)], Expression[(Expression{.id=(expr_body: ExprId), .kind=(_: ExprKind), .scope=(scope: Scope), .span=(_: Span)}: Expression)].".to_string(),
                                  arr: ( Relations::__Prefix_0 as RelId, 0),
                                  xform: XFormArrangement::Join{
                                             description: "__Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Left{.l=(expr_body: ExprId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)]".to_string(),
                                             ffun: None,
                                             arrangement: (Relations::Arrow as RelId,0),
                                             jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                             {
                                                 let (ref expr, ref pat, ref name) = match *unsafe {<::types::ddlog_std::tuple3<::types::ExprId, ::types::internment::Intern<::types::Pattern>, ::types::internment::Intern<String>>>::from_ddvalue_ref(__v1) } {
                                                     ::types::ddlog_std::tuple3(ref expr, ref pat, ref name) => ((*expr).clone(), (*pat).clone(), (*name).clone()),
                                                     _ => return None
                                                 };
                                                 let ref expr_body = match *unsafe {<::types::Arrow>::from_ddvalue_ref(__v2) } {
                                                     ::types::Arrow{expr_id: _, body: ::types::ddlog_std::Option::Some{x: ::types::ddlog_std::Either::Left{l: ref expr_body}}} => (*expr_body).clone(),
                                                     _ => return None
                                                 };
                                                 Some((::types::ddlog_std::tuple3((*expr).clone(), (*name).clone(), (*expr_body).clone())).into_ddvalue())
                                             }
                                             __f},
                                             next: Box::new(Some(XFormCollection::Arrange {
                                                                     description: "arrange __Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Left{.l=(expr_body: ExprId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)] by (expr_body)" .to_string(),
                                                                     afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                                     {
                                                                         let ::types::ddlog_std::tuple3(ref expr, ref name, ref expr_body) = *unsafe {<::types::ddlog_std::tuple3<::types::ExprId, ::types::internment::Intern<String>, ::types::ExprId>>::from_ddvalue_ref( &__v ) };
                                                                         Some((((*expr_body).clone()).into_ddvalue(), (::types::ddlog_std::tuple2((*expr).clone(), (*name).clone())).into_ddvalue()))
                                                                     }
                                                                     __f},
                                                                     next: Box::new(XFormArrangement::Join{
                                                                                        description: "__Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Left{.l=(expr_body: ExprId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)], Expression[(Expression{.id=(expr_body: ExprId), .kind=(_: ExprKind), .scope=(scope: Scope), .span=(_: Span)}: Expression)]".to_string(),
                                                                                        ffun: None,
                                                                                        arrangement: (Relations::Expression as RelId,1),
                                                                                        jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                                        {
                                                                                            let ::types::ddlog_std::tuple2(ref expr, ref name) = *unsafe {<::types::ddlog_std::tuple2<::types::ExprId, ::types::internment::Intern<String>>>::from_ddvalue_ref( __v1 ) };
                                                                                            let ref scope = match *unsafe {<::types::Expression>::from_ddvalue_ref(__v2) } {
                                                                                                ::types::Expression{id: _, kind: _, scope: ref scope, span: _} => (*scope).clone(),
                                                                                                _ => return None
                                                                                            };
                                                                                            Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (::types::AnyId::AnyIdExpr{expr: (*expr).clone()})})).into_ddvalue())
                                                                                        }
                                                                                        __f},
                                                                                        next: Box::new(None)
                                                                                    })
                                                                 }))
                                         }
                              },
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdExpr{.expr=expr}: AnyId)}: NameInScope)] :- __Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Right{.r=(stmt_body: StmtId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)], Statement[(Statement{.id=(stmt_body: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)]. */
                              Rule::ArrangementRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=(AnyIdExpr{.expr=expr}: AnyId)}: NameInScope)] :- __Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Right{.r=(stmt_body: StmtId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)], Statement[(Statement{.id=(stmt_body: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)].".to_string(),
                                  arr: ( Relations::__Prefix_0 as RelId, 0),
                                  xform: XFormArrangement::Join{
                                             description: "__Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Right{.r=(stmt_body: StmtId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)]".to_string(),
                                             ffun: None,
                                             arrangement: (Relations::Arrow as RelId,1),
                                             jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                             {
                                                 let (ref expr, ref pat, ref name) = match *unsafe {<::types::ddlog_std::tuple3<::types::ExprId, ::types::internment::Intern<::types::Pattern>, ::types::internment::Intern<String>>>::from_ddvalue_ref(__v1) } {
                                                     ::types::ddlog_std::tuple3(ref expr, ref pat, ref name) => ((*expr).clone(), (*pat).clone(), (*name).clone()),
                                                     _ => return None
                                                 };
                                                 let ref stmt_body = match *unsafe {<::types::Arrow>::from_ddvalue_ref(__v2) } {
                                                     ::types::Arrow{expr_id: _, body: ::types::ddlog_std::Option::Some{x: ::types::ddlog_std::Either::Right{r: ref stmt_body}}} => (*stmt_body).clone(),
                                                     _ => return None
                                                 };
                                                 Some((::types::ddlog_std::tuple3((*expr).clone(), (*name).clone(), (*stmt_body).clone())).into_ddvalue())
                                             }
                                             __f},
                                             next: Box::new(Some(XFormCollection::Arrange {
                                                                     description: "arrange __Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Right{.r=(stmt_body: StmtId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)] by (stmt_body)" .to_string(),
                                                                     afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                                     {
                                                                         let ::types::ddlog_std::tuple3(ref expr, ref name, ref stmt_body) = *unsafe {<::types::ddlog_std::tuple3<::types::ExprId, ::types::internment::Intern<String>, ::types::StmtId>>::from_ddvalue_ref( &__v ) };
                                                                         Some((((*stmt_body).clone()).into_ddvalue(), (::types::ddlog_std::tuple2((*expr).clone(), (*name).clone())).into_ddvalue()))
                                                                     }
                                                                     __f},
                                                                     next: Box::new(XFormArrangement::Join{
                                                                                        description: "__Prefix_0[((expr: ExprId), (pat: internment::Intern<Pattern>), (name: internment::Intern<string>))], Arrow[(Arrow{.expr_id=(expr: ExprId), .body=(ddlog_std::Some{.x=(ddlog_std::Right{.r=(stmt_body: StmtId)}: ddlog_std::Either<ExprId,StmtId>)}: ddlog_std::Option<ddlog_std::Either<ExprId,StmtId>>)}: Arrow)], Statement[(Statement{.id=(stmt_body: StmtId), .kind=(_: StmtKind), .scope=(scope: Scope), .span=(_: Span)}: Statement)]".to_string(),
                                                                                        ffun: None,
                                                                                        arrangement: (Relations::Statement as RelId,0),
                                                                                        jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                                        {
                                                                                            let ::types::ddlog_std::tuple2(ref expr, ref name) = *unsafe {<::types::ddlog_std::tuple2<::types::ExprId, ::types::internment::Intern<String>>>::from_ddvalue_ref( __v1 ) };
                                                                                            let ref scope = match *unsafe {<::types::Statement>::from_ddvalue_ref(__v2) } {
                                                                                                ::types::Statement{id: _, kind: _, scope: ref scope, span: _} => (*scope).clone(),
                                                                                                _ => return None
                                                                                            };
                                                                                            Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (::types::AnyId::AnyIdExpr{expr: (*expr).clone()})})).into_ddvalue())
                                                                                        }
                                                                                        __f},
                                                                                        next: Box::new(None)
                                                                                    })
                                                                 }))
                                         }
                              },
                              /* NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=declared_in}: NameInScope)] :- NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(interum: Scope), .declared_in=(declared_in: AnyId)}: NameInScope)], ChildScope[(ChildScope{.parent=(interum: Scope), .child=(scope: Scope)}: ChildScope)]. */
                              Rule::ArrangementRule {
                                  description: "NameInScope[(NameInScope{.name=name, .scope=scope, .declared_in=declared_in}: NameInScope)] :- NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(interum: Scope), .declared_in=(declared_in: AnyId)}: NameInScope)], ChildScope[(ChildScope{.parent=(interum: Scope), .child=(scope: Scope)}: ChildScope)].".to_string(),
                                  arr: ( Relations::NameInScope as RelId, 1),
                                  xform: XFormArrangement::Join{
                                             description: "NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(interum: Scope), .declared_in=(declared_in: AnyId)}: NameInScope)], ChildScope[(ChildScope{.parent=(interum: Scope), .child=(scope: Scope)}: ChildScope)]".to_string(),
                                             ffun: None,
                                             arrangement: (Relations::ChildScope as RelId,0),
                                             jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                             {
                                                 let (ref name, ref interum, ref declared_in) = match *unsafe {<::types::NameInScope>::from_ddvalue_ref(__v1) } {
                                                     ::types::NameInScope{name: ref name, scope: ref interum, declared_in: ref declared_in} => ((*name).clone(), (*interum).clone(), (*declared_in).clone()),
                                                     _ => return None
                                                 };
                                                 let ref scope = match *unsafe {<::types::ChildScope>::from_ddvalue_ref(__v2) } {
                                                     ::types::ChildScope{parent: _, child: ref scope} => (*scope).clone(),
                                                     _ => return None
                                                 };
                                                 Some(((::types::NameInScope{name: (*name).clone(), scope: (*scope).clone(), declared_in: (*declared_in).clone()})).into_ddvalue())
                                             }
                                             __f},
                                             next: Box::new(None)
                                         }
                              }],
                          arrangements: vec![
                              Arrangement::Set{
                                  name: r###"(NameInScope{.name=(_0: internment::Intern<string>), .scope=(_1: Scope), .declared_in=(_: AnyId)}: NameInScope) /*antijoin*/"###.to_string(),
                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                  {
                                      match unsafe {< ::types::NameInScope>::from_ddvalue(__v) } {
                                          ::types::NameInScope{name: ref _0, scope: ref _1, declared_in: _} => Some((::types::ddlog_std::tuple2((*_0).clone(), (*_1).clone())).into_ddvalue()),
                                          _ => None
                                      }
                                  }
                                  __f},
                                  distinct: true
                              },
                              Arrangement::Map{
                                 name: r###"(NameInScope{.name=(_: internment::Intern<string>), .scope=(_0: Scope), .declared_in=(_: AnyId)}: NameInScope) /*join*/"###.to_string(),
                                  afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                  {
                                      let __cloned = __v.clone();
                                      match unsafe {< ::types::NameInScope>::from_ddvalue(__v) } {
                                          ::types::NameInScope{name: _, scope: ref _0, declared_in: _} => Some(((*_0).clone()).into_ddvalue()),
                                          _ => None
                                      }.map(|x|(x,__cloned))
                                  }
                                  __f},
                                  queryable: false
                              },
                              Arrangement::Map{
                                 name: r###"(NameInScope{.name=(_0: internment::Intern<string>), .scope=(_1: Scope), .declared_in=(AnyIdStmt{.stmt=(_: StmtId)}: AnyId)}: NameInScope) /*join*/"###.to_string(),
                                  afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                  {
                                      let __cloned = __v.clone();
                                      match unsafe {< ::types::NameInScope>::from_ddvalue(__v) } {
                                          ::types::NameInScope{name: ref _0, scope: ref _1, declared_in: ::types::AnyId::AnyIdStmt{stmt: _}} => Some((::types::ddlog_std::tuple2((*_0).clone(), (*_1).clone())).into_ddvalue()),
                                          _ => None
                                      }.map(|x|(x,__cloned))
                                  }
                                  __f},
                                  queryable: false
                              }],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let InvalidNameUse = Relation {
                             name:         "InvalidNameUse".to_string(),
                             input:        false,
                             distinct:     true,
                             caching_mode: CachingMode::Set,
                             key_func:     None,
                             id:           Relations::InvalidNameUse as RelId,
                             rules:        vec![
                                 /* InvalidNameUse[(InvalidNameUse{.name=name, .scope=scope, .span=span}: InvalidNameUse)] :- NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(scope: Scope), .span=(span: Span)}: Expression)], not NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(scope: Scope), .declared_in=(_: AnyId)}: NameInScope)]. */
                                 Rule::ArrangementRule {
                                     description: "InvalidNameUse[(InvalidNameUse{.name=name, .scope=scope, .span=span}: InvalidNameUse)] :- NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(scope: Scope), .span=(span: Span)}: Expression)], not NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(scope: Scope), .declared_in=(_: AnyId)}: NameInScope)].".to_string(),
                                     arr: ( Relations::NameRef as RelId, 0),
                                     xform: XFormArrangement::Join{
                                                description: "NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(scope: Scope), .span=(span: Span)}: Expression)]".to_string(),
                                                ffun: None,
                                                arrangement: (Relations::Expression as RelId,0),
                                                jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                {
                                                    let (ref expr, ref name) = match *unsafe {<::types::NameRef>::from_ddvalue_ref(__v1) } {
                                                        ::types::NameRef{expr_id: ref expr, value: ref name} => ((*expr).clone(), (*name).clone()),
                                                        _ => return None
                                                    };
                                                    let (ref scope, ref span) = match *unsafe {<::types::Expression>::from_ddvalue_ref(__v2) } {
                                                        ::types::Expression{id: _, kind: ::types::ExprKind::ExprNameRef{}, scope: ref scope, span: ref span} => ((*scope).clone(), (*span).clone()),
                                                        _ => return None
                                                    };
                                                    Some((::types::ddlog_std::tuple3((*name).clone(), (*scope).clone(), (*span).clone())).into_ddvalue())
                                                }
                                                __f},
                                                next: Box::new(Some(XFormCollection::Arrange {
                                                                        description: "arrange NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(scope: Scope), .span=(span: Span)}: Expression)] by (name, scope)" .to_string(),
                                                                        afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                                        {
                                                                            let ::types::ddlog_std::tuple3(ref name, ref scope, ref span) = *unsafe {<::types::ddlog_std::tuple3<::types::internment::Intern<String>, ::types::Scope, ::types::Span>>::from_ddvalue_ref( &__v ) };
                                                                            Some(((::types::ddlog_std::tuple2((*name).clone(), (*scope).clone())).into_ddvalue(), (::types::ddlog_std::tuple3((*name).clone(), (*scope).clone(), (*span).clone())).into_ddvalue()))
                                                                        }
                                                                        __f},
                                                                        next: Box::new(XFormArrangement::Antijoin {
                                                                                           description: "NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(scope: Scope), .span=(span: Span)}: Expression)], not NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(scope: Scope), .declared_in=(_: AnyId)}: NameInScope)]".to_string(),
                                                                                           ffun: None,
                                                                                           arrangement: (Relations::NameInScope as RelId,0),
                                                                                           next: Box::new(Some(XFormCollection::FilterMap{
                                                                                                                   description: "head of InvalidNameUse[(InvalidNameUse{.name=name, .scope=scope, .span=span}: InvalidNameUse)] :- NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(scope: Scope), .span=(span: Span)}: Expression)], not NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(scope: Scope), .declared_in=(_: AnyId)}: NameInScope)]." .to_string(),
                                                                                                                   fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                                                                                   {
                                                                                                                       let ::types::ddlog_std::tuple3(ref name, ref scope, ref span) = *unsafe {<::types::ddlog_std::tuple3<::types::internment::Intern<String>, ::types::Scope, ::types::Span>>::from_ddvalue_ref( &__v ) };
                                                                                                                       Some(((::types::InvalidNameUse{name: (*name).clone(), scope: (*scope).clone(), span: (*span).clone()})).into_ddvalue())
                                                                                                                   }
                                                                                                                   __f},
                                                                                                                   next: Box::new(None)
                                                                                                               }))
                                                                                       })
                                                                    }))
                                            }
                                 }],
                             arrangements: vec![
                                 ],
                             change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                         };
    let VarUseBeforeDeclaration = Relation {
                                      name:         "VarUseBeforeDeclaration".to_string(),
                                      input:        false,
                                      distinct:     true,
                                      caching_mode: CachingMode::Set,
                                      key_func:     None,
                                      id:           Relations::VarUseBeforeDeclaration as RelId,
                                      rules:        vec![
                                          /* VarUseBeforeDeclaration[(VarUseBeforeDeclaration{.name=name, .used_in=used_in, .declared_in=declared_in}: VarUseBeforeDeclaration)] :- NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)], NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(used_scope: Scope), .declared_in=(AnyIdStmt{.stmt=(stmt: StmtId)}: AnyId)}: NameInScope)], Statement[(Statement{.id=(stmt: StmtId), .kind=(StmtVarDecl{}: StmtKind), .scope=(declared_scope: Scope), .span=(declared_in: Span)}: Statement)], ChildScope[(ChildScope{.parent=(used_scope: Scope), .child=(declared_scope: Scope)}: ChildScope)]. */
                                          Rule::ArrangementRule {
                                              description: "VarUseBeforeDeclaration[(VarUseBeforeDeclaration{.name=name, .used_in=used_in, .declared_in=declared_in}: VarUseBeforeDeclaration)] :- NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)], NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(used_scope: Scope), .declared_in=(AnyIdStmt{.stmt=(stmt: StmtId)}: AnyId)}: NameInScope)], Statement[(Statement{.id=(stmt: StmtId), .kind=(StmtVarDecl{}: StmtKind), .scope=(declared_scope: Scope), .span=(declared_in: Span)}: Statement)], ChildScope[(ChildScope{.parent=(used_scope: Scope), .child=(declared_scope: Scope)}: ChildScope)].".to_string(),
                                              arr: ( Relations::NameRef as RelId, 0),
                                              xform: XFormArrangement::Join{
                                                         description: "NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)]".to_string(),
                                                         ffun: None,
                                                         arrangement: (Relations::Expression as RelId,0),
                                                         jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                         {
                                                             let (ref expr, ref name) = match *unsafe {<::types::NameRef>::from_ddvalue_ref(__v1) } {
                                                                 ::types::NameRef{expr_id: ref expr, value: ref name} => ((*expr).clone(), (*name).clone()),
                                                                 _ => return None
                                                             };
                                                             let (ref used_scope, ref used_in) = match *unsafe {<::types::Expression>::from_ddvalue_ref(__v2) } {
                                                                 ::types::Expression{id: _, kind: ::types::ExprKind::ExprNameRef{}, scope: ref used_scope, span: ref used_in} => ((*used_scope).clone(), (*used_in).clone()),
                                                                 _ => return None
                                                             };
                                                             Some((::types::ddlog_std::tuple3((*name).clone(), (*used_scope).clone(), (*used_in).clone())).into_ddvalue())
                                                         }
                                                         __f},
                                                         next: Box::new(Some(XFormCollection::Arrange {
                                                                                 description: "arrange NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)] by (name, used_scope)" .to_string(),
                                                                                 afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                                                 {
                                                                                     let ::types::ddlog_std::tuple3(ref name, ref used_scope, ref used_in) = *unsafe {<::types::ddlog_std::tuple3<::types::internment::Intern<String>, ::types::Scope, ::types::Span>>::from_ddvalue_ref( &__v ) };
                                                                                     Some(((::types::ddlog_std::tuple2((*name).clone(), (*used_scope).clone())).into_ddvalue(), (::types::ddlog_std::tuple3((*name).clone(), (*used_scope).clone(), (*used_in).clone())).into_ddvalue()))
                                                                                 }
                                                                                 __f},
                                                                                 next: Box::new(XFormArrangement::Join{
                                                                                                    description: "NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)], NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(used_scope: Scope), .declared_in=(AnyIdStmt{.stmt=(stmt: StmtId)}: AnyId)}: NameInScope)]".to_string(),
                                                                                                    ffun: None,
                                                                                                    arrangement: (Relations::NameInScope as RelId,2),
                                                                                                    jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                                                    {
                                                                                                        let ::types::ddlog_std::tuple3(ref name, ref used_scope, ref used_in) = *unsafe {<::types::ddlog_std::tuple3<::types::internment::Intern<String>, ::types::Scope, ::types::Span>>::from_ddvalue_ref( __v1 ) };
                                                                                                        let ref stmt = match *unsafe {<::types::NameInScope>::from_ddvalue_ref(__v2) } {
                                                                                                            ::types::NameInScope{name: _, scope: _, declared_in: ::types::AnyId::AnyIdStmt{stmt: ref stmt}} => (*stmt).clone(),
                                                                                                            _ => return None
                                                                                                        };
                                                                                                        Some((::types::ddlog_std::tuple4((*name).clone(), (*used_scope).clone(), (*used_in).clone(), (*stmt).clone())).into_ddvalue())
                                                                                                    }
                                                                                                    __f},
                                                                                                    next: Box::new(Some(XFormCollection::Arrange {
                                                                                                                            description: "arrange NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)], NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(used_scope: Scope), .declared_in=(AnyIdStmt{.stmt=(stmt: StmtId)}: AnyId)}: NameInScope)] by (stmt)" .to_string(),
                                                                                                                            afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                                                                                            {
                                                                                                                                let ::types::ddlog_std::tuple4(ref name, ref used_scope, ref used_in, ref stmt) = *unsafe {<::types::ddlog_std::tuple4<::types::internment::Intern<String>, ::types::Scope, ::types::Span, ::types::StmtId>>::from_ddvalue_ref( &__v ) };
                                                                                                                                Some((((*stmt).clone()).into_ddvalue(), (::types::ddlog_std::tuple3((*name).clone(), (*used_scope).clone(), (*used_in).clone())).into_ddvalue()))
                                                                                                                            }
                                                                                                                            __f},
                                                                                                                            next: Box::new(XFormArrangement::Join{
                                                                                                                                               description: "NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)], NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(used_scope: Scope), .declared_in=(AnyIdStmt{.stmt=(stmt: StmtId)}: AnyId)}: NameInScope)], Statement[(Statement{.id=(stmt: StmtId), .kind=(StmtVarDecl{}: StmtKind), .scope=(declared_scope: Scope), .span=(declared_in: Span)}: Statement)]".to_string(),
                                                                                                                                               ffun: None,
                                                                                                                                               arrangement: (Relations::Statement as RelId,1),
                                                                                                                                               jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,__v2: &DDValue) -> Option<DDValue>
                                                                                                                                               {
                                                                                                                                                   let ::types::ddlog_std::tuple3(ref name, ref used_scope, ref used_in) = *unsafe {<::types::ddlog_std::tuple3<::types::internment::Intern<String>, ::types::Scope, ::types::Span>>::from_ddvalue_ref( __v1 ) };
                                                                                                                                                   let (ref declared_scope, ref declared_in) = match *unsafe {<::types::Statement>::from_ddvalue_ref(__v2) } {
                                                                                                                                                       ::types::Statement{id: _, kind: ::types::StmtKind::StmtVarDecl{}, scope: ref declared_scope, span: ref declared_in} => ((*declared_scope).clone(), (*declared_in).clone()),
                                                                                                                                                       _ => return None
                                                                                                                                                   };
                                                                                                                                                   Some((::types::ddlog_std::tuple5((*name).clone(), (*used_scope).clone(), (*used_in).clone(), (*declared_scope).clone(), (*declared_in).clone())).into_ddvalue())
                                                                                                                                               }
                                                                                                                                               __f},
                                                                                                                                               next: Box::new(Some(XFormCollection::Arrange {
                                                                                                                                                                       description: "arrange NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)], NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(used_scope: Scope), .declared_in=(AnyIdStmt{.stmt=(stmt: StmtId)}: AnyId)}: NameInScope)], Statement[(Statement{.id=(stmt: StmtId), .kind=(StmtVarDecl{}: StmtKind), .scope=(declared_scope: Scope), .span=(declared_in: Span)}: Statement)] by (used_scope, declared_scope)" .to_string(),
                                                                                                                                                                       afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                                                                                                                                                                       {
                                                                                                                                                                           let ::types::ddlog_std::tuple5(ref name, ref used_scope, ref used_in, ref declared_scope, ref declared_in) = *unsafe {<::types::ddlog_std::tuple5<::types::internment::Intern<String>, ::types::Scope, ::types::Span, ::types::Scope, ::types::Span>>::from_ddvalue_ref( &__v ) };
                                                                                                                                                                           Some(((::types::ddlog_std::tuple2((*used_scope).clone(), (*declared_scope).clone())).into_ddvalue(), (::types::ddlog_std::tuple3((*name).clone(), (*used_in).clone(), (*declared_in).clone())).into_ddvalue()))
                                                                                                                                                                       }
                                                                                                                                                                       __f},
                                                                                                                                                                       next: Box::new(XFormArrangement::Semijoin{
                                                                                                                                                                                          description: "NameRef[(NameRef{.expr_id=(expr: ExprId), .value=(name: internment::Intern<string>)}: NameRef)], Expression[(Expression{.id=(expr: ExprId), .kind=(ExprNameRef{}: ExprKind), .scope=(used_scope: Scope), .span=(used_in: Span)}: Expression)], NameInScope[(NameInScope{.name=(name: internment::Intern<string>), .scope=(used_scope: Scope), .declared_in=(AnyIdStmt{.stmt=(stmt: StmtId)}: AnyId)}: NameInScope)], Statement[(Statement{.id=(stmt: StmtId), .kind=(StmtVarDecl{}: StmtKind), .scope=(declared_scope: Scope), .span=(declared_in: Span)}: Statement)], ChildScope[(ChildScope{.parent=(used_scope: Scope), .child=(declared_scope: Scope)}: ChildScope)]".to_string(),
                                                                                                                                                                                          ffun: None,
                                                                                                                                                                                          arrangement: (Relations::ChildScope as RelId,1),
                                                                                                                                                                                          jfun: &{fn __f(_: &DDValue ,__v1: &DDValue,___v2: &()) -> Option<DDValue>
                                                                                                                                                                                          {
                                                                                                                                                                                              let ::types::ddlog_std::tuple3(ref name, ref used_in, ref declared_in) = *unsafe {<::types::ddlog_std::tuple3<::types::internment::Intern<String>, ::types::Span, ::types::Span>>::from_ddvalue_ref( __v1 ) };
                                                                                                                                                                                              Some(((::types::VarUseBeforeDeclaration{name: (*name).clone(), used_in: (*used_in).clone(), declared_in: (*declared_in).clone()})).into_ddvalue())
                                                                                                                                                                                          }
                                                                                                                                                                                          __f},
                                                                                                                                                                                          next: Box::new(None)
                                                                                                                                                                                      })
                                                                                                                                                                   }))
                                                                                                                                           })
                                                                                                                        }))
                                                                                                })
                                                                             }))
                                                     }
                                          }],
                                      arrangements: vec![
                                          ],
                                      change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                                  };
    let INPUT_VarDecl = Relation {
                            name:         "INPUT_VarDecl".to_string(),
                            input:        false,
                            distinct:     false,
                            caching_mode: CachingMode::Set,
                            key_func:     None,
                            id:           Relations::INPUT_VarDecl as RelId,
                            rules:        vec![
                                /* INPUT_VarDecl[x] :- VarDecl[(x: VarDecl)]. */
                                Rule::CollectionRule {
                                    description: "INPUT_VarDecl[x] :- VarDecl[(x: VarDecl)].".to_string(),
                                    rel: Relations::VarDecl as RelId,
                                    xform: Some(XFormCollection::FilterMap{
                                                    description: "head of INPUT_VarDecl[x] :- VarDecl[(x: VarDecl)]." .to_string(),
                                                    fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                    {
                                                        let ref x = match *unsafe {<::types::VarDecl>::from_ddvalue_ref(&__v) } {
                                                            ref x => (*x).clone(),
                                                            _ => return None
                                                        };
                                                        Some(((*x).clone()).into_ddvalue())
                                                    }
                                                    __f},
                                                    next: Box::new(None)
                                                })
                                }],
                            arrangements: vec![
                                ],
                            change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                        };
    let While = Relation {
                    name:         "While".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::While as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_While = Relation {
                          name:         "INPUT_While".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_While as RelId,
                          rules:        vec![
                              /* INPUT_While[x] :- While[(x: While)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_While[x] :- While[(x: While)].".to_string(),
                                  rel: Relations::While as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_While[x] :- While[(x: While)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::While>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let With = Relation {
                   name:         "With".to_string(),
                   input:        true,
                   distinct:     false,
                   caching_mode: CachingMode::Set,
                   key_func:     None,
                   id:           Relations::With as RelId,
                   rules:        vec![
                       ],
                   arrangements: vec![
                       ],
                   change_cb:    None
               };
    let INPUT_With = Relation {
                         name:         "INPUT_With".to_string(),
                         input:        false,
                         distinct:     false,
                         caching_mode: CachingMode::Set,
                         key_func:     None,
                         id:           Relations::INPUT_With as RelId,
                         rules:        vec![
                             /* INPUT_With[x] :- With[(x: With)]. */
                             Rule::CollectionRule {
                                 description: "INPUT_With[x] :- With[(x: With)].".to_string(),
                                 rel: Relations::With as RelId,
                                 xform: Some(XFormCollection::FilterMap{
                                                 description: "head of INPUT_With[x] :- With[(x: With)]." .to_string(),
                                                 fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                 {
                                                     let ref x = match *unsafe {<::types::With>::from_ddvalue_ref(&__v) } {
                                                         ref x => (*x).clone(),
                                                         _ => return None
                                                     };
                                                     Some(((*x).clone()).into_ddvalue())
                                                 }
                                                 __f},
                                                 next: Box::new(None)
                                             })
                             }],
                         arrangements: vec![
                             ],
                         change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                     };
    let Yield = Relation {
                    name:         "Yield".to_string(),
                    input:        true,
                    distinct:     false,
                    caching_mode: CachingMode::Set,
                    key_func:     None,
                    id:           Relations::Yield as RelId,
                    rules:        vec![
                        ],
                    arrangements: vec![
                        ],
                    change_cb:    None
                };
    let INPUT_Yield = Relation {
                          name:         "INPUT_Yield".to_string(),
                          input:        false,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::INPUT_Yield as RelId,
                          rules:        vec![
                              /* INPUT_Yield[x] :- Yield[(x: Yield)]. */
                              Rule::CollectionRule {
                                  description: "INPUT_Yield[x] :- Yield[(x: Yield)].".to_string(),
                                  rel: Relations::Yield as RelId,
                                  xform: Some(XFormCollection::FilterMap{
                                                  description: "head of INPUT_Yield[x] :- Yield[(x: Yield)]." .to_string(),
                                                  fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                  {
                                                      let ref x = match *unsafe {<::types::Yield>::from_ddvalue_ref(&__v) } {
                                                          ref x => (*x).clone(),
                                                          _ => return None
                                                      };
                                                      Some(((*x).clone()).into_ddvalue())
                                                  }
                                                  __f},
                                                  next: Box::new(None)
                                              })
                              }],
                          arrangements: vec![
                              ],
                          change_cb:    Some(sync::Arc::new(sync::Mutex::new(__update_cb.clone())))
                      };
    let __Null = Relation {
                     name:         "__Null".to_string(),
                     input:        false,
                     distinct:     false,
                     caching_mode: CachingMode::Set,
                     key_func:     None,
                     id:           Relations::__Null as RelId,
                     rules:        vec![
                         ],
                     arrangements: vec![
                         Arrangement::Map{
                            name: r###"_ /*join*/"###.to_string(),
                             afun: &{fn __f(__v: DDValue) -> Option<(DDValue,DDValue)>
                             {
                                 let __cloned = __v.clone();
                                 match unsafe {< ()>::from_ddvalue(__v) } {
                                     _ => Some((()).into_ddvalue()),
                                     _ => None
                                 }.map(|x|(x,__cloned))
                             }
                             __f},
                             queryable: true
                         }],
                     change_cb:    None
                 };
    Program {
        nodes: vec![
            ProgNode::Rel{rel: Array},
            ProgNode::Rel{rel: INPUT_Array},
            ProgNode::Rel{rel: Arrow},
            ProgNode::Rel{rel: INPUT_Arrow},
            ProgNode::Rel{rel: ArrowParam},
            ProgNode::Rel{rel: INPUT_ArrowParam},
            ProgNode::Rel{rel: __Prefix_0},
            ProgNode::Rel{rel: Await},
            ProgNode::Rel{rel: INPUT_Await},
            ProgNode::Rel{rel: BinOp},
            ProgNode::Rel{rel: INPUT_BinOp},
            ProgNode::Rel{rel: BracketAccess},
            ProgNode::Rel{rel: INPUT_BracketAccess},
            ProgNode::Rel{rel: Break},
            ProgNode::Rel{rel: INPUT_Break},
            ProgNode::Rel{rel: ConstDecl},
            ProgNode::Rel{rel: INPUT_ConstDecl},
            ProgNode::Rel{rel: Continue},
            ProgNode::Rel{rel: INPUT_Continue},
            ProgNode::Rel{rel: DoWhile},
            ProgNode::Rel{rel: INPUT_DoWhile},
            ProgNode::Rel{rel: DotAccess},
            ProgNode::Rel{rel: INPUT_DotAccess},
            ProgNode::Rel{rel: EveryScope},
            ProgNode::Rel{rel: INPUT_EveryScope},
            ProgNode::Rel{rel: ExprBigInt},
            ProgNode::Rel{rel: INPUT_ExprBigInt},
            ProgNode::Rel{rel: ExprBool},
            ProgNode::Rel{rel: INPUT_ExprBool},
            ProgNode::Rel{rel: ExprNumber},
            ProgNode::Rel{rel: INPUT_ExprNumber},
            ProgNode::Rel{rel: ExprString},
            ProgNode::Rel{rel: INPUT_ExprString},
            ProgNode::Rel{rel: Expression},
            ProgNode::Rel{rel: INPUT_Expression},
            ProgNode::Rel{rel: For},
            ProgNode::Rel{rel: INPUT_For},
            ProgNode::Rel{rel: ForIn},
            ProgNode::Rel{rel: INPUT_ForIn},
            ProgNode::Rel{rel: Function},
            ProgNode::Rel{rel: INPUT_Function},
            ProgNode::Rel{rel: FunctionArg},
            ProgNode::Rel{rel: INPUT_FunctionArg},
            ProgNode::Rel{rel: If},
            ProgNode::Rel{rel: INPUT_If},
            ProgNode::Rel{rel: ImplicitGlobal},
            ProgNode::Rel{rel: INPUT_ImplicitGlobal},
            ProgNode::Rel{rel: InputScope},
            ProgNode::Rel{rel: ChildScope},
            ProgNode::Rel{rel: ClosestFunction},
            ProgNode::Rel{rel: INPUT_InputScope},
            ProgNode::Rel{rel: Label},
            ProgNode::Rel{rel: INPUT_Label},
            ProgNode::Rel{rel: LetDecl},
            ProgNode::Rel{rel: INPUT_LetDecl},
            ProgNode::Rel{rel: NameRef},
            ProgNode::Rel{rel: INPUT_NameRef},
            ProgNode::Rel{rel: Property},
            ProgNode::Rel{rel: INPUT_Property},
            ProgNode::Rel{rel: Return},
            ProgNode::Rel{rel: INPUT_Return},
            ProgNode::Rel{rel: Statement},
            ProgNode::Rel{rel: INPUT_Statement},
            ProgNode::Rel{rel: Switch},
            ProgNode::Rel{rel: INPUT_Switch},
            ProgNode::Rel{rel: SwitchCase},
            ProgNode::Rel{rel: INPUT_SwitchCase},
            ProgNode::Rel{rel: Template},
            ProgNode::Rel{rel: INPUT_Template},
            ProgNode::Rel{rel: Ternary},
            ProgNode::Rel{rel: INPUT_Ternary},
            ProgNode::Rel{rel: Throw},
            ProgNode::Rel{rel: INPUT_Throw},
            ProgNode::Rel{rel: Try},
            ProgNode::Rel{rel: INPUT_Try},
            ProgNode::Rel{rel: UnaryOp},
            ProgNode::Rel{rel: INPUT_UnaryOp},
            ProgNode::Rel{rel: VarDecl},
            ProgNode::SCC{rels: vec![RecursiveRelation{rel: NameInScope, distinct: true}]},
            ProgNode::Rel{rel: InvalidNameUse},
            ProgNode::Rel{rel: VarUseBeforeDeclaration},
            ProgNode::Rel{rel: INPUT_VarDecl},
            ProgNode::Rel{rel: While},
            ProgNode::Rel{rel: INPUT_While},
            ProgNode::Rel{rel: With},
            ProgNode::Rel{rel: INPUT_With},
            ProgNode::Rel{rel: Yield},
            ProgNode::Rel{rel: INPUT_Yield},
            ProgNode::Rel{rel: __Null}
        ],
        init_data: vec![
        ]
    }
}