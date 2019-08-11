#![feature(async_await)]

pub(crate) mod depends {
    pub(crate) use log;
    pub(crate) use postgres;
    pub(crate) use futures;
    pub(crate) use failure;
    pub(crate) use url;
    pub(crate) use indexmap;
    pub(crate) use parking_lot;
}

// `block_on` blocks the current thread until the provided future has run to
// completion. Other executors provide more complex behavior, like scheduling
// multiple futures onto the same thread.
use crate::depends::futures::executor::block_on;
use crate::depends::futures::{Future, Poll};
use crate::depends::log::trace;
use crate::depends::url::Url;
use crate::depends::postgres::{Connection, TlsMode};
use crate::depends::failure::Error;
use crate::depends::indexmap::IndexMap;
use crate::depends::parking_lot::RwLock;

use std::io::Read;
use std::process::Stdio;
use std::borrow::Cow;
use std::any::Any;
use std::sync::Arc;
use std::sync::Mutex;

struct Person {
    id: i32,
    name: String,
    data: Option<Vec<u8>>,
}


struct Command {
    process: std::process::Child
}

impl Command {
    pub fn new(cmd: &mut std::process::Command) -> Result<Self, Error> {
        let process = cmd.spawn()?;
        Ok(Command {
            process
        })
    }
}


impl Future for Command {
    type Output = std::process::Output;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<Self::Output> {

        match self.process.try_wait() {
            Ok(Some(status)) => {
                println!("status READY");

                let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
                match ((&mut self.process).stdout.take(), (&mut self.process).stderr.take()) {
                    (None, None) => {}
                    (Some(mut out), None) => {
                        let res = out.read_to_end(&mut stdout);
                        res.unwrap();
                    }
                    (None, Some(mut err)) => {
                        let res = err.read_to_end(&mut stderr);
                        res.unwrap();
                    }
                    (Some(mut out), Some(mut err)) => {
                        let res = out.read_to_end(&mut stdout).and_then(|size| err.read_to_end(&mut stderr).map(|s| size + s));
                        res.unwrap();
                    }
                }

                Poll::Ready(std::process::Output {
                    status,
                    stdout,
                    stderr,
                })
            },
            Ok(None) => {
                println!("status not ready yet, let's really wait");
                cx.waker().wake_by_ref();
                Poll::Pending
            },
           Err(e) =>{ println!("error attempting to wait: {}", e);
               cx.waker().wake_by_ref();
               Poll::Pending
            }
        }

    }
}


async fn hello_world() {
    println!("hello, world!");
}

async fn run_async_command_test() -> std::process::Output {
    let mut cmd = Command::new(std::process::Command::new("/bin/ls")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .arg("--help"))
        .unwrap();
    cmd.await
}


pub trait TestRunner {
    type State;
    type Error;

    fn before(&mut self) -> Result<(), Self::Error>;
    fn after(&mut self);

    fn run<T, F>(&mut self, mut inner: F) -> Result<T, Self::Error>
    where F:( FnMut(&mut Self::State) -> T) + std::panic::UnwindSafe ;
}


struct StatelessTest;

impl TestRunner for StatelessTest {
    type State = ();
    type Error = ();

    fn before(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn after(&mut self)  {

    }

    fn run<T, F>(&mut self, mut inner: F) -> Result<T, Self::Error> where F: (FnMut(&mut Self::State) -> T) +std::panic::UnwindSafe {
        self.before()?;
        let ret = Ok(inner(&mut ()));
        self.after();
        ret
    }
}

pub struct TestState<S, E> {
    user_state: S,
    resources: IndexMap<String, Box<dyn Resource<Error=E>>>
}

impl<S, E> TestState<S, E> {
    pub fn user_state_mut(&mut self) -> &mut S{
        &mut self.user_state
    }
    pub fn user_state(&self) -> &S {
        &self.user_state
    }


    pub fn resource<N: AsRef<str>>(&self, name: N) -> Option<&Box<dyn Resource<Error=E>>> {
        self.resources.get(name.as_ref())
    }

