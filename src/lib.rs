#![allow(unused_imports)]
#![allow(unused_macros)]
#[macro_use]

extern crate cfg_if;
pub extern crate pairing;
extern crate rand;
extern crate bit_vec;
extern crate byteorder;

#[macro_use]
mod log;

pub mod domain;
pub mod groth16;

mod group;
mod multiexp;
pub mod source;

#[cfg(test)]
mod tests;

cfg_if! {
    if #[cfg(feature = "wasm")] {
        mod wasm_multicore;
        pub mod worker {
            pub use crate::wasm_multicore::*;
        }
    } else if #[cfg(feature = "multicore")] {
        mod multicore;
        pub mod worker {
            pub use crate::multicore::*;
        }
    } else {
        mod singlecore;
        pub mod worker {
            pub use crate::singlecore::*;
        }
    }
}

mod cs;
pub use self::cs::*;

use std::str::FromStr;
use std::env;

cfg_if!{
    if #[cfg(any(not(feature = "nolog"), feature = "sonic"))] {
        fn verbose_flag() -> bool {
            option_env!("BELLMAN_VERBOSE").unwrap_or("0") == "1"
        }
    }
}
