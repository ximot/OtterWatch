use crate::Settings;
use config::{Config, Environment, File};

pub fn load_config() -> Result<Settings, config::ConfigError> {
    let config = Config::builder()
        .add_source(File::with_name("settings"))
        .add_source(Environment::with_prefix("APP"))
        .build()?;

    config.try_deserialize()
}
