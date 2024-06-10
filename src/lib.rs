#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use strum::{EnumString, FromRepr};

pub mod longtaillib;
pub use longtaillib::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
