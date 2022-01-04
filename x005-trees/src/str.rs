use std::fmt;
use crate::deps::bytes::Bytes;
#[cfg(feature = "serde")]
use crate::deps::serde::{
    Serialize,
    Deserialize,
};

/// An immutable String
#[derive(Clone, PartialOrd, PartialEq, Ord, Eq, Hash)]
pub struct Str(Bytes);


impl Str {
    pub fn from_str(s: &str) -> Self {
        Self(Bytes::copy_from_slice(s.as_bytes()))
    }

    pub const fn from_static(s: &'static str) -> Self {
        Self(Bytes::from_static(s.as_bytes()))
    }

    pub fn as_ref(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(self.0.as_ref()) }
    }
}


impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_ref())
    }
}

impl fmt::Display for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}


impl std::cmp::PartialEq<str> for Str  {
    fn eq(&self, other: &str) -> bool {
        self.as_ref() == other
    }
}

impl std::cmp::PartialEq<std::string::String> for Str  {
    fn eq(&self, other: &std::string::String) -> bool {
        self.as_ref() == other.as_str()
    }
}

impl std::convert::From<String> for Str {
    fn from(s: String) -> Self {
        Self(Bytes::from(s))
    }
}

impl std::convert::From<&'_ str> for Str {
    fn from(s: &str) -> Self {
        Self(Bytes::copy_from_slice(s.as_bytes()))
    }
}
