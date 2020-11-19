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

impl Config {
    pub const fn preset() -> Self {
        Self {
            no_shadow: NoShadowConf {
                enabled: true,
                hoisting: NoShadowHoisting::HoistingNever,
            },
            no_undef: true,
            no_unused_labels: true,
            typeof_undef: true,
            unused_vars: true,
            use_before_def: true,
        }
    }
}

#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct Config {
    pub no_shadow: crate::config::NoShadowConf,
    pub no_undef: bool,
    pub no_unused_labels: bool,
    pub typeof_undef: bool,
    pub unused_vars: bool,
    pub use_before_def: bool
}
impl abomonation::Abomonation for Config{}
::differential_datalog::decl_struct_from_record!(Config["config::Config"]<>, ["config::Config"][6]{[0]no_shadow["no_shadow"]: crate::config::NoShadowConf, [1]no_undef["no_undef"]: bool, [2]no_unused_labels["no_unused_labels"]: bool, [3]typeof_undef["typeof_undef"]: bool, [4]unused_vars["unused_vars"]: bool, [5]use_before_def["use_before_def"]: bool});
::differential_datalog::decl_struct_into_record!(Config, ["config::Config"]<>, no_shadow, no_undef, no_unused_labels, typeof_undef, unused_vars, use_before_def);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(Config, <>, no_shadow: crate::config::NoShadowConf, no_undef: bool, no_unused_labels: bool, typeof_undef: bool, unused_vars: bool, use_before_def: bool);
impl ::std::fmt::Display for Config {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::config::Config{no_shadow,no_undef,no_unused_labels,typeof_undef,unused_vars,use_before_def} => {
                __formatter.write_str("config::Config{")?;
                ::std::fmt::Debug::fmt(no_shadow, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(no_undef, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(no_unused_labels, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(typeof_undef, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(unused_vars, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(use_before_def, __formatter)?;
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
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct NoShadowConf {
    pub enabled: bool,
    pub hoisting: crate::config::NoShadowHoisting
}
impl abomonation::Abomonation for NoShadowConf{}
::differential_datalog::decl_struct_from_record!(NoShadowConf["config::NoShadowConf"]<>, ["config::NoShadowConf"][2]{[0]enabled["enabled"]: bool, [1]hoisting["hoisting"]: crate::config::NoShadowHoisting});
::differential_datalog::decl_struct_into_record!(NoShadowConf, ["config::NoShadowConf"]<>, enabled, hoisting);
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_struct!(NoShadowConf, <>, enabled: bool, hoisting: crate::config::NoShadowHoisting);
impl ::std::fmt::Display for NoShadowConf {
    fn fmt(&self, __formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match self {
            crate::config::NoShadowConf{enabled,hoisting} => {
                __formatter.write_str("config::NoShadowConf{")?;
                ::std::fmt::Debug::fmt(enabled, __formatter)?;
                __formatter.write_str(",")?;
                ::std::fmt::Debug::fmt(hoisting, __formatter)?;
                __formatter.write_str("}")
            }
        }
    }
}
impl ::std::fmt::Debug for NoShadowConf {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&self, f)
    }
}
#[derive(Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum NoShadowHoisting {
    HoistingNever,
    HoistingAlways,
    HoistingFunctions
}
impl abomonation::Abomonation for NoShadowHoisting{}
::differential_datalog::decl_enum_from_record!(NoShadowHoisting["config::NoShadowHoisting"]<>, HoistingNever["config::HoistingNever"][0]{}, HoistingAlways["config::HoistingAlways"][0]{}, HoistingFunctions["config::HoistingFunctions"][0]{});
::differential_datalog::decl_enum_into_record!(NoShadowHoisting<>, HoistingNever["config::HoistingNever"]{}, HoistingAlways["config::HoistingAlways"]{}, HoistingFunctions["config::HoistingFunctions"]{});
#[rustfmt::skip] ::differential_datalog::decl_record_mutator_enum!(NoShadowHoisting<>, HoistingNever{}, HoistingAlways{}, HoistingFunctions{});
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
            },
            crate::config::NoShadowHoisting::HoistingFunctions{} => {
                __formatter.write_str("config::HoistingFunctions{")?;
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