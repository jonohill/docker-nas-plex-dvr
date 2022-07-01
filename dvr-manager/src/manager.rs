use std::hint;
use std::ops::Sub;
use std::os::unix;

use crate::plex::{self, PlexError, GridMetadata, Channel, Subscription, SubscriptionPrefs, SubscriptionHints, ProvidersMediaProviders, ProviderDirectoryType, GridMetadataType, SubscriptionParams};
use crate::plex::Plex;
use chrono::format::format;
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use futures::future::{try_join_all, try_join};
use tokio::time::sleep;
use itertools::Itertools;

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("Plex error: {0}")]
    Plex(#[from] plex::PlexError),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Unimplemented: {0}")]
    Unimplemented(String),
}

impl ManagerError {
    fn from_unknown_plex_error(err: &str) -> Self {
        ManagerError::Plex(PlexError::PlexResponse(err.to_string()))
    }
}

type Result<T, E = ManagerError> = std::result::Result<T, E>;

const PRE_SCHEDULE_TIME: i64 = 30;

#[derive(Default)]
pub struct ManagerConfig {
    tv_library_id: Option<String>,
    film_library_id: Option<String>,
    channels: Vec<String>,
}

pub struct Manager {
    plex: Plex,
    tv_library_id: String,
    film_library_id: String,
}

impl Manager {
    pub async fn new(plex: Plex, config: ManagerConfig) -> Result<Self> {
        let providers = plex.get_providers().await?;

        let get_library_id = |library_type, default: Option<String>| {
            let show_dirs = providers.get_dirs_of_type(library_type)?;
            let mut show_dir_ids = show_dirs
                .iter()
                .map(|d| d.id.clone().unwrap());
            let id = default
                .and_then(|id| show_dir_ids.find(|did| did == &id))
                .or_else(|| show_dir_ids.next());
            Ok::<_, ManagerError>(id)
        };
        
        let tv_library_id = get_library_id(ProviderDirectoryType::Show, config.tv_library_id)?
            .ok_or_else(|| ManagerError::Config("No matching TV Show library found".into()))?;
        let film_library_id = get_library_id(ProviderDirectoryType::Movie, config.film_library_id)?
            .ok_or_else(|| ManagerError::Config("No matching Film library found".into()))?;

        log::debug!("Using tv library {}, film library {}", tv_library_id, film_library_id);

        Ok(Self { plex, tv_library_id, film_library_id })
    }

    async fn schedule_recording(&self, metadata: GridMetadata) -> Result<()> {
        let templates = self.plex.get_subscription_template(&metadata.guid).await?;
        
        println!("{:#?}", templates);
        
        let media = metadata.media.first()
            .ok_or_else(|| ManagerError::from_unknown_plex_error("Recording has no Media"))?;

        // let channel = format!("{}={}", media.channel_identifier, media.channel_title);
        // // This needs to be double encoded
        // let airing_channels = urlencoding::encode(&urlencoding::encode(&channel)).into_owned();

        // let library = match metadata.r#type {
        //     GridMetadataType::Show => Ok(self.tv_library_id),
        //     GridMetadataType::Movie => Ok(self.film_library_id),
        //     _ => Err(ManagerError::Unimplemented("Show has unknown media type".into()))
        // }?;

        let media_template = templates.first()
            .ok_or_else(|| ManagerError::from_unknown_plex_error("Subscription template has no media"))?;
        let hints = &media_template.parameters.hints;
        let params = &media_template.parameters.params;

        let target_library = match media_template.r#type {
            1 => &self.film_library_id,
            _ => &self.tv_library_id,
        };

        let sub = Subscription {
            prefs: SubscriptionPrefs {
                min_video_quality: media_template.setting_default("minVideoQuality")?,
                replace_lower_quality: media_template.setting_default("replaceLowerQuality")?,
                record_partials: media_template.setting_default("recordPartials")?,
                start_offset_minutes: 0,
                end_offset_minutes: 4,
                lineup_channel: media.channel_identifier.clone(),
                start_timeslot: media.begins_at,
                comskip_enabled: media_template.setting_default("comskipEnabled")?,
                comskip_method: media_template.setting_default("comskipMethod")?,
                one_shot: media_template.setting_default("oneShot")?,
                remote_media: media_template.setting_default("remoteMedia")?,
            },
            hints: hints.clone(),
            params: params.clone(),
            target_library_section_id: target_library.clone(),
            target_library_location_id: target_library.clone(),
            include_grabs: 1,
        };

        println!("{:#?}", sub);

        todo!()
    }

    /// Schedule next recording if close to start time.
    /// If a recording was scheduled, returns time of following recording.
    /// If recording was not scheduled (too far away), returns time of next recording.
    pub async fn schedule_next_recordings(&self) -> Result<DateTime<Utc>> {
        const DATE_FORMAT: &str = "%Y-%m-%d";

        let channels = self.plex.get_channels().await?;

        let now = Utc::now();
        let unix_now = now.timestamp();
        let yesterday = now - Duration::days(1);
        let tomorrow = now + Duration::days(1);

        let all_requests = channels.into_iter().map(|c| {
            let day_requests: Vec<_> = [yesterday, now, tomorrow]
                .iter()
                .map(|d| {
                    // Get shows and delete ones from the past
                    let date = d.clone().format(DATE_FORMAT).to_string();
                    let id = c.id.clone();
                    async move { 
                        let shows = self.plex.get_grid(&id, &date).await?
                            .map_or_else(Vec::new, |s| {
                                s
                                .into_iter()
                                .skip_while(|s| s.begins_at_ts() < unix_now)
                                .collect()
                            });
                        Ok::<Vec<_>, ManagerError>(shows)
                    }
                })
                .collect();

            async move {
                let next_show = try_join_all(day_requests).await?
                    .into_iter()
                    .flatten()
                    .filter(|s| s.subscription_id.is_none() && s.grandparent_subscription_id.is_none())
                    .sorted_by_key(|s| s.begins_at_ts())
                    .next();
                Ok::<_, ManagerError>((c, next_show))
            }
        });

        let next_shows = try_join_all(all_requests).await?;

        let mut next_show: Option<GridMetadata> = None;
        for (_channel, show) in next_shows {
            let unix_now = Utc::now().timestamp();
            if let Some(show) = show {
                let begins_at = show.begins_at_ts();
                if (begins_at - unix_now) < PRE_SCHEDULE_TIME {
                    log::info!("Beginning automatic recording of {}", show.show_title());
                    self.schedule_recording(show).await?;
                } else if let Some(prev_next) = &next_show {
                    if begins_at < prev_next.begins_at_ts() {
                        next_show = Some(show);
                    }
                } else {
                    next_show = Some(show);
                }
            }
        }

        if let Some(show) = &next_show {
            log::info!("Next show is {} due to start at {}", show.show_title(), show.begins_at().unwrap());
        }
        
        Ok(next_show.map_or_else(|| Utc::now() + Duration::hours(1), |s| s.begins_at().unwrap()))
    }

    /// Runs forever, setting everything to record just before it airs
    pub async fn auto_record(&self) -> Result<()> {
        loop {
            let next_time = self.schedule_next_recordings().await?;
            let sleep_time = next_time - Utc::now() - Duration::seconds(PRE_SCHEDULE_TIME);
            log::debug!(
                "Next recording at {}, sleeping for {}",
                next_time,
                sleep_time
            );
            sleep(
                sleep_time
                    .to_std()
                    .unwrap_or_else(|_| std::time::Duration::from_secs(0)),
            )
            .await;
        }
    }
}
