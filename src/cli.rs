use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "acnl-treasuretown-mngr")]
#[command(about = "A CLI-based manager for making Treasure Towns for Animal Crossing: New Leaf!")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Version,
    Doctor,
    Town {
        #[command(subcommand)]
        command: TownCommands,
    },
    Start {
        name: String,
    },
    Stop {
        name: String,
    },
    Restart {
        name: String,
    },
    Status {
        name: Option<String>,
        #[arg(long, conflicts_with = "name")]
        all: bool,
    },
    Logs {
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum TownCommands {
    Create { name: String },
    List,
    Info { name: String },
    Delete { name: String },
    ImportSave { name: String, path: PathBuf },
    Backup { name: String },
    Snapshots { name: String },
    Restore { name: String, snapshot: String },
    Restock { name: String },
}
