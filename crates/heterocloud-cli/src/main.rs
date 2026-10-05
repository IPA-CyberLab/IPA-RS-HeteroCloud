use std::process::ExitCode;

use clap::Parser;
use heterocloud_cli::{Cli, TopLevelCommand, execute, update};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let notice = (!matches!(cli.command, TopLevelCommand::Update(_)))
        .then(|| tokio::spawn(update::available_update_notice()));
    let result = match execute(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    };
    if let Some(task) = notice
        && let Ok(Some(message)) = task.await
    {
        eprintln!("{message}");
    }
    result
}
