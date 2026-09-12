use std::process::ExitCode;

use clap::Parser;
use quantam_fs::{config::Config, daemon};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match daemon::run(Config::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            quantam_fs::demo_log::event(
                quantam_fs::demo_log::Kind::Warning,
                "LOCAL",
                "qfsd: daemon stopped with an error",
                &[format!("reason  {error}")],
            );
            quantam_fs::demo_log::flush();
            ExitCode::FAILURE
        }
    }
}
