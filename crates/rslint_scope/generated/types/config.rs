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
pub struct Config {
    pub no_shadow_hoisting: crate::config::NoShadowHoisting
}
impl abomonation::Abomonation for Config{}
::differential_datalog::decl_struct_from_record!(Config["config::Config"]<>, ["config::Config"][1]{[0]no_shadow_hoisting["no_shadow_hoisting"]: crate::config::NoShadowHoisting});
::differential_datalog::decl_struct_into_record!(Config, ["config::Config"]<>, no_shadow_hoisting);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(Config, <>, no_shadow_hoisting: crate::config::NoShadowHoisting);
impl ::std::fmt::Display for Config {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::config::Config{no_shadow_hoisting} => {
                __formatter.write_str("config::Config{")?;
                ::std::fmt::Debug::fmt(no_shadow_hoisting, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for Config {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum NoShadowHoisting {
    HoistingNever,
    HoistingAlways
}
impl abomonation::Abomonation for NoShadowHoisting{}
::differential_datalog::decl_enum_from_record!(NoShadowHoisting["config::NoShadowHoisting"]<>, HoistingNever["config::HoistingNever"][0]{}, HoistingAlways["config::HoistingAlways"][0]{});
::differential_datalog::decl_enum_into_record!(NoShadowHoisting<>, HoistingNever["config::HoistingNever"]{}, HoistingAlways["config::HoistingAlways"]{});
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_enum!(NoShadowHoisting<>, HoistingNever{}, HoistingAlways{});
impl ::std::fmt::Display for NoShadowHoisting {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::config::NoShadowHoisting::HoistingNever{} => {
                __formatter.write_str("config::HoistingNever{")?;
                __formatter.write_str("}")
            },
            crate::config::NoShadowHoisting::HoistingAlways{} => {
                __formatter.write_str("config::HoistingAlways{")?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for NoShadowHoisting {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
impl ::std::default::Default for NoShadowHoisting {
    fn default() -> Self {
        crate::config::NoShadowHoisting::HoistingNever{}
    }
}