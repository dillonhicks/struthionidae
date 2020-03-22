# Async/Await and Custom Test Framework

**Summary**

Started off as experimenting with `!#[feature(async_await)]` and became an
experiment in `!#[feature(custom_test_framework)]`. The custom test 
framework uses`TestRunner` and `Resouce` traits along with panic handling 
to emulate the JUnit style `before` and `after` test semantics. 

**Requires**

* linux
* docker
* docker-compose
* network connection (or a cached postgres image) 


# Running  


```
$ rustup show active-toolchain
nightly-x86_64-unknown-linux-gnu (default)

$ rustc --version
rustc 1.38.0-nightly (534b42394 2019-08-09)

$ cargo test --lib
    Finished dev [unoptimized + debuginfo] target(s) in 0.04s
     Running ~/001-async-await-and-custom-test-framework/target/debug/deps/spinnaker-7ca3474fdf24a94b
[2019-08-12T15:07:14Z INFO  spinnaker] [test:0] running 0x564fd9dd0d70
[2019-08-12T15:07:14Z DEBUG spinnaker::framework] [StandardTestRunner:PostgresUpdateTest] before: setting up resource postgres
[2019-08-12T15:07:14Z DEBUG spinnaker::test_resources] [Resource:PostgresServer] setup: created temporary directory TempDir { path: "/tmp/.tmpO9vnIH" }
[2019-08-12T15:07:14Z DEBUG spinnaker::test_resources] [Resource:PostgresServer] setup: wrote "/tmp/.tmpO9vnIH/docker-compose.yml"
[2019-08-12T15:07:14Z DEBUG spinnaker::test_resources] [Resource:PostgresServer] setup: starting postgres docker containers
[2019-08-12T15:07:14Z DEBUG spinnaker::cmd] [Sh] which: "docker-compose" -> "~/.local/bin/docker-compose"
Creating network "tmpo9vnih_default" with the default driver
Creating tmpo9vnih_postgres_1 ... done
[2019-08-12T15:07:15Z DEBUG spinnaker::cmd] [Sh] which: "docker-compose" -> "~/.local/bin/docker-compose"
[2019-08-12T15:07:15Z INFO  spinnaker::test_resources] [Resource:PostgresServer] waited 80ns for process to startup due to connection error: Error(Io(Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }))
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [Resource:PostgresServer] waited 250.979835ms for process to startup due to connection error: Error(Io(Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }))
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [Resource:PostgresServer] waited 502.042355ms for process to startup due to connection error: Error(Io(Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }))
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [Resource:PostgresServer] ready PostgresServerConfig { server_url: "postgres://postgres:postgres@localhost:32877/?connect_timeout=1", tempdir: TempDir { path: "/tmp/.tmpO9vnIH" }, compose_filepath: "/tmp/.tmpO9vnIH/docker-compose.yml" }
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] ConfigKeys: ["server_url"]
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 0: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  0 in 1.49533ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 1: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  1 in 1.356359ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 2: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  2 in 1.290185ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 3: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  3 in 1.358242ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 4: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  4 in 1.509266ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 5: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  5 in 1.396134ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 6: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  6 in 1.291837ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 7: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  7 in 1.214852ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 8: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  8 in 1.297297ms
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] found entity 9: Kevin
[2019-08-12T15:07:16Z INFO  spinnaker::test_resources] [PostgresUpdateTest] updated entity  9 in 1.289593ms
[2019-08-12T15:07:16Z DEBUG spinnaker::cmd] [Sh] which: "docker-compose" -> "/home/dillon/.local/bin/docker-compose"
Stopping tmpo9vnih_postgres_1 ... done
Removing tmpo9vnih_postgres_1 ... done
Removing network tmpo9vnih_default
[2019-08-12T15:07:17Z INFO  spinnaker] [test:0] passed in 3.088785448s
[2019-08-12T15:07:17Z INFO  spinnaker] [test:1] running 0x564fd9dd0d80
[2019-08-12T15:07:17Z ERROR spinnaker] [test:1] FAILED after 79.941µs due to ErrorMessage { msg: "na na na nanana live for today and dont worry about tomorrow" }

stack backtrace:
   0: failure::backtrace::internal::InternalBacktrace::new::hc8bfae0ce5a85992 (0x564fd99fe70f)
             at /home/dillon/.cargo/registry/src/github.com-1ecc6299db9ec823/failure-0.1.5/src/backtrace/internal.rs:44
   1: failure::backtrace::Backtrace::new::h785887ea420908c2 (0x564fd99fe1de)
             at /home/dillon/.cargo/registry/src/github.com-1ecc6299db9ec823/failure-0.1.5/src/backtrace/mod.rs:111
   2: <failure::error::error_impl::ErrorImpl as core::convert::From<F>>::from::h7e39c204b21d89f0 (0x564fd955f2e6)
             at /home/dillon/.cargo/registry/src/github.com-1ecc6299db9ec823/failure-0.1.5/src/error/error_impl.rs:19
   3: <failure::error::Error as core::convert::From<F>>::from::hcfdb984057dc781b (0x564fd958a05d)
             at /home/dillon/.cargo/registry/src/github.com-1ecc6299db9ec823/failure-0.1.5/src/error/mod.rs:36
   4: failure::error_message::err_msg::h8100645ceba6140b (0x564fd958f541)
             at /home/dillon/.cargo/registry/src/github.com-1ecc6299db9ec823/failure-0.1.5/src/error_message.rs:12
   5: spinnaker::test_resources::test_shit_the_bed::he0bdf4e5945b752a (0x564fd956bbf8)
             at 001-async-await-and-custom-test-framework/src/test_resources.rs:390
   6: core::ops::function::Fn::call::h9d79150e63565d49 (0x564fd9563cfe)
             at /rustc/534b42394d743511db1335d5ed08d507ab7c6e73/src/libcore/ops/function.rs:69
   7: core::ops::function::impls::<impl core::ops::function::Fn<A> for &F>::call::h7b40b8f9dd810694 (0x564fd9584898)
             at /rustc/534b42394d743511db1335d5ed08d507ab7c6e73/src/libcore/ops/function.rs:244
   8: spinnaker::spinnaker_test_main::h8c61b900eff5e8df (0x564fd958aba9)
             at 001-async-await-and-custom-test-framework/src/lib.rs:37
   9: spinnaker::main::hbef01c79cc35aec3 (0x564fd958b215)
  10: std::rt::lang_start::{{closure}}::h598a785576d1fa0c (0x564fd958e620)
             at /rustc/534b42394d743511db1335d5ed08d507ab7c6e73/src/libstd/rt.rs:64
  11: std::rt::lang_start_internal::{{closure}}::h28ff8287e9455c3a (0x564fd9a4e883)
             at src/libstd/rt.rs:49
      std::panicking::try::do_call::haf15ac1b923e8d21
             at src/libstd/panicking.rs:296
  12: __rust_maybe_catch_panic (0x564fd9a569ba)
             at src/libpanic_unwind/lib.rs:80
  13: std::panicking::try::hd298e972069003c1 (0x564fd9a4f38d)
             at src/libstd/panicking.rs:275
      std::panic::catch_unwind::h1ebb71b7f4614d6d
             at src/libstd/panic.rs:394
      std::rt::lang_start_internal::hdd1116449fab9fb1
             at src/libstd/rt.rs:48
  14: std::rt::lang_start::h6f25126b47cbf109 (0x564fd958e5f9)
             at /rustc/534b42394d743511db1335d5ed08d507ab7c6e73/src/libstd/rt.rs:64
  15: main (0x564fd958b24a)
  16: __libc_start_main (0x7f7f51a85b97)
  17: _start (0x564fd955ad3a)
  18: <unknown> (0x0)

```
