mod manager;
mod plex;

use tokio::time::{Duration, Instant};

use plex::{
    Plex, PlexHost, Subscription, SubscriptionHints, SubscriptionParams, SubscriptionPrefs,
};
use tokio::time::sleep_until;
use manager::{Manager, ManagerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let prefs_path = std::env::args().nth(1);
    let host = std::env::args()
        .nth(2)
        .map(PlexHost::Custom)
        .unwrap_or(PlexHost::Localhost);
    let plex = Plex::new(prefs_path, host)?;

    let manager = Manager::new(plex, ManagerConfig::default()).await?;
    manager.auto_record().await?;

    // let channels = plex.get_channels().await?;
    // if let Some(ch1) = channels.first() {
    //     let data = plex.get_grid(&ch1.id, "2022-06-21").await?;
    //     println!("{:#?}", data);
    // }

    // let sub = Subscription {
    //     target_library_location_id: "".into(),
    //     target_library_section_id: "".into(),
    //     include_grabs: 0,
    //     prefs: SubscriptionPrefs::default(),
    //     hints: SubscriptionHints {
    //         grandparent_guid: "".into(),
    //         grandparent_thumb: "".into(),
    //         grandparent_title: "".into(),
    //         guid: "".into(),
    //         index: "".into(),
    //         originally_available_at: "".into(),
    //         parent_guid: "".into(),
    //         parent_index: "".into(),
    //         parent_title: "".into(),
    //         rating_key: "".into(),
    //         title: "".into(),
    //         r#type: "".into(),
    //     },
    //     params: SubscriptionParams {
    //         airing_channels: "".into(),
    //         airing_times: 0,
    //         library_type: "".into(),
    //         media_provider_id: "".into(),
    //     }
    // };

    // plex.create_subscription(&sub).await?;

    Ok(())
}
