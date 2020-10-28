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


decl_update_deserializer!(UpdateSerializer,(0, ::types::ConstDecl), (1, ::types::ExprBigInt), (2, ::types::ExprBool), (3, ::types::ExprNameRef), (4, ::types::ExprNumber), (5, ::types::ExprString), (6, ::types::Expression), (7, ::types::Function), (8, ::types::FunctionArg), (9, ::types::ConstDecl), (10, ::types::ExprBigInt), (11, ::types::ExprBool), (12, ::types::ExprNameRef), (13, ::types::ExprNumber), (14, ::types::ExprString), (15, ::types::Expression), (16, ::types::Function), (17, ::types::FunctionArg), (18, ::types::InputScope), (19, ::types::LetDecl), (20, ::types::Statement), (21, ::types::VarDecl), (22, ::types::InputScope), (23, ::types::LetDecl), (24, ::types::Statement), (25, ::types::VarDecl));
impl TryFrom<&str> for Relations {
    type Error = ();
    fn try_from(rname: &str) -> ::std::result::Result<Self, ()> {
         match rname {
        "ConstDecl" => Ok(Relations::ConstDecl),
        "ExprBigInt" => Ok(Relations::ExprBigInt),
        "ExprBool" => Ok(Relations::ExprBool),
        "ExprNameRef" => Ok(Relations::ExprNameRef),
        "ExprNumber" => Ok(Relations::ExprNumber),
        "ExprString" => Ok(Relations::ExprString),
        "Expression" => Ok(Relations::Expression),
        "Function" => Ok(Relations::Function),
        "FunctionArg" => Ok(Relations::FunctionArg),
        "INPUT_ConstDecl" => Ok(Relations::INPUT_ConstDecl),
        "INPUT_ExprBigInt" => Ok(Relations::INPUT_ExprBigInt),
        "INPUT_ExprBool" => Ok(Relations::INPUT_ExprBool),
        "INPUT_ExprNameRef" => Ok(Relations::INPUT_ExprNameRef),
        "INPUT_ExprNumber" => Ok(Relations::INPUT_ExprNumber),
        "INPUT_ExprString" => Ok(Relations::INPUT_ExprString),
        "INPUT_Expression" => Ok(Relations::INPUT_Expression),
        "INPUT_Function" => Ok(Relations::INPUT_Function),
        "INPUT_FunctionArg" => Ok(Relations::INPUT_FunctionArg),
        "INPUT_InputScope" => Ok(Relations::INPUT_InputScope),
        "INPUT_LetDecl" => Ok(Relations::INPUT_LetDecl),
        "INPUT_Statement" => Ok(Relations::INPUT_Statement),
        "INPUT_VarDecl" => Ok(Relations::INPUT_VarDecl),
        "InputScope" => Ok(Relations::InputScope),
        "LetDecl" => Ok(Relations::LetDecl),
        "Statement" => Ok(Relations::Statement),
        "VarDecl" => Ok(Relations::VarDecl),
        "__Null" => Ok(Relations::__Null),
             _  => Err(())
         }
    }
}
impl Relations {
    pub fn is_output(&self) -> bool {
        match self {
        Relations::INPUT_ConstDecl => true,
        Relations::INPUT_ExprBigInt => true,
        Relations::INPUT_ExprBool => true,
        Relations::INPUT_ExprNameRef => true,
        Relations::INPUT_ExprNumber => true,
        Relations::INPUT_ExprString => true,
        Relations::INPUT_Expression => true,
        Relations::INPUT_Function => true,
        Relations::INPUT_FunctionArg => true,
        Relations::INPUT_InputScope => true,
        Relations::INPUT_LetDecl => true,
        Relations::INPUT_Statement => true,
        Relations::INPUT_VarDecl => true,
            _  => false
        }
    }
}
impl Relations {
    pub fn is_input(&self) -> bool {
        match self {
        Relations::ConstDecl => true,
        Relations::ExprBigInt => true,
        Relations::ExprBool => true,
        Relations::ExprNameRef => true,
        Relations::ExprNumber => true,
        Relations::ExprString => true,
        Relations::Expression => true,
        Relations::Function => true,
        Relations::FunctionArg => true,
        Relations::InputScope => true,
        Relations::LetDecl => true,
        Relations::Statement => true,
        Relations::VarDecl => true,
            _  => false
        }
    }
}
impl TryFrom<RelId> for Relations {
    type Error = ();
    fn try_from(rid: RelId) -> ::std::result::Result<Self, ()> {
         match rid {
        0 => Ok(Relations::ConstDecl),
        1 => Ok(Relations::ExprBigInt),
        2 => Ok(Relations::ExprBool),
        3 => Ok(Relations::ExprNameRef),
        4 => Ok(Relations::ExprNumber),
        5 => Ok(Relations::ExprString),
        6 => Ok(Relations::Expression),
        7 => Ok(Relations::Function),
        8 => Ok(Relations::FunctionArg),
        9 => Ok(Relations::INPUT_ConstDecl),
        10 => Ok(Relations::INPUT_ExprBigInt),
        11 => Ok(Relations::INPUT_ExprBool),
        12 => Ok(Relations::INPUT_ExprNameRef),
        13 => Ok(Relations::INPUT_ExprNumber),
        14 => Ok(Relations::INPUT_ExprString),
        15 => Ok(Relations::INPUT_Expression),
        16 => Ok(Relations::INPUT_Function),
        17 => Ok(Relations::INPUT_FunctionArg),
        18 => Ok(Relations::INPUT_InputScope),
        19 => Ok(Relations::INPUT_LetDecl),
        20 => Ok(Relations::INPUT_Statement),
        21 => Ok(Relations::INPUT_VarDecl),
        22 => Ok(Relations::InputScope),
        23 => Ok(Relations::LetDecl),
        24 => Ok(Relations::Statement),
        25 => Ok(Relations::VarDecl),
        26 => Ok(Relations::__Null),
             _  => Err(())
         }
    }
}
pub fn relid2name(rid: RelId) -> Option<&'static str> {
   match rid {
        0 => Some(&"ConstDecl"),
        1 => Some(&"ExprBigInt"),
        2 => Some(&"ExprBool"),
        3 => Some(&"ExprNameRef"),
        4 => Some(&"ExprNumber"),
        5 => Some(&"ExprString"),
        6 => Some(&"Expression"),
        7 => Some(&"Function"),
        8 => Some(&"FunctionArg"),
        9 => Some(&"INPUT_ConstDecl"),
        10 => Some(&"INPUT_ExprBigInt"),
        11 => Some(&"INPUT_ExprBool"),
        12 => Some(&"INPUT_ExprNameRef"),
        13 => Some(&"INPUT_ExprNumber"),
        14 => Some(&"INPUT_ExprString"),
        15 => Some(&"INPUT_Expression"),
        16 => Some(&"INPUT_Function"),
        17 => Some(&"INPUT_FunctionArg"),
        18 => Some(&"INPUT_InputScope"),
        19 => Some(&"INPUT_LetDecl"),
        20 => Some(&"INPUT_Statement"),
        21 => Some(&"INPUT_VarDecl"),
        22 => Some(&"InputScope"),
        23 => Some(&"LetDecl"),
        24 => Some(&"Statement"),
        25 => Some(&"VarDecl"),
        26 => Some(&"__Null"),
       _  => None
   }
}
pub fn relid2cname(rid: RelId) -> Option<&'static ::std::ffi::CStr> {
    RELIDMAPC.get(&rid).copied()
}   /// A map of `RelId`s to their name as an `&'static str`
pub static RELIDMAP: ::once_cell::sync::Lazy<::fnv::FnvHashMap<Relations, &'static str>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(27, ::fnv::FnvBuildHasher::default());
        map.insert(Relations::ConstDecl, "ConstDecl");
        map.insert(Relations::ExprBigInt, "ExprBigInt");
        map.insert(Relations::ExprBool, "ExprBool");
        map.insert(Relations::ExprNameRef, "ExprNameRef");
        map.insert(Relations::ExprNumber, "ExprNumber");
        map.insert(Relations::ExprString, "ExprString");
        map.insert(Relations::Expression, "Expression");
        map.insert(Relations::Function, "Function");
        map.insert(Relations::FunctionArg, "FunctionArg");
        map.insert(Relations::INPUT_ConstDecl, "INPUT_ConstDecl");
        map.insert(Relations::INPUT_ExprBigInt, "INPUT_ExprBigInt");
        map.insert(Relations::INPUT_ExprBool, "INPUT_ExprBool");
        map.insert(Relations::INPUT_ExprNameRef, "INPUT_ExprNameRef");
        map.insert(Relations::INPUT_ExprNumber, "INPUT_ExprNumber");
        map.insert(Relations::INPUT_ExprString, "INPUT_ExprString");
        map.insert(Relations::INPUT_Expression, "INPUT_Expression");
        map.insert(Relations::INPUT_Function, "INPUT_Function");
        map.insert(Relations::INPUT_FunctionArg, "INPUT_FunctionArg");
        map.insert(Relations::INPUT_InputScope, "INPUT_InputScope");
        map.insert(Relations::INPUT_LetDecl, "INPUT_LetDecl");
        map.insert(Relations::INPUT_Statement, "INPUT_Statement");
        map.insert(Relations::INPUT_VarDecl, "INPUT_VarDecl");
        map.insert(Relations::InputScope, "InputScope");
        map.insert(Relations::LetDecl, "LetDecl");
        map.insert(Relations::Statement, "Statement");
        map.insert(Relations::VarDecl, "VarDecl");
        map.insert(Relations::__Null, "__Null");
        map
    });
    /// A map of `RelId`s to their name as an `&'static CStr`
