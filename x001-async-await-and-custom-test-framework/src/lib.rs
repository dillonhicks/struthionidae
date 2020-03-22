#![feature(async_await)]
#![feature(custom_test_frameworks)]
#![test_runner(spinnaker_test_main)]

#[cfg(feature = "serialize")]
extern crate serde;

pub(crate) mod depends {
    pub(crate) use failure;
    pub(crate) use futures;
    pub(crate) use indexmap;
    pub(crate) use log;
    pub(crate) use parking_lot;
    pub(crate) use postgres;
    pub(crate) use serde;
    pub(crate) use serde_json;
    pub(crate) use tempfile;
    pub(crate) use url;
}

pub mod cmd;
mod test_resources;

pub mod framework;
pub use test_resources::*;

use crate::depends::failure::Error;
use futures::StreamExt;

fn spinnaker_test_main(mut functions: &[&dyn Fn() -> Result<(), Error>]) {
    use crate::depends::log::{error, info};
    env_logger::init();

    for (i, test_case_function) in functions.iter().enumerate() {
        info!("[test:{}] running {:?}", i, test_case_function as *const _);
        let start = std::time::Instant::now();
        let ret = test_case_function();
        let elapsed = std::time::Instant::now().duration_since(start);
        match ret {
            Ok(_) => info!("[test:{}] passed in {:?}", i, elapsed),
            Err(err) => error!("[test:{}] FAILED after {:?} due to {:?}", i, elapsed, err),
        }
    }
}
