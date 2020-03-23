use crate::depends::{
    failure::{bail, err_msg, Error},
    futures::{executor::block_on, Future, Poll},
    indexmap::IndexMap,
    log::{debug, error, info, trace, warn},
    parking_lot::RwLock,
    postgres::{Connection, TlsMode},
    url::Url,
};

#[cfg(feature = "serialize")]
use crate::depends::serde::{Deserialize, Serialize};

use failure::_core::time::Duration;
use std::{
    any::Any,
    borrow::Cow,
    ffi::OsStr,
    io::Read,
    path::PathBuf,
    process::Stdio,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime},
};

pub type ConfigKeys<'a> = Box<dyn Iterator<Item = Cow<'a, str>> + 'a>;
pub type JsonObject = crate::depends::serde_json::map::Map<String, crate::depends::serde_json::value::Value>;

pub struct TestState<S, E> {
    user_state: S,
    resources:  IndexMap<String, Box<dyn Resource<Error = E>>>,
}

impl<S, E> TestState<S, E> {
    pub fn user_state_mut(&mut self) -> &mut S {
        &mut self.user_state
    }

    pub fn user_state(&self) -> &S {
        &self.user_state
    }

    pub fn resource<N: AsRef<str>>(
        &self,
        name: N,
    ) -> Option<&Box<dyn Resource<Error = E>>> {
        self.resources.get(name.as_ref())
    }

    pub fn resource_mut<N: AsRef<str>>(
        &mut self,
        name: N,
    ) -> Option<&mut Box<dyn Resource<Error = E>>> {
        self.resources.get_mut(name.as_ref())
    }
}

pub struct StatelessTest;

impl TestRunner for StatelessTest {
    type Error = ();
    type State = ();

    fn before(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn after(&mut self) {}

    fn run<T, F>(
        &mut self,
        mut inner: F,
    ) -> Result<T, Self::Error>
    where
        F: (FnMut(&mut Self::State) -> T) + std::panic::UnwindSafe,
    {
        self.before()?;
        let ret = Ok(inner(&mut ()));
        self.after();
        ret
    }
}

pub trait Resource {
    type Error;

    fn name<'a>(&'a self) -> Cow<'a, str>;
    fn setup(&mut self) -> Result<(), Self::Error>;
    fn teardown(&mut self) -> Result<(), Self::Error>;
    fn config(&self) -> Option<&dyn ConfigureResource>;
}

//noinspection RsNeedlessLifetimes
pub trait ConfigureResource {
    fn value(&self) -> &dyn Any;
    fn keys<'a>(&'a self) -> ConfigKeys<'a>;
    fn get<'a>(
        &'a self,
        name: &str,
    ) -> Option<&'a (dyn Any + 'a)>;
}

pub trait TestRunner {
    type State;
    type Error;

    fn before(&mut self) -> Result<(), Self::Error>;
    fn after(&mut self);

    fn run<T, F>(
        &mut self,
        inner: F,
    ) -> Result<T, Self::Error>
    where
        F: (FnMut(&mut Self::State) -> T) + std::panic::UnwindSafe;
}

pub struct StandardTestRunner<S, E = Error> {
    name:  Cow<'static, str>,
    state: Arc<Mutex<TestState<S, E>>>,
}

impl<S, E> StandardTestRunner<S, E> {
    const TAG: Cow<'static, str> = Cow::Borrowed("StandardTestRunner");

    pub fn new<N, I>(
        name: N,
        user_state: S,
        resources: I,
    ) -> Self
    where
        N: AsRef<str>,
        I: IntoIterator<Item = Box<dyn Resource<Error = E>>>,
    {
        let mut resources_by_name = IndexMap::new();
        for rsrc in resources.into_iter() {
            assert!(resources_by_name.insert(rsrc.name().to_string(), rsrc).is_none());
        }

        Self {
            name:  Cow::Owned(name.as_ref().to_string()),
            state: Arc::new(Mutex::new(TestState {
                user_state,
                resources: resources_by_name,
            })),
        }
    }
}

impl<E> StandardTestRunner<(), E> {
    pub fn with_resources<N, I>(
        name: N,
        resources: I,
    ) -> Self
    where
        N: AsRef<str>,
        I: IntoIterator<Item = Box<dyn Resource<Error = E>>>,
    {
        Self::new(name, (), resources)
    }
}

impl Default for StandardTestRunner<()> {
    fn default() -> Self {
        Self::with_resources("<None>", vec![])
    }
}

impl<S> TestRunner for StandardTestRunner<S, Error> {
    type Error = Error;
    type State = TestState<S, Error>;

    fn before(&mut self) -> Result<(), Self::Error> {
        for resource in self.state.lock().unwrap().resources.values_mut() {
            debug!(
                "[{}:{}] before: setting up resource {}",
                Self::TAG,
                self.name,
                resource.name()
            );
            resource.setup().map_err(|err| {
                error!(
                    "[{}:{}] before: FAILED to setup resource {}, error: {:?}",
                    Self::TAG,
                    self.name,
                    resource.name(),
                    err
                );
                err
            })?;
        }
        Ok(())
    }

    fn after(&mut self) {
        // In the case of a panic within the test code, the lock is poisoned by this point.
        // However the returned result can be pattern matched to return the underlying
        // guard on both branches.
        let mut guard = match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        for resource in guard.resources.values_mut() {
            resource
                .teardown()
                .unwrap_or_else(|err| panic!("could not teardown resource due to error: {:?}", err));
        }
    }

    fn run<T, F>(
        &mut self,
        mut inner: F,
    ) -> Result<T, Self::Error>
    where
        F: (FnOnce(&mut Self::State) -> T) + std::panic::UnwindSafe,
    {
        self.before()?;
        let mut state = self.state.clone();
        let mut state = state.lock().unwrap();
        let ret = std::panic::catch_unwind(move || inner(&mut state))
            .map_err(|err| err_msg(format!("test failure: {:?}", err)));

        self.after();
        ret
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JsonConfig {
    json: JsonObject,
}

//noinspection RsNeedlessLifetimes
impl ConfigureResource for JsonConfig {
    fn value(&self) -> &dyn Any {
        self
    }

    fn keys<'a>(&'a self) -> ConfigKeys<'a> {
        Box::new(self.json.keys().map(|key| key.into()))
    }

    fn get<'a>(
        &'a self,
        name: &str,
    ) -> Option<&'a (dyn Any + 'a)> {
        use crate::depends::serde_json::Value;
        // The coercion Option<&T> -> Option<&dyn Any> isn't
        // automatic so unwrap the &T and allow the automatic
        // coercion of &T -> &dyn Any happen.
        match self.json.get(name) {
            Some(value) => Some(value),
            None => None,
        }
    }
}