pub static RELIDMAPC: ::once_cell::sync::Lazy<::fnv::FnvHashMap<RelId, &'static ::std::ffi::CStr>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(27, ::fnv::FnvBuildHasher::default());
        map.insert(0, ::std::ffi::CStr::from_bytes_with_nul(b"ConstDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(1, ::std::ffi::CStr::from_bytes_with_nul(b"ExprBigInt\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(2, ::std::ffi::CStr::from_bytes_with_nul(b"ExprBool\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(3, ::std::ffi::CStr::from_bytes_with_nul(b"ExprNameRef\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(4, ::std::ffi::CStr::from_bytes_with_nul(b"ExprNumber\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(5, ::std::ffi::CStr::from_bytes_with_nul(b"ExprString\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(6, ::std::ffi::CStr::from_bytes_with_nul(b"Expression\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(7, ::std::ffi::CStr::from_bytes_with_nul(b"Function\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(8, ::std::ffi::CStr::from_bytes_with_nul(b"FunctionArg\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(9, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ConstDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(10, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprBigInt\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(11, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprBool\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(12, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprNameRef\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(13, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprNumber\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(14, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_ExprString\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(15, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Expression\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(16, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Function\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(17, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_FunctionArg\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(18, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_InputScope\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(19, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_LetDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(20, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_Statement\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(21, ::std::ffi::CStr::from_bytes_with_nul(b"INPUT_VarDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(22, ::std::ffi::CStr::from_bytes_with_nul(b"InputScope\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(23, ::std::ffi::CStr::from_bytes_with_nul(b"LetDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(24, ::std::ffi::CStr::from_bytes_with_nul(b"Statement\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(25, ::std::ffi::CStr::from_bytes_with_nul(b"VarDecl\0").expect("Unreachable: A null byte was specifically inserted"));
        map.insert(26, ::std::ffi::CStr::from_bytes_with_nul(b"__Null\0").expect("Unreachable: A null byte was specifically inserted"));
        map
    });
    /// A map of input `Relations`s to their name as an `&'static str`
pub static INPUT_RELIDMAP: ::once_cell::sync::Lazy<::fnv::FnvHashMap<Relations, &'static str>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(13, ::fnv::FnvBuildHasher::default());
        map.insert(Relations::ConstDecl, "ConstDecl");
        map.insert(Relations::ExprBigInt, "ExprBigInt");
        map.insert(Relations::ExprBool, "ExprBool");
        map.insert(Relations::ExprNameRef, "ExprNameRef");
        map.insert(Relations::ExprNumber, "ExprNumber");
        map.insert(Relations::ExprString, "ExprString");
        map.insert(Relations::Expression, "Expression");
        map.insert(Relations::Function, "Function");
        map.insert(Relations::FunctionArg, "FunctionArg");
        map.insert(Relations::InputScope, "InputScope");
        map.insert(Relations::LetDecl, "LetDecl");
        map.insert(Relations::Statement, "Statement");
        map.insert(Relations::VarDecl, "VarDecl");
        map
    });
    /// A map of output `Relations`s to their name as an `&'static str`
pub static OUTPUT_RELIDMAP: ::once_cell::sync::Lazy<::fnv::FnvHashMap<Relations, &'static str>> =
    ::once_cell::sync::Lazy::new(|| {
        let mut map = ::fnv::FnvHashMap::with_capacity_and_hasher(13, ::fnv::FnvBuildHasher::default());
        map.insert(Relations::INPUT_ConstDecl, "INPUT_ConstDecl");
        map.insert(Relations::INPUT_ExprBigInt, "INPUT_ExprBigInt");
        map.insert(Relations::INPUT_ExprBool, "INPUT_ExprBool");
        map.insert(Relations::INPUT_ExprNameRef, "INPUT_ExprNameRef");
        map.insert(Relations::INPUT_ExprNumber, "INPUT_ExprNumber");
        map.insert(Relations::INPUT_ExprString, "INPUT_ExprString");
        map.insert(Relations::INPUT_Expression, "INPUT_Expression");
        map.insert(Relations::INPUT_Function, "INPUT_Function");
        map.insert(Relations::INPUT_FunctionArg, "INPUT_FunctionArg");
        map.insert(Relations::INPUT_InputScope, "INPUT_InputScope");
        map.insert(Relations::INPUT_LetDecl, "INPUT_LetDecl");
        map.insert(Relations::INPUT_Statement, "INPUT_Statement");
        map.insert(Relations::INPUT_VarDecl, "INPUT_VarDecl");
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
        Relations::ConstDecl => {
            Ok(<::types::ConstDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ExprBigInt => {
            Ok(<::types::ExprBigInt>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ExprBool => {
            Ok(<::types::ExprBool>::from_record(_rec)?.into_ddvalue())
        },
        Relations::ExprNameRef => {
            Ok(<::types::ExprNameRef>::from_record(_rec)?.into_ddvalue())
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
        Relations::Function => {
            Ok(<::types::Function>::from_record(_rec)?.into_ddvalue())
        },
        Relations::FunctionArg => {
            Ok(<::types::FunctionArg>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ConstDecl => {
            Ok(<::types::ConstDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ExprBigInt => {
            Ok(<::types::ExprBigInt>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ExprBool => {
            Ok(<::types::ExprBool>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_ExprNameRef => {
            Ok(<::types::ExprNameRef>::from_record(_rec)?.into_ddvalue())
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
        Relations::INPUT_Function => {
            Ok(<::types::Function>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_FunctionArg => {
            Ok(<::types::FunctionArg>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_InputScope => {
            Ok(<::types::InputScope>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_LetDecl => {
            Ok(<::types::LetDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_Statement => {
            Ok(<::types::Statement>::from_record(_rec)?.into_ddvalue())
        },
        Relations::INPUT_VarDecl => {
            Ok(<::types::VarDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::InputScope => {
            Ok(<::types::InputScope>::from_record(_rec)?.into_ddvalue())
        },
        Relations::LetDecl => {
            Ok(<::types::LetDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::Statement => {
            Ok(<::types::Statement>::from_record(_rec)?.into_ddvalue())
        },
        Relations::VarDecl => {
            Ok(<::types::VarDecl>::from_record(_rec)?.into_ddvalue())
        },
        Relations::__Null => {
            Ok(<()>::from_record(_rec)?.into_ddvalue())
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
        Indexes::__Null_by_none => ( 26, 0),
    }
}
#[derive(Copy,Clone,Debug,PartialEq,Eq,Hash)]
pub enum Relations {
    ConstDecl = 0,
    ExprBigInt = 1,
    ExprBool = 2,
    ExprNameRef = 3,
    ExprNumber = 4,
    ExprString = 5,
    Expression = 6,
    Function = 7,
    FunctionArg = 8,
    INPUT_ConstDecl = 9,
    INPUT_ExprBigInt = 10,
    INPUT_ExprBool = 11,
    INPUT_ExprNameRef = 12,
    INPUT_ExprNumber = 13,
    INPUT_ExprString = 14,
    INPUT_Expression = 15,
    INPUT_Function = 16,
    INPUT_FunctionArg = 17,
    INPUT_InputScope = 18,
    INPUT_LetDecl = 19,
    INPUT_Statement = 20,
    INPUT_VarDecl = 21,
    InputScope = 22,
    LetDecl = 23,
    Statement = 24,
    VarDecl = 25,
    __Null = 26
}
#[derive(Copy,Clone,Debug,PartialEq,Eq,Hash)]
pub enum Indexes {
    __Null_by_none = 0
}
pub fn prog(__update_cb: Box<dyn CBFn>) -> Program {
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
    let ExprNameRef = Relation {
                          name:         "ExprNameRef".to_string(),
                          input:        true,
                          distinct:     false,
                          caching_mode: CachingMode::Set,
                          key_func:     None,
                          id:           Relations::ExprNameRef as RelId,
                          rules:        vec![
                              ],
                          arrangements: vec![
                              ],
                          change_cb:    None
                      };
    let INPUT_ExprNameRef = Relation {
                                name:         "INPUT_ExprNameRef".to_string(),
                                input:        false,
                                distinct:     false,
                                caching_mode: CachingMode::Set,
                                key_func:     None,
                                id:           Relations::INPUT_ExprNameRef as RelId,
                                rules:        vec![
                                    /* INPUT_ExprNameRef[x] :- ExprNameRef[(x: ExprNameRef)]. */
                                    Rule::CollectionRule {
                                        description: "INPUT_ExprNameRef[x] :- ExprNameRef[(x: ExprNameRef)].".to_string(),
                                        rel: Relations::ExprNameRef as RelId,
                                        xform: Some(XFormCollection::FilterMap{
                                                        description: "head of INPUT_ExprNameRef[x] :- ExprNameRef[(x: ExprNameRef)]." .to_string(),
                                                        fmfun: &{fn __f(__v: DDValue) -> Option<DDValue>
                                                        {
                                                            let ref x = match *unsafe {<::types::ExprNameRef>::from_ddvalue_ref(&__v) } {
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
                             ],
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
                           ],
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
                             ],
                         change_cb:    None
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
                            ],
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
            ProgNode::Rel{rel: ConstDecl},
            ProgNode::Rel{rel: INPUT_ConstDecl},
            ProgNode::Rel{rel: ExprBigInt},
            ProgNode::Rel{rel: INPUT_ExprBigInt},
            ProgNode::Rel{rel: ExprBool},
            ProgNode::Rel{rel: INPUT_ExprBool},
            ProgNode::Rel{rel: ExprNameRef},
            ProgNode::Rel{rel: INPUT_ExprNameRef},
            ProgNode::Rel{rel: ExprNumber},
            ProgNode::Rel{rel: INPUT_ExprNumber},
            ProgNode::Rel{rel: ExprString},
            ProgNode::Rel{rel: INPUT_ExprString},
            ProgNode::Rel{rel: Expression},
            ProgNode::Rel{rel: INPUT_Expression},
            ProgNode::Rel{rel: Function},
            ProgNode::Rel{rel: INPUT_Function},
            ProgNode::Rel{rel: FunctionArg},
            ProgNode::Rel{rel: INPUT_FunctionArg},
            ProgNode::Rel{rel: InputScope},
            ProgNode::Rel{rel: INPUT_InputScope},
            ProgNode::Rel{rel: LetDecl},
            ProgNode::Rel{rel: INPUT_LetDecl},
            ProgNode::Rel{rel: Statement},
            ProgNode::Rel{rel: INPUT_Statement},
            ProgNode::Rel{rel: VarDecl},
            ProgNode::Rel{rel: INPUT_VarDecl},
            ProgNode::Rel{rel: __Null}
        ],
        init_data: vec![
        ]
    }
}