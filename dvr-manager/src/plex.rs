use chrono::{DateTime, Utc, TimeZone};
use derive_builder::Builder;
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use serde_xml_rs::from_str;
use tokio::sync::Semaphore;
use std::{future::Future, sync::Arc, ops::Deref};
use async_trait::async_trait;

const PREFS_PATH: &str = "/config/Library/Application Support/Plex Media Server/Preferences.xml";

#[derive(Debug, thiserror::Error)]
pub enum PlexError {
    #[error("Failed to request data from Plex: {0}")]
    PlexRequest(#[from] reqwest::Error),

    #[error("Couldn't parse Plex response: {0}")]
    PlexXmlResponse(#[from] serde_xml_rs::Error),

    #[error("Couldn't parse Plex response: {0}")]
    PlexJsonResponse(#[from] serde_json::Error),

    #[error("Couldn't parse Plex response: {0}")]
    PlexEncodedResponse(#[from] serde_qs::Error),

    #[error("Couldn't parse Plex response: {0}")]
    PlexResponse(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = PlexError> = std::result::Result<T, E>;

#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateParameters {
    pub hints: SubscriptionHints,
    pub params: SubscriptionParams,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct TemplateSetting {
    id: String,
    default: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateSubscription<T> {
    pub parameters: T,
    pub r#type: i16,
    #[serde(rename="targetSectionLocationID")]
    pub target_section_location_id: i16,
    #[serde(rename="Setting")]
    pub setting: Vec<TemplateSetting>
}

impl<T> TemplateSubscription<T> {

    pub fn setting_default(&self, id: &str) -> Result<String> {
        let val = self.setting
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| PlexError::PlexResponse(format!("Setting {} not found", id)))?
            .default
            .clone();
        Ok(val)
    }

}

#[derive(Debug, Serialize, Deserialize)]
struct TemplateTemplate {
    #[serde(rename = "MediaSubscription")]
    media_subscription: Vec<TemplateSubscription<String>>
}

#[derive(Debug, Serialize, Deserialize)]
struct TemplateContainer {
    #[serde(rename = "SubscriptionTemplate")]
    subscription_template: Vec<TemplateTemplate>
}

#[derive(Debug, Serialize, Deserialize)]
struct TemplateResponse {
    #[serde(rename = "MediaContainer")]
    media_container: TemplateContainer
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Preferences {
    plex_online_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ChannelResponse {
    media_container: ChannelContainer,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ChannelContainer {
    pub channel: Vec<Channel>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GridResponse {
    media_container: GridContainer,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GridContainer {
    metadata: Option<Vec<GridMetadata>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridMetadataType {
    Movie,
    Show,
    #[serde(other)]
    Other,
}

impl From<GridMetadataType> for u8 {
    fn from(g: GridMetadataType) -> Self {
        ((g as usize) + 1) as u8
    }
}


#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridMetadata {
    pub rating_key: String,
    pub guid: String,
    pub title: String,
    pub grandparent_guid: Option<String>,
    pub grandparent_title: Option<String>,
    pub parent_guid: Option<String>,
    pub parent_title: Option<String>,
    pub parent_index: Option<u64>,
    pub index: Option<u64>,
    pub r#type: GridMetadataType,
    pub duration: u32,
    pub on_air: Option<bool>,
    #[serde(rename = "subscriptionID")]
    pub subscription_id: Option<String>,
    pub subscription_type: Option<String>,
    #[serde(rename = "grandparentSubscriptionID")]
    pub grandparent_subscription_id: Option<String>,
    pub grandparent_subscription_type: Option<String>,
    pub grandparent_thumb: Option<String>,
    pub originally_available_at: String,
    #[serde(rename = "Media")]
    pub media: Vec<GridMedia>,
}

impl GridMetadata {
    pub fn begins_at_ts(&self) -> i64 {
        self.media.first().map_or(0, |m| m.begins_at)
    }
    
    pub fn begins_at(&self) -> Option<DateTime<Utc>> {
        self.media.first().map(|m| Utc.timestamp(m.begins_at, 0))
    }

    pub fn show_title(&self) -> String {
        let gt = &self.grandparent_title;
        gt.clone().unwrap_or_else(|| self.title.clone())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridMedia {
    pub id: u64,
    pub begins_at: i64,
    pub ends_at: i64,
    pub channel_identifier: String,
    pub channel_title: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ProviderDirectoryType {
    Movie,
    Show,
    #[serde(other)]
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDirectory {
    pub r#type: Option<ProviderDirectoryType>,
    pub id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersFeature {
    key: Option<String>,
    r#type: String,
    #[serde(rename = "Directory")]
    directory: Option<Vec<ProviderDirectory>>
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersMediaProvider {
    pub identifier: String,
    pub title: String,
    #[serde(rename = "Feature")]
    pub feature: Vec<ProvidersFeature>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersContainer {
    #[serde(rename = "MediaProvider")]
    media_provider: Vec<ProvidersMediaProvider>
}

pub trait ProvidersMediaProviders {
    fn get_dirs_of_type(&self, dir_type: ProviderDirectoryType) -> Result<Vec<ProviderDirectory>, PlexError>;
}

impl ProvidersMediaProviders for Vec<ProvidersMediaProvider> {
    fn get_dirs_of_type(&self, dir_type: ProviderDirectoryType) -> Result<Vec<ProviderDirectory>, PlexError> {
        let dirs = self
            .iter()
            .find(|p| p.identifier == "com.plexapp.plugins.library")
            .ok_or_else(|| PlexError::PlexResponse("Plex is missing its library".into()))?
            .feature
            .first()
            .ok_or_else(|| PlexError::PlexResponse("Plex library has no features".into()))?
            .clone()
            .directory
            .ok_or_else(|| PlexError::PlexResponse("Plex library has no dirs".into()))?
            .iter()
            .filter(|d| 
                d.r#type.clone().map_or(false, |t| t == dir_type))
            .cloned()
            .collect::<Vec<_>>();
        Ok(dirs)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProvidersResponse {
    #[serde(rename="MediaContainer")]
    media_container: ProvidersContainer
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPrefs {
    pub min_video_quality: String,
    pub replace_lower_quality: String,
    pub record_partials: String,
    pub start_offset_minutes: u8,
    pub end_offset_minutes: u8,
    pub lineup_channel: String,
    pub start_timeslot: i64,
    pub comskip_enabled: String,
    pub comskip_method: String,
    pub one_shot: String,
    pub remote_media: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionHints {
    pub grandparent_guid: Option<String>,
    pub grandparent_thumb: Option<String>,
    pub grandparent_title: Option<String>,
    pub guid: String,
    pub index: Option<String>,
    pub originally_available_at: Option<String>,
    pub parent_guid: Option<String>,
    pub parent_index: Option<String>,
    pub parent_title: Option<String>,
    pub rating_key: String,
    pub title: String,
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionParams {
    pub airing_channels: String,
    pub airing_times: String,
    pub library_type: String,      // 2 = tv show?
    #[serde(rename="mediaProviderID")]
    pub media_provider_id: String, // ??
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub prefs: SubscriptionPrefs,
    pub hints: SubscriptionHints,
    pub params: SubscriptionParams,
    #[serde(rename = "targetLibrarySectionID")]
    pub target_library_section_id: String,
    #[serde(rename = "targetLibraryLocationID")]
    pub target_library_location_id: String,
    pub include_grabs: i8,
}

#[async_trait]
trait RequestBuilderLimited {
    async fn send_limited(self, limit: Arc<Semaphore>) -> Result<reqwest::Response, reqwest::Error>;
}

#[async_trait]
impl RequestBuilderLimited for RequestBuilder {
    async fn send_limited(self, limit: Arc<Semaphore>) -> Result<reqwest::Response, reqwest::Error> {
        let _permit = limit.acquire_owned().await.unwrap();
        self.send().await
    }
}

pub enum PlexHost {
    Localhost,
    Custom(String),
}
pub struct Plex {
    token: String,
    client: reqwest::Client,
    req_limit: Arc<Semaphore>,
    host: String,
}

impl Plex {
    pub fn new(prefs_path: Option<String>, host: PlexHost) -> Result<Plex> {
        let prefs_path = prefs_path.unwrap_or_else(|| PREFS_PATH.to_string());
        let prefs_str = std::fs::read_to_string(prefs_path)?;
        let prefs: Preferences = from_str(&prefs_str)?;

        log::debug!("Prefs: {:?}", prefs);

        let client = reqwest::Client::new();

        Ok(Plex {
            token: prefs.plex_online_token,
            host: match host {
                PlexHost::Localhost => "http://localhost:32400".to_string(),
                PlexHost::Custom(host) => host,
            },
            client,
            req_limit: Arc::new(Semaphore::new(5)),
        })
    }

    pub fn get(&self, resource: &str) -> RequestBuilder {
        self.client
            .get(&format!("{}/{}", self.host, resource))
            .query(&[("X-Plex-Token", &self.token)])
            .header("accept", "application/json")
    }

    pub fn post(&self, resource: &str) -> RequestBuilder {
        self.client
            .post(&format!("{}/{}", self.host, resource))
            .query(&[("X-Plex-Token", &self.token)])
            .header("accept", "application/json")
    }

    pub async fn get_providers(&self) -> Result<Vec<ProvidersMediaProvider>> {
        const RESOURCE: &str = "media/providers";
        let providers: ProvidersResponse = self.get(RESOURCE).send_limited(self.req_limit.clone()).await?.json().await?;
        Ok(providers.media_container.media_provider)
    }

    pub async fn get_channels(&self) -> Result<Vec<Channel>> {
        const RESOURCE: &str = "tv.plex.providers.epg.xmltv:2/lineups/dvr/channels";
        let container: ChannelResponse = self.get(RESOURCE).send_limited(self.req_limit.clone()).await?.json().await?;
        Ok(container.media_container.channel)
    }

    pub async fn get_grid(&self, channel_grid_key: &str, date: &str) -> Result<Option<Vec<GridMetadata>>> {
        const RESOURCE: &str = "tv.plex.providers.epg.xmltv:2/grid";
        let container: GridResponse = self
            .get(RESOURCE)
            .query(&[("channelGridKey", channel_grid_key), ("date", date)])
            .send_limited(self.req_limit.clone())
            .await?
            .json()
            .await?;
        Ok(container.media_container.metadata)
    }

    pub async fn get_subscription_template(&self, guid: &str) -> Result<Vec<TemplateSubscription<TemplateParameters>>> {
        const RESOURCE: &str = "media/subscriptions/template";
        
        let template_response: TemplateResponse = self
            .get(RESOURCE)
            // .header("accept", "text/plain") // json not supported
            .query(&[("guid", guid)])
            .send_limited(self.req_limit.clone())
            .await?
            .json()
            .await?;

        // println!("{}", response_text);
        
        // let template_response: TemplateResponse = serde_json::from_str(&response_text)?;

        template_response
            .media_container
            .subscription_template
            .into_iter()
            .next()
            .ok_or_else(|| PlexError::PlexResponse("Expected single SubscriptionTemplate body".into()))?
            .media_subscription
            .into_iter()
            .map(|s| {
                let decoded = urlencoding::decode(&s.parameters)
                    .map_err(|_| PlexError::PlexResponse("Couldn't decode parameters".into()))?;
                let ts = TemplateSubscription::<TemplateParameters> {
                    parameters: serde_qs::from_str(&decoded)?,
                    r#type: s.r#type,
                    target_section_location_id: s.target_section_location_id,
                    setting: s.setting
                };
                Ok::<_, PlexError>(ts)
            })
            .collect()
    }

    pub async fn create_subscription(&self, subscription: &Subscription) -> Result<()> {
        const RESOURCE: &str = "media/subscriptions";
        let query = serde_qs::to_string(subscription).expect("subscription is not serializable");

        self.post(&format!("{}?{}", RESOURCE, query))
            .send_limited(self.req_limit.clone())
            .await?;

        Ok(())
    }

}
