use crate::Settings;
use config::{Config, File};

// fn load_config() -> Settings {
//     let config_file = fs::read_to_string("settings.toml").expect("Error loading config file");
//     let config: Settings = toml::from_str(&config_file).expect("Wrong config file format!");
//     config
// }

pub fn load_confg() -> rusqlite::Result<Settings, config::ConfigError> {
    let mut settings = Config::default();
    settings
        .merge(File::with_name("settings"))?
        .merge(config::Environment::with_prefix("APP"))?;
    settings.try_into()
}
