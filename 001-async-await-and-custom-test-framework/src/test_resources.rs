// `block_on` blocks the current thread until the provided future has run to
// completion. Other executors provide more complex behavior, like scheduling
// multiple futures onto the same thread.
use crate::depends::{
    failure::{bail, err_msg, Error},
    futures::{executor::block_on, Future, Poll},
    indexmap::IndexMap,
    log::{debug, error, info, trace, warn},
    parking_lot::RwLock,
    postgres::{Connection, TlsMode},
    tempfile,
    url::Url,
};

use crate::{
    cmd::{Command, Sh},
    framework::{ConfigureResource, Resource, StandardTestRunner, TestRunner, TestState},
};

#[cfg(feature = "serialize")]
use crate::depends::serde::{Deserialize, Serialize};

use std::{
    any::Any,
    borrow::Cow,
    ffi::OsStr,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    process::Stdio,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

pub type ConfigKeys<'a> = Box<dyn Iterator<Item = Cow<'a, str>> + 'a>;
type Port = u16;
type JsonObject = crate::depends::serde_json::map::Map<String, crate::depends::serde_json::value::Value>;

#[derive(Debug)]
#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
struct Entity {
    id:   i32,
    name: String,
    data: Option<Vec<u8>>,
    pos:  Vec<f32>,
}

async fn hello_world() {
    info!("hello, world!");
}

async fn run_async_command_test() -> std::process::Output {
    let mut cmd = Command::new(
        std::process::Command::new("/bin/ls")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .arg("--help"),
    )
    .unwrap();
    cmd.await
}
//
//pub struct ContainerizedEnvironment<'a> {
//    docker_compose_text: Cow<'a, str>,
//}
//
//pub trait ContainerizedEnvironment<'a> {
//    fn docker_compose<S: AsRef<OsStr>>(
//        &self,
//        compose_filepath: S,
//    ) -> std::process::Command {
//        let docker_compose_path = Sh::which("docker-compose").unwrap();
//
//        let mut command = std::process::Command::new(docker_compose_path);
//        command
//            .stdin(Stdio::null())
//            .stdout(Stdio::piped())
//            .stderr(Stdio::inherit())
//            .arg("-f")
//            .arg(compose_filepath);
//
//        command
//    }
//
//}

#[derive(Debug)]
pub struct PostgresServerConfig {
    server_url:       Url,
    tempdir:          tempfile::TempDir,
    compose_filepath: PathBuf,
}

impl PostgresServerConfig {
    fn server_url(&self) -> &Url {
        &self.server_url
    }
}

impl PostgresServerConfig {
    // TODO: Tried to put this on a `trait Settings: Any` but downcast_ref is only implemented on `impl dyn Any {}`
    //   - instead of impl<T> Any for T {} (the trait object, not the concrete type. It would be great to extend
    //   - Any but that probably requires wrapping the inner Any.. I think
    //
    pub fn keys<'a>(&self) -> ConfigKeys {
        const NAMES: [Cow<'static, str>; 1] = [Cow::Borrowed("server_url")];
        Box::new(NAMES.to_vec().into_iter())
    }
}

//noinspection RsNeedlessLifetimes
impl ConfigureResource for PostgresServerConfig {
    fn value(&self) -> &dyn Any {
        self
    }

    fn keys<'a>(&'a self) -> ConfigKeys<'a> {
        PostgresServerConfig::keys(self)
    }

    fn get<'a>(
        &'a self,
        name: &str,
    ) -> Option<&'a (dyn Any + 'a)> {
        match name {
            "server_url" => Some(&self.server_url),
            "tempdir" => Some(&self.tempdir),
            "compose_filepath" => Some(&self.compose_filepath),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PostgresServer {
    connect_timeout: std::time::Duration,
    max_wait:        std::time::Duration,
    config:          Option<PostgresServerConfig>,
}

impl Default for PostgresServer {
    fn default() -> Self {
        PostgresServer::new()
    }
}

impl PostgresServer {
    const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
    const DEFAULT_WAIT_TIME: Duration = Duration::from_secs(3);
    const TAG: Cow<'static, str> = Cow::Borrowed("Resource:PostgresServer");

    pub fn new() -> Self {
        Self {
            connect_timeout: Self::DEFAULT_CONNECT_TIMEOUT,
            max_wait:        Self::DEFAULT_WAIT_TIME,
            config:          None,
        }
    }

    fn docker_compose<S: AsRef<OsStr>>(filepath: S) -> std::process::Command {
        let docker_compose_path = Sh::which("docker-compose").unwrap();

        let mut command = std::process::Command::new(docker_compose_path);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .arg("-f")
            .arg(filepath);

        command
    }

    fn docker_compose_text() -> Cow<'static, str> {
        r#"
version: '3.1'
services:
  postgres:
    image: postgres:latest
    restart: always
    environment:
      POSTGRES_PASSWORD: postgres
    ports:
      - 5432
"#
        .into()
    }
}

//noinspection RsNeedlessLifetimes
impl Resource for PostgresServer {
    type Error = Error;

    fn name<'a>(&'a self) -> Cow<'a, str> {
        Cow::Borrowed("postgres")
    }

    fn setup(&mut self) -> Result<(), Self::Error> {
        trace!("[{}] called setup", Self::TAG);

        // Create a directory inside of `std::env::temp_dir()`
        let tempdir = tempfile::tempdir()?;
        debug!("[{}] setup: created temporary directory {:?}", Self::TAG, tempdir);

        let compose_filepath = tempdir.path().join("docker-compose.yml");
        {
            let mut file = File::create(&compose_filepath)?;
            writeln!(file, "{}", Self::docker_compose_text())?;
        }

        debug!("[{}] setup: wrote {:?}", Self::TAG, compose_filepath);

        debug!("[{}] setup: starting postgres docker containers", Self::TAG);

        Command::new(Self::docker_compose(&compose_filepath).arg("up").arg("--detach"))
            .unwrap()
            .wait()?;

        let output = Command::new(
            Self::docker_compose(&compose_filepath)
                .arg("port")
                .arg("postgres")
                .arg("5432"),
        )
        .unwrap()
        .wait_with_output()?;

        let stdout = String::from_utf8(output.stdout)?;

        // "0.0.0.0:12345" -> "12345"
        let port_str = stdout
            .trim()
            .split(":")
            .last()
            .ok_or_else(|| err_msg(format!("could not parse postgres port from {:?}", &stdout)))?;

        // check that the port is a number
        let port = Port::from_str(port_str).map_err(|err| {
            err_msg(format!(
                "could not parse postgres port from {:?} becuase of {:?}",
                &port_str, err
            ))
        })?;

        // build the postgres connection url with ephemeral port of the container and
        // the desired connect timeout
        let server_url = Url::parse(&format!(
            "postgres://postgres:postgres@localhost:{}/?connect_timeout={}",
            port,
            self.connect_timeout.as_secs()
        ))?;

        self.config = Some(PostgresServerConfig {
            server_url: server_url.clone(),
            tempdir,
            compose_filepath,
        });

        let wait_start = std::time::Instant::now();
        'wait_for_startup: loop {
            let wait_time = std::time::Instant::now().duration_since(wait_start);

            match Connection::connect(server_url.as_ref(), TlsMode::None) {
                Ok(_) => {
                    break 'wait_for_startup;
                }
                Err(ref err) if wait_time < self.max_wait => {
                    info!(
                        "[{}] waited {:?} for process to startup due to connection error: {:?}",
                        Self::TAG,
                        wait_time,
                        err
                    );
                    std::thread::sleep(Duration::from_millis(250));
                    continue 'wait_for_startup;
                }
                Err(_) => {
                    bail!(
                        "[{}] exceeded max_wait ({:?} >= {:?}) verifying resource was available",
                        Self::TAG,
                        wait_time,
                        self.max_wait
                    );
                }
            }
        }

        info!("[{}] ready {:?}", Self::TAG, self.config.as_ref().unwrap());
        Ok(())
    }

    fn teardown(&mut self) -> Result<(), Self::Error> {
        Command::new(
            Self::docker_compose(self.config.as_ref().unwrap().compose_filepath.as_os_str())
                .arg("down")
                .arg("--timeout")
                .arg("3")
                .arg("--volumes"),
        )
        .unwrap()
        .wait()?;

        Ok(())
    }

    fn config(&self) -> Option<&dyn ConfigureResource> {
        match &self.config {
            Some(cfg) => Some(cfg),
            None => None,
        }
    }
}