    pub fn resource_mut<N: AsRef<str>>(&mut self, name: N) -> Option<&mut Box<dyn Resource<Error=E>>> {
        self.resources.get_mut(name.as_ref())
    }
}

pub struct StandardTestRunner<S, E = Error>  {
    state: Arc<Mutex<TestState<S, E>>>
}

impl<S,E> StandardTestRunner<S,E> {
    pub fn new<I>(user_state: S, resources: I) -> Self
        where I: IntoIterator<Item=Box<dyn Resource<Error=E>>>
    {

        let mut resources_by_name = IndexMap::new();
        for rsrc in resources.into_iter() {
            assert!(resources_by_name.insert(rsrc.name().to_string(), rsrc).is_none());
        }

        Self {
           state: Arc::new(Mutex::new(TestState {
               user_state,
               resources: resources_by_name
           }))
        }
    }

}

impl<E> StandardTestRunner<(),E> {
    pub fn with_resources<I>(resources: I) -> Self
        where I: IntoIterator<Item=Box<dyn Resource<Error=E>>>
    {
        Self::new((), resources)
    }
}



impl Default for StandardTestRunner<()> {
    fn default() -> Self {
        Self::with_resources(vec![])
    }
}


impl<S> TestRunner for StandardTestRunner<S, Error> {
    type State = TestState<S, Error>;
    type Error = Error;

    fn before(&mut self) -> Result<(), Self::Error> {
        for resource in self.state.lock().unwrap().resources.values_mut() {
            resource.setup()?;
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
            resource.teardown()
                .unwrap_or_else(|err| panic!("could not teardown resource due to error: {:?}", err));
        }
    }

    fn run<T, F>(&mut self, mut inner: F) -> Result<T, Self::Error> where F: (FnOnce(&mut Self::State) -> T)  + std::panic::UnwindSafe  {
        self.before()?;
        let mut state = self.state.clone();
        let mut state = state.lock().unwrap();
        let ret = std::panic::catch_unwind(move || {
            inner(&mut state)
        }).map_err(|err| crate::depends::failure::err_msg(format!("test failure: {:?}", err)));

        self.after();
        ret
    }
}


pub trait Resource {
    type Error;

    fn name<'a>(&'a self) -> Cow<'a, str>;
    fn setup(&mut self) -> Result<(), Self::Error>;
    fn teardown(&mut self) -> Result<(), Self::Error>;
    fn values(&self) -> Option<&dyn Any>;
}



struct PostgresServerResource{
    pub server_url: Url
}

impl Resource for PostgresServerResource {
    type Error = Error;

    fn name<'a>(&'a self) -> Cow<str> {
        Cow::Borrowed("postgres")
    }

    fn setup(&mut self) -> Result<(), Self::Error> {
        Command::new(std::process::Command::new("/home/dillon/.local/bin/docker-compose")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .arg("-f")
            .arg("docker-compose.yml")
            .arg("up")
            .arg("--detach"))
            .unwrap().process.wait()?;
        Ok(())
    }

    fn teardown(&mut self) -> Result<(), Self::Error> {
        Command::new(std::process::Command::new("/home/dillon/.local/bin/docker-compose")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .arg("-f")
            .arg("docker-compose.yml")
            .arg("down")
            .arg("--timeout")
            .arg("3")
            .arg("--volumes"))
            .unwrap().process.wait()?;
        Ok(())
    }

    fn values(&self) -> Option<&dyn Any> {
        Some(&self.server_url)
    }
}


async fn run_postgres_test() {
    let resources: Vec<Box<dyn Resource<Error=_>>> = vec![
        Box::new(PostgresServerResource { server_url: Url::parse("postgres://postgres:postgres@localhost:8002").unwrap() })
     ];

    let mut test = StandardTestRunner::with_resources(resources);
    test.run(|state: &mut TestState<(), Error>| {

        let postgres = state.resource("postgres").unwrap();
        let value = postgres.values().unwrap();
        let url = value.downcast_ref::<Url>().unwrap().to_string();

        let conn = Connection::connect(url.as_ref(), TlsMode::None).unwrap();
        conn.execute("CREATE TABLE person (
                    id              SERIAL PRIMARY KEY,
                    name            VARCHAR NOT NULL,
                    data            BYTEA
                  )", &[]).unwrap();

        let me = Person {
            id: 0,
            name: "Steven".to_string(),
            data: None,
        };
        conn.execute("INSERT INTO person (name, data) VALUES ($1, $2)",
                     &[&me.name, &me.data]).unwrap();
        for row in &conn.query("SELECT id, name, data FROM person", &[]).unwrap(){
            let person = Person {
                id: row.get(0),
                name: row.get(1),
                data: row.get(2),
            };
            println!("Found person {}: {}", person.id, person.name);
        }

    })
        .unwrap()
}


fn main() {
    // run_test()
    // block_on(hello_world());
    let mut  pool = futures::executor::ThreadPool::new().unwrap();
    let output= pool.run(run_postgres_test());
    println!("{:?}", output);
}