use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use log::error;

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
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FileConfig {
    pub id: String,
    pub path: String,
    #[serde(default, rename = "loop")]
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

pub fn load_config(path: &str) -> Result<FlattenedConfig, Box<dyn std::error::Error>> {
    
    if !Path::new(path).exists() {
        return Err(format!("Config file not found: {}", path).into());
    }
    
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read config file {}: {}", path, e))?;
    
    let cfg: Config = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse TOML config {}: {}", path, e))?;
    
    log::info!("Config loaded from: {}", path);
    
    // Log what sources will be processed
    let total_sources = cfg.sources.iter()
        .map(|g| g.lrng.len() + g.file.len())
        .sum::<usize>();
    log::info!("Found {} total sources in config", total_sources);

    // Flatten groups
    let mut combine = CombineMode::Xor;
    let mut lrng_sources = Vec::new();
    let mut file_sources = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    for group in cfg.sources.into_iter() {
        if let Some(c) = group.combine.as_deref() {
            if c.eq_ignore_ascii_case("xor") {
                combine = CombineMode::Xor;
            }
        }
        for s in group.lrng.into_iter().filter(|s| s.enabled) {
            if !is_valid_id(&s.id) {
                error!("Invalid source id '{}'. Use [a-z0-9][a-z0-9_-]*", s.id);
                continue;
            }
            if !seen_ids.insert(s.id.clone()) {
                error!("Duplicate source id '{}' - skipping", s.id);
                continue;
            }
            lrng_sources.push(s);
        }
        for s in group.file.into_iter().filter(|s| s.enabled) {
            if !is_valid_id(&s.id) {
                error!("Invalid source id '{}'. Use [a-z0-9][a-z0-9_-]*", s.id);
                continue;
            }
            if !seen_ids.insert(s.id.clone()) {
                error!("Duplicate source id '{}' - skipping", s.id);
                continue;
            }
            file_sources.push(s);
        }
    }

    log::info!("Enabled sources: {} lrng, {} file", lrng_sources.len(), file_sources.len());
    
    let total_enabled = lrng_sources.len() + file_sources.len();
    if total_enabled == 0 {
        log::warn!("No enabled entropy sources found in config - service will fail on requests");
    } else if total_enabled == 1 {
        log::warn!("Only one entropy source enabled - consider enabling multiple sources for better security");
    }
    
    Ok(FlattenedConfig { combine, lrng_sources, file_sources })
}

fn is_valid_id(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_lc_alnum(c) => {},
        _ => return false,
    }
    for c in chars {
        if !(is_lc_alnum(c) || c == '-' || c == '_') { return false; }
    }
    true
}

fn is_lc_alnum(c: char) -> bool {
    matches!(c, 'a'..='z' | '0'..='9')
}

