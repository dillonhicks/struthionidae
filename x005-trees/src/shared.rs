#[cfg(feature = "serde")]
use crate::deps::serde::{
    Deserialize,
    Serialize,
};

use core::cell::{
    Ref,
    RefCell,
    RefMut,
};
use std::{
    ops::Deref,
    rc::Rc,
};
use std::rc::Weak;
use std::mem::ManuallyDrop;



/// A lighter-weight version of Arc<RwLock<T>> for `!(Send + Sync)` structures. Instead
/// of using locks and atomics it uses counters and the runtime borrow checker.
///
pub struct Shared<T: ?Sized>(Rc<RefCell<T>>);


impl<T: Sized> Shared<T> {
    pub fn new(init: T) -> Self {
        Shared(Rc::new(RefCell::new(init)))
    }
}


impl<T: ?Sized> Shared<T> {

    pub fn read(&self) -> Ref<'_, T> {
        self.0.deref().borrow()
    }

    pub fn write(&self) -> RefMut<'_, T> {
        self.0.deref().borrow_mut()
    }

}


impl<T: ?Sized> std::convert::From<Rc<RefCell<T>>> for Shared<T> {
    fn from(rc: Rc<RefCell<T>>) -> Self {
        Self(rc)
    }
}


impl<T> Shared<T> {
    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr() as *const _ as *const u8
    }
}

impl<T> std::fmt::Debug for Shared<T>
    where
        T: std::fmt::Debug,
{
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter,
    ) -> std::fmt::Result {
        f.debug_tuple("Shared").field(&self.read()).finish()
    }
}

#[cfg(feature = "serde")]
impl<'de, T> Deserialize<'de> for Shared<T>
    where
        T: Sized + Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
        where
            D: crate::deps::serde::Deserializer<'de>,
    {
        Ok(Shared::new(T::deserialize(deserializer)?))
    }
}

#[cfg(feature = "serde")]
impl<T> Serialize for Shared<T>
    where
        T: Serialize,
{
    fn serialize<S>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
        where
            S: crate::deps::serde::Serializer,
    {
        let inner: Ref<'_, T> = self.0.deref().borrow();
        inner.serialize(serializer)
    }
}

impl<T: ?Sized> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(self.0.clone())
    }
}

impl<T> std::cmp::PartialEq for Shared<T>
    where
        T: Sized + PartialEq,
{
    fn eq(
        &self,
        other: &Self,
    ) -> bool {
        self.read().eq(&other.read())
    }
}

impl<T> std::cmp::Eq for Shared<T> where T: Sized + Eq {}

impl<T> std::cmp::PartialOrd for Shared<T>
    where
        T: Sized + PartialOrd,
{
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<std::cmp::Ordering> {
        self.read().partial_cmp(&other.read())
    }
}

impl<T> std::cmp::Ord for Shared<T>
    where
        T: Sized + Ord,
{
    fn cmp(
        &self,
        other: &Self,
    ) -> std::cmp::Ordering {
        self.read().cmp(&other.read())
    }
}

impl<T> std::hash::Hash for Shared<T>
    where
        T: Sized + std::hash::Hash,
{
    fn hash<H: std::hash::Hasher>(
        &self,
        state: &mut H,
    ) {
        self.read().hash(state)
    }
}

impl<T> Default for Shared<T>
    where
        T: Sized + Default,
{
    fn default() -> Self {
        Shared::new(T::default())
    }
}


#[derive(Debug)]
pub struct WeaklyShared<T: ?Sized>(Weak<RefCell<T>>);


impl<T: ?Sized> WeaklyShared<T> {
    pub fn downgrade(strong: &Shared<T>) -> Self {
        WeaklyShared(Rc::downgrade(&strong.0))
    }

    pub fn upgrade(&self) -> Option<Shared<T>> {
        self.0.upgrade().map(Shared)
    }

}

impl<T> WeaklyShared<T> where T: Sized {
    pub fn dangling() -> WeaklyShared<T> {
        let weak = {
            let rc = Rc::new(ManuallyDrop::new(unsafe { std::mem::MaybeUninit::<RefCell<T>>::uninit().assume_init() }));
            Rc::downgrade(&rc)
        };
        WeaklyShared(unsafe { std::mem::transmute(weak) })

    }
}

#[test]
fn test_interior_mutability() {
    use std::collections::HashMap;
    let mut map = HashMap::<usize, Shared<u32>>::new();

    let answer = Shared::new(0u32);
    for i in 1..=1024usize {
        assert!(map.insert(i, answer.clone()).is_none());
    }

    *(answer.write()) = 42u32;
    assert!(map.values().all(|v| *(v.read()) == 42u32))
}
