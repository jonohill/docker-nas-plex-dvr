mod manager;
mod plex;

use figment::{providers::Serialized, Figment};
use manager::{Manager, ManagerConfig};
use plex::{Plex, PlexHost};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Default, Debug)]
struct Config {
    plex_prefs_path: Option<String>,
    plex_url: Option<String>,
    tv_library_id: Option<String>,
    film_library_id: Option<String>,
    channels: Vec<String>,
    size_limit: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config: Config = Figment::from(Serialized::defaults(Config::default()))
        .merge(figment::providers::Env::prefixed("DVR_MANAGER_"))
        .extract()?;

    log::debug!("{:#?}", config);
    
    let host = config.plex_url
        .map(PlexHost::Custom)
        .unwrap_or(PlexHost::Localhost);
    let plex = Plex::new(config.plex_prefs_path, host)?;

    let manager_config = ManagerConfig {
        tv_library_id: config.tv_library_id,
        film_library_id: config.film_library_id,
        channels: config.channels,
        limit: config.size_limit,
    };

    let manager = Manager::new(plex, manager_config).await?;
    manager.auto_record().await?;

    Ok(())
}
