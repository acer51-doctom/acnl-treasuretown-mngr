pub mod cli;
pub mod config;
pub mod hosting;
pub mod logging;
pub mod process_monitor;
pub mod storage;
pub mod town;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, TownCommands};
use config::ConfigStore;
use town::TownManager;

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = config::AppPaths::detect()?;
    let config_store = ConfigStore::new(paths.clone());
    let app_config = config_store.load_or_create()?;
    let manager = TownManager::new(paths, app_config);

    match cli.command {
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Doctor => manager.doctor()?,
        Commands::Town { command } => match command {
            TownCommands::Create { name } => manager.create_town(&name)?,
            TownCommands::List => manager.list_towns()?,
            TownCommands::Info { name } => manager.town_info(&name)?,
            TownCommands::Delete { name } => manager.delete_town(&name)?,
            TownCommands::ImportSave { name, path } => manager.import_save(&name, &path)?,
            TownCommands::Backup { name } => manager.backup_town(&name)?,
            TownCommands::Snapshots { name } => manager.list_snapshots(&name)?,
            TownCommands::Restore { name, snapshot } => {
                manager.restore_snapshot(&name, &snapshot)?
            }
            TownCommands::Restock { name } => manager.restock(&name)?,
        },
        Commands::Start { name } => manager.start_town(&name).await?,
        Commands::Stop { name } => manager.stop_town(&name).await?,
        Commands::Restart { name } => manager.restart_town(&name).await?,
        Commands::Status { name, all } => manager.status(name.as_deref(), all)?,
        Commands::Logs { name } => manager.logs(&name)?,
    }

    Ok(())
}
