pub mod build;
pub mod init;
pub mod migrate;
pub mod new;
pub mod test;
pub mod track;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ffi",
    about = "A tool for bootstrapping and managing Rust FFI projects",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new FFI-ready Rust workspace from the template
    Init(init::InitArgs),
    /// Create a new FFI project in the given directory
    New(new::NewArgs),
    /// Add FFI structure to an existing Rust project
    Migrate(migrate::MigrateArgs),
    /// Build language bindings
    Build(build::BuildArgs),
    /// Run binding smoke tests
    Test(test::TestArgs),
    /// Initialize and set up uniffied-workflow repos
    Track(track::TrackArgs),
}
