use std::process::ExitCode;

use clap::Parser;
use quantam_fs::{config::Config, daemon};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match daemon::run(Config::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("qfsd: {error}");
            ExitCode::FAILURE
        }
    }
}
