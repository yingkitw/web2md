//! `web2md` binary entry point: parse CLI input and dispatch to subcommand handlers.

mod ansi;
mod cli;
mod commands;
mod options;
mod tui;
mod watch;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    commands::run(cli).await
}
