//! Build a `ProPresenter` service playlist from a Planning Center plan.
//!
//! See [`proflow::cli`] for the commands and exit statuses.

use std::process::ExitCode;

use proflow::cli::{run, Invocation, USAGE};
use proflow::config::Config;
use proflow::workstation::Workstation;

#[tokio::main]
async fn main() -> ExitCode {
    let invocation = match Invocation::parse(std::env::args().skip(1)) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("proflow: {error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match execute(&invocation).await {
        Ok(outcome) => {
            print!("{outcome}");
            ExitCode::from(outcome.exit_code())
        }
        Err(error) => {
            eprintln!("proflow: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn execute(invocation: &Invocation) -> anyhow::Result<proflow::cli::Outcome> {
    let workstation = Workstation::load(&Config::load()?, invocation.workspace)?;
    Ok(run(&workstation, invocation).await?)
}
