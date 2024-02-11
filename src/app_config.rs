use crate::Settings;
use config::{Config, File};

pub fn load_config() -> rusqlite::Result<Settings, config::ConfigError> {
    let mut settings = Config::default();
    settings
        .merge(File::with_name("settings"))?
        .merge(config::Environment::with_prefix("APP"))?;
    settings.try_into()
}
