mod collision;
mod commands;
mod repo_identity;
mod template;
mod workspace;

use anyhow::Result;
use clap::Parser;
use commands::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init(args) => commands::init::run(args),
        Commands::New(args) => commands::new::run(args),
        Commands::Migrate(args) => commands::migrate::run(args),
        Commands::Build(args) => commands::build::run(args),
        Commands::Test(args) => commands::test::run(args),
        Commands::Track(args) => commands::track::run(args),
    }
}
