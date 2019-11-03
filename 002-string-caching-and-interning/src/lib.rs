use std::fmt::Debug;
use std::alloc::Layout;
use std::ptr::NonNull;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::marker::PhantomData;
use std::borrow::Borrow;

pub(crate) mod deps {
    pub(crate) use struthionidae;
    pub(crate) use rpds;
    pub(crate) use sha3;
}

pub(crate) mod collections {
    pub mod persistent {
        pub use crate::deps::rpds::*;
    }
}

pub(crate) mod hashes {
    pub(crate) use crate::deps::sha3;
}

use crate::deps::struthionidae::logging::debug;
use sha3::digest::generic_array::GenericArray;

pub type Result<T> = crate::deps::struthionidae::result::Result<T>;

#[derive(Copy, Clone)]
struct Static<T> {
    ptr: NonNull<T>
}

impl<T> Static<T> {
    pub const fn new<V>(ptr: NonNull<V>) -> Static<V> where V: 'static + Sized {
        Static {
            ptr
        }
    }
}

unsafe impl<T> Send for Static<T> {}

unsafe impl<T> Sync for Static<T> {}

impl<T> Debug for Static<T> where T: Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        unsafe { self.ptr.as_ref() }.fmt(f)
    }
}


pub unsafe trait AllocInterned {
    type Type: 'static + Sized;
    type Borrowed: ?Sized;
    type Interned: Borrow<Self::Borrowed>;

    unsafe fn alloc_interned(&self, value: &Self::Borrowed) -> Result<Self::Interned>;
}


pub trait Intern<A: AllocInterned> {
    type Interned;
    fn intern(self) -> Self::Interned;
}


#[derive(Copy, Clone)]
pub struct Interned<'a, T, A> {
    ptr: NonNull<T>,
    _p: PhantomData<&'a A>,
}

impl<T, A> Interned<'static, T, A> where T: 'static + Sized, A: 'static + AllocInterned {}

impl<T, A> Interned<'static, T, A> where T: 'static + Sized, A: 'static + AllocInterned {}


impl<'a, T, A> std::cmp::PartialEq<T> for Interned<'a, T, A> where T: PartialEq {
    fn eq(&self, other: &T) -> bool {
        self.borrow().eq(other)
    }
}

impl<'a, T, A> std::cmp::PartialEq<Self> for Interned<'a, T, A> where *const T: std::cmp::PartialEq<*const T> {
    fn eq(&self, other: &Interned<'a, T, A>) -> bool {
        self.ptr.eq(&other.ptr)
    }
}

#[derive(Copy, Clone)]
pub struct InternedSlice<'a, T, A> {
    ptr: NonNull<T>,
    len: usize,
    _p: PhantomData<&'a A>,
}


impl<'a, T, A> InternedSlice<'a, T, A> {
    const fn new(ptr: NonNull<T>, len: usize) -> Self {
        Self {
            ptr,
            len,
            _p: PhantomData,
        }
    }

    pub fn as_slice(&self) -> &'static [T] {
        unsafe {
            std::slice::from_raw_parts(self.ptr.as_ptr(), self.len)
        }
    }
}

impl<'a, T, A> Borrow<[T]> for InternedSlice<'a, T, A> {
    fn borrow(&self) -> &[T] {
        unsafe {
            std::slice::from_raw_parts(self.ptr.as_ptr(), self.len)
        }
    }
}

impl<'a, T, A> std::fmt::Debug for InternedSlice<'a, T, A> where T: std::fmt::Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        <Self as Borrow<[T]>>::borrow(self).fmt(f)
    }
}


impl<T, A> Interned<'static, T, A> where T: 'static + Sized, A: 'static + AllocInterned {}

impl<T, A> Interned<'static, T, A> where T: 'static + Sized, A: 'static + AllocInterned {}


pub struct InternedString<'a, A>(InternedSlice<'a, u8, A>);

impl<'a, A> InternedString<'a, A> {
    const fn new(ptr: NonNull<u8>, len: usize) -> Self {
        Self(InternedSlice::new(ptr, len))
    }
}

