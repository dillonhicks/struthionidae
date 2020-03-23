use std::fmt;
pub(crate) mod deps {
    pub(crate) use struthionidae;
    pub(crate) use failure;
    pub(crate) use failure_derive;
    pub(crate) use serde;
    pub(crate) use futures;
    pub(crate) use os_pipe;
    pub(crate) use lazy_static;
    pub(crate) use libc;
    pub(crate) use pin_utils;
}


pub mod process;
pub mod pipe;