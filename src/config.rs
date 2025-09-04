use serde::Deserialize;
use std::fs;
use std::path::Path;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/trng-dbus/config.toml";

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Config {
    #[serde(default)]
    pub sources: Vec<SourcesGroup>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SourcesGroup {
    #[serde(default)]
    pub combine: Option<String>,
    #[serde(default)]
    pub lrng: Vec<LrngConfig>,
    #[serde(default)]
    pub file: Vec<FileConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LrngConfig {
    pub key: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FileConfig {
    pub key: Option<String>,
    pub path: String,
    #[serde(default)]
    pub loop_: Option<bool>,
    #[serde(default)]
    pub enabled: bool,
}

pub enum CombineMode {
    Xor,
}

pub struct FlattenedConfig {
    pub combine: CombineMode,
    pub lrng_sources: Vec<LrngConfig>,
    pub file_sources: Vec<FileConfig>,
}

pub fn load_config(path: Option<&str>) -> FlattenedConfig {
    let path = path.unwrap_or(DEFAULT_CONFIG_PATH);
    let cfg: Config = if Path::new(path).exists() {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|_| String::new());
        toml::from_str(&content).unwrap_or_default()
    } else {
        Config::default()
    };

    // Flatten groups
    let mut combine = CombineMode::Xor;
    let mut lrng_sources = Vec::new();
    let mut file_sources = Vec::new();
    for group in cfg.sources.into_iter() {
        if let Some(c) = group.combine.as_deref() {
            if c.eq_ignore_ascii_case("xor") {
                combine = CombineMode::Xor;
            }
        }
        lrng_sources.extend(group.lrng.into_iter().filter(|s| s.enabled));
        file_sources.extend(group.file.into_iter().filter(|s| s.enabled));
    }

    FlattenedConfig { combine, lrng_sources, file_sources }
}

