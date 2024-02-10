use crate::Settings;
use std::fs;

fn load_config() -> Settings {
    let config_file = fs::read_to_string("Settings.toml").expect("Error loading config file");
    let config: Settings = toml::from_str(&config_file).expect("Wrong config file format!");
    config
}
