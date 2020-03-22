#![feature(async_await)]

use log::{self, info};
use spinnaker;

fn main() {
    #[cfg(debug_assertions)]
    {
        env_logger::init();
    }
    // run_test()
    // block_on(hello_world());
    let mut pool = futures::executor::ThreadPool::new().unwrap();
    let output = pool.run(spinnaker::async_run_postgres_test());
    info!("{:?}", output);
}