pub async fn async_run_postgres_test() {
    run_postgres_test();
}

pub fn run_postgres_test() -> Result<(), Error> {
    const TAG: Cow<'static, str> = Cow::Borrowed("PostgresUpdateTest");

    let resources: Vec<Box<dyn Resource<Error = _>>> = vec![Box::new(PostgresServer::new())];

    let mut test = StandardTestRunner::with_resources(TAG, resources);
    test.run(|state: &mut TestState<(), Error>| {
        let postgres = state.resource("postgres").unwrap();
        let config = postgres.config().unwrap();
        info!("[{}] ConfigKeys: {:?}", TAG, config.keys().collect::<Vec<_>>());

        let settings: &PostgresServerConfig = config.value().downcast_ref::<_>().unwrap();
        let url = settings.server_url().to_string();

        let conn = Connection::connect(url.as_ref(), TlsMode::None).unwrap();
        conn.execute(
            "CREATE TABLE entity (
                    id              SERIAL PRIMARY KEY,
                    name            VARCHAR NOT NULL,
                    data            BYTEA,
                    pos             REAL[]
                  )",
            &[],
        )
        .unwrap();

        for id in 0..10 {
            let me = Entity {
                id,
                name: "Kevin".to_string(),
                data: None,
                pos: vec![1.0f32, 2.0f32, 3.0f32],
            };

            conn.execute(
                "INSERT INTO entity (id, name, data, pos) VALUES ($1, $2, $3, $4)",
                &[&me.id, &me.name, &me.data, &me.pos],
            )
            .unwrap();
        }

        for row in &conn.query("SELECT id, name, data FROM entity", &[]).unwrap() {
            let mut entity = Entity {
                id:   row.get(0),
                name: row.get(1),
                data: row.get(2),
                pos:  Default::default(),
            };

            info!("[{}] found entity {}: {}", TAG, entity.id, entity.name);
            let new_pos = vec![2.0f32, 3.0f32, 5.0f32];

            let start = Instant::now();
            conn.execute("UPDATE entity SET pos = $2 WHERE id = $1", &[&entity.id, &new_pos])
                .unwrap();
            let time = Instant::now().duration_since(start);
            info!("[{}] updated entity  {} in {:?}", TAG, entity.id, time);
        }
    })?;

    Ok(())
}

#[test_case]
pub fn test_run_postgres_test() -> Result<(), Error> {
    run_postgres_test()
}

#[test_case]
pub fn test_shit_the_bed() -> Result<(), Error> {
    bail!("na na na nanana live for today and dont worry about tomorrow")
}
