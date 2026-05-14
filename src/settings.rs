use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

const SETTINGS_FILE_ENV: &str = "GRATE_BOT_SETTINGS_FILE";
const DEFAULT_SETTINGS_FILE: &str = "grate-bot-settings.json";

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
    settings: Arc<RwLock<BotSettings>>,
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new(default_settings_path(), BotSettings::default())
    }
}

impl SettingsStore {
    pub fn new(path: PathBuf, settings: BotSettings) -> Self {
        Self {
            path,
            settings: Arc::new(RwLock::new(settings)),
        }
    }

    pub async fn load_from_env() -> anyhow::Result<Self> {
        let path = settings_path_from_env();
        Self::load(path).await
    }

    pub async fn load(path: PathBuf) -> anyhow::Result<Self> {
        let settings = match tokio::fs::read_to_string(&path).await {
            Ok(contents) => serde_json::from_str(&contents)
                .with_context(|| format!("could not parse {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BotSettings::default(),
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", path.display()));
            }
        };

        Ok(Self::new(path, settings))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn channel(&self, guild_id: u64, family: ChannelFamily) -> Option<u64> {
        let settings = self.settings.read().await;
        let guild = settings.guilds.get(&guild_id.to_string())?;
        family.channel(guild)
    }

    pub async fn set_channel(
        &self,
        guild_id: u64,
        family: ChannelFamily,
        channel_id: u64,
    ) -> anyhow::Result<()> {
        self.update_and_save(|settings| {
            let guild = settings.guild_mut(guild_id);
            family.set_channel(guild, Some(channel_id));
        })
        .await
    }

    pub async fn clear_channel(&self, guild_id: u64, family: ChannelFamily) -> anyhow::Result<()> {
        self.update_and_save(|settings| {
            let guild = settings.guild_mut(guild_id);
            family.set_channel(guild, None);
        })
        .await
    }

    pub async fn hytale_password(&self, guild_id: u64) -> HytalePasswordSettings {
        let settings = self.settings.read().await;
        settings
            .guilds
            .get(&guild_id.to_string())
            .map(|guild| guild.hytale.password.clone())
            .unwrap_or_default()
    }

    pub async fn hytale_cached_public_ip(&self, guild_id: u64) -> Option<String> {
        let settings = self.settings.read().await;
        settings
            .guilds
            .get(&guild_id.to_string())
            .and_then(|guild| guild.hytale.cached_public_ip.clone())
    }

    pub async fn set_hytale_cached_public_ip(
        &self,
        guild_id: u64,
        public_ip: String,
    ) -> anyhow::Result<()> {
        self.update_and_save(|settings| {
            let guild = settings.guild_mut(guild_id);
            guild.hytale.cached_public_ip = Some(public_ip);
        })
        .await
    }

    pub async fn set_hytale_password(
        &self,
        guild_id: u64,
        password: String,
        enabled: bool,
    ) -> anyhow::Result<HytalePasswordSettings> {
        let mut password_settings = HytalePasswordSettings::default();
        self.update_and_save(|settings| {
            let guild = settings.guild_mut(guild_id);
            guild.hytale.password.last_password = Some(password);
            guild.hytale.password.password_enabled = enabled;
            password_settings = guild.hytale.password.clone();
        })
        .await?;
        Ok(password_settings)
    }

    pub async fn set_hytale_password_enabled(
        &self,
        guild_id: u64,
        enabled: bool,
    ) -> anyhow::Result<HytalePasswordSettings> {
        let mut password_settings = HytalePasswordSettings::default();
        self.update_and_save(|settings| {
            let guild = settings.guild_mut(guild_id);
            guild.hytale.password.password_enabled = enabled;
            password_settings = guild.hytale.password.clone();
        })
        .await?;
        Ok(password_settings)
    }

    async fn update_and_save(&self, update: impl FnOnce(&mut BotSettings)) -> anyhow::Result<()> {
        let mut settings = self.settings.write().await;
        let mut next_settings = settings.clone();
        update(&mut next_settings);
        save_and_verify(&self.path, &next_settings).await?;
        *settings = next_settings;
        Ok(())
    }
}

async fn save_and_verify(path: &Path, settings: &BotSettings) -> anyhow::Result<()> {
    let contents = serde_json::to_string_pretty(settings)?;
    atomic_write(path, contents.as_bytes()).await?;
    let reloaded = SettingsStore::load(path.to_path_buf())
        .await
        .with_context(|| format!("could not verify written settings file {}", path.display()))?;
    let reloaded_settings = reloaded.settings.read().await;
    if &*reloaded_settings != settings {
        anyhow::bail!(
            "settings file {} did not match the settings that were just written",
            path.display()
        );
    }
    Ok(())
}

async fn atomic_write(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("could not create {}", parent.display()))?;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("settings path has no file name: {}", path.display()))?;
    let temp_path = path.with_file_name(format!("{file_name}.tmp"));
    tokio::fs::write(&temp_path, contents)
        .await
        .with_context(|| format!("could not write {}", temp_path.display()))?;
    tokio::fs::rename(&temp_path, path)
        .await
        .with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

pub fn settings_path_from_env() -> PathBuf {
    std::env::var(SETTINGS_FILE_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_settings_path)
}

pub fn default_settings_path() -> PathBuf {
    PathBuf::from(DEFAULT_SETTINGS_FILE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelFamily {
    Grateic,
    Hytale,
}

impl ChannelFamily {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grateic => "Grateic",
            Self::Hytale => "Hytale",
        }
    }

    fn channel(self, guild: &GuildSettings) -> Option<u64> {
        match self {
            Self::Grateic => guild.grateic_channel_id,
            Self::Hytale => guild.hytale_channel_id,
        }
    }

    fn set_channel(self, guild: &mut GuildSettings, channel_id: Option<u64>) {
        match self {
            Self::Grateic => guild.grateic_channel_id = channel_id,
            Self::Hytale => guild.hytale_channel_id = channel_id,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BotSettings {
    #[serde(default)]
    pub guilds: BTreeMap<String, GuildSettings>,
}

impl BotSettings {
    fn guild_mut(&mut self, guild_id: u64) -> &mut GuildSettings {
        self.guilds.entry(guild_id.to_string()).or_default()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuildSettings {
    #[serde(default)]
    pub grateic_channel_id: Option<u64>,
    #[serde(default)]
    pub hytale_channel_id: Option<u64>,
    #[serde(default)]
    pub hytale: HytaleSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HytaleSettings {
    #[serde(default)]
    pub password: HytalePasswordSettings,
    #[serde(default)]
    pub cached_public_ip: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HytalePasswordSettings {
    #[serde(default)]
    pub password_enabled: bool,
    #[serde(default)]
    pub last_password: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn unique_temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "grate-bot-{name}-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[test]
    fn default_settings_path_is_repo_local_file() {
        assert_eq!(
            default_settings_path(),
            PathBuf::from("grate-bot-settings.json")
        );
    }

    #[test]
    fn repo_local_settings_file_is_gitignored() {
        assert!(
            include_str!("../.gitignore")
                .lines()
                .any(|line| line.trim() == "grate-bot-settings.json")
        );
    }

    #[test]
    fn settings_path_uses_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var(SETTINGS_FILE_ENV).ok();
        unsafe { std::env::set_var(SETTINGS_FILE_ENV, "/tmp/custom-grate-settings.json") };

        assert_eq!(
            settings_path_from_env(),
            PathBuf::from("/tmp/custom-grate-settings.json")
        );

        match previous {
            Some(value) => unsafe { std::env::set_var(SETTINGS_FILE_ENV, value) },
            None => unsafe { std::env::remove_var(SETTINGS_FILE_ENV) },
        }
    }

    #[tokio::test]
    async fn load_missing_file_uses_defaults_and_save_persists_channels() {
        let path = unique_temp_path("missing");
        let _ = tokio::fs::remove_file(&path).await;

        let store = SettingsStore::load(path.clone()).await.unwrap();
        assert_eq!(store.channel(1, ChannelFamily::Grateic).await, None);

        store
            .set_channel(1, ChannelFamily::Grateic, 10)
            .await
            .unwrap();
        let reloaded = SettingsStore::load(path.clone()).await.unwrap();
        assert_eq!(reloaded.channel(1, ChannelFamily::Grateic).await, Some(10));

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn load_rejects_corrupted_file() {
        let path = unique_temp_path("corrupt");
        tokio::fs::write(&path, "{not json").await.unwrap();

        let error = SettingsStore::load(path.clone()).await.unwrap_err();
        assert!(format!("{error:#}").contains("could not parse"));

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn failed_save_does_not_update_memory() {
        let path =
            std::env::temp_dir().join(format!("grate-bot-settings-dir-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&path).await;
        tokio::fs::create_dir_all(&path).await.unwrap();
        let store = SettingsStore::new(path.clone(), BotSettings::default());

        let error = store
            .set_channel(1, ChannelFamily::Grateic, 10)
            .await
            .unwrap_err();

        assert!(format!("{error:#}").contains("could not replace"));
        assert_eq!(store.channel(1, ChannelFamily::Grateic).await, None);

        let _ = tokio::fs::remove_dir_all(&path).await;
    }

    #[tokio::test]
    async fn stores_hytale_password_settings() {
        let path = unique_temp_path("password");
        let _ = tokio::fs::remove_file(&path).await;
        let store = SettingsStore::load(path.clone()).await.unwrap();

        let password = store
            .set_hytale_password(1, "secret".to_owned(), true)
            .await
            .unwrap();
        assert_eq!(password.last_password.as_deref(), Some("secret"));
        assert!(password.password_enabled);

        let password = store.set_hytale_password_enabled(1, false).await.unwrap();
        assert_eq!(password.last_password.as_deref(), Some("secret"));
        assert!(!password.password_enabled);

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn stores_cached_hytale_public_ip() {
        let path = unique_temp_path("public-ip");
        let _ = tokio::fs::remove_file(&path).await;
        let store = SettingsStore::load(path.clone()).await.unwrap();

        assert_eq!(store.hytale_cached_public_ip(1).await, None);
        store
            .set_hytale_cached_public_ip(1, "203.0.113.10".to_owned())
            .await
            .unwrap();
        let reloaded = SettingsStore::load(path.clone()).await.unwrap();
        assert_eq!(
            reloaded.hytale_cached_public_ip(1).await.as_deref(),
            Some("203.0.113.10")
        );

        let _ = tokio::fs::remove_file(&path).await;
    }
}