impl<'a, A> Borrow<str> for InternedString<'a, A> {
    fn borrow(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(self.0.as_slice()) }
    }
}

impl<'a, A> std::fmt::Debug for InternedString<'a, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("Interned")
            .field(&<Self as Borrow<str>>::borrow(self))
            .finish()
    }
}

impl<'a, A> std::fmt::Display for InternedString<'a, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s = <Self as Borrow<str>>::borrow(self);
        <str as std::fmt::Display>::fmt(s, f)
    }
}

#[test]
fn test_heap() {
    use crate::deps::struthionidae::logging::{span, info, Level};
    use std::alloc::alloc;
    crate::deps::struthionidae::logging::initialize();

    let span = span!(Level::INFO, "test_heap");
    let _enter = span.enter();

    struct Heap;
    unsafe impl AllocInterned for Heap {
        type Type = String;
        type Borrowed = str;
        type Interned = InternedString<'static, Self>;

        unsafe fn alloc_interned(&self, value: &Self::Borrowed) -> Result<Self::Interned> {
            let layout = Layout::for_value(value);
            let ptr = unsafe { alloc(layout) };
            unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), ptr, value.bytes().len()); }
            Ok(InternedString::new(NonNull::new_unchecked(ptr), layout.size()))
        }
    }

    let heap = Heap;
    let interned = unsafe { heap.alloc_interned("hello world") }.unwrap();

    info!("{:?} ", interned);
}


#[test]
fn test_static_string_kv_cache() {
    use crate::deps::struthionidae::logging::{span, info, Level, event};
    use crate::deps::struthionidae::sync::{Arc, RwLock};
    use std::alloc::alloc;
    use crate::hashes::sha3;
    use sha3::Digest;
    type HashAlgo = sha3::Sha3_512;
    type HashDigest = sha3::digest::generic_array::GenericArray<u8, <HashAlgo as sha3::Digest>::OutputSize>;
    crate::deps::struthionidae::logging::initialize();

    let span = span!(Level::INFO, "test_static_string_kv_cache");
    let _enter = span.enter();

    #[inline]
    fn cache_key<B: AsRef<[u8]>>(bytes: B) -> HashDigest {
        let mut algo = HashAlgo::new();
        algo.input(bytes.as_ref());
        algo.result()
    }

    struct HashFmt<'a>(&'a HashDigest);
    impl<'a> std::fmt::LowerHex for HashFmt<'a> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            use crate::deps::struthionidae::bytes;
            f.write_str(&bytes::to_lower_hex_string(&self.0[..]))
        }
    }

    #[derive(Debug, Default)]
    struct StaticStringCache {
        cache: RwLock<std::collections::HashMap<HashDigest, Arc<[u8]>>>,
    };

    unsafe impl AllocInterned for StaticStringCache {
        type Type = String;
        type Borrowed = str;
        type Interned = InternedString<'static, Self>;

        unsafe fn alloc_interned(&self, value: &Self::Borrowed) -> Result<Self::Interned> {
            let span = span!(Level::INFO, "alloc_interned");
            let _enter = span.enter();
            let key = cache_key(value);
            event!(Level::DEBUG, "{} => {:x}", value, HashFmt(&key));
            event!(Level::DEBUG, "{:?}", key);

            let cache = self.cache.read();
            let entry = cache.get(&key);

            match entry {
                Some(interned) => {
                    let ptr = NonNull::new_unchecked(interned.as_ptr() as *mut u8);
                    Ok(InternedString::new(ptr, interned.len()))
                }
                None => {
                    drop(cache);
                    let mut cache = self.cache.write();
                    cache.insert(key, value.to_owned().into_boxed_str().into_boxed_bytes().into());
                    let interned = cache.get(&key).unwrap();
                    let ptr = NonNull::new_unchecked(interned.as_ptr() as *mut u8);
                    Ok(InternedString::new(ptr, interned.len()))
                }
            }
        }
    }



    let heap = StaticStringCache::default();

    let interned = unsafe { heap.alloc_interned("hello world") }.unwrap();

    info!("{:?} ", interned);
}