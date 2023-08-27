use std::env;
use std::fs::{create_dir_all, File};
use std::io::{Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use log::{debug, error, info};

use clap::Parser;

#[derive(Clone, PartialEq, Eq, Hash, Parser)]
#[command(name = "rsspal")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "aftix (aftix@aftix.xyz)")]
#[command(about = "A Discord bot to turn a server into an RSS reader", long_about = None)]
struct Args {
    // Discord token to use for the Discord API
    #[arg(short, long, default_value_t = String::default())]
    token: String,

    // Location of the configuration file
    #[arg(short, long, default_value_t = String::default())]
    config: String,

    // Location of the data directory
    #[arg(short, long, default_value_t = String::default())]
    data_dir: String,

    // Interval inbetween updates in seconds
    #[arg(short, long)]
    interval: Option<u64>,
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip, default)]
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    #[serde(skip, default)]
    pub discord_token: String,
    // time to wait in seconds between updates
    pub interval: u64,
}

fn get_token() -> anyhow::Result<String> {
    let token = env::var("DISCORD_TOKEN")?;
    Ok(token)
}

fn get_data_dir() -> PathBuf {
    let xdg_data_dir = env::var("XDG_DATA_HOME");
    if let Ok(dir) = xdg_data_dir {
        let path = PathBuf::from(dir).join("rsspal");
        return path;
    }

    let home = env::var("HOME");
    if let Ok(dir) = home {
        let path = PathBuf::from(dir).join(".rsspal");
        return path;
    }

    env::current_dir().expect("Can not access current directory")
}

pub fn get_config_path() -> PathBuf {
    let xdg_config_dir = env::var("XDG_CONFIG_HOME");
    if let Ok(dir) = xdg_config_dir {
        let path = PathBuf::from(dir).join("rsspal/config.toml");
        return path;
    }

    let home = env::var("HOME");
    if let Ok(dir) = home {
        let path = PathBuf::from(dir).join(".rsspal/config.toml");
        return path;
    }

    let cwd = env::current_dir().expect("Can not access current directory");
    cwd.join("config.toml")
}

impl Config {
    pub fn new() -> anyhow::Result<Self> {
        let args = Args::parse();

        let config_path = if args.config.is_empty() {
            get_config_path()
        } else {
            PathBuf::from(args.config)
        };
        debug!("config path: {:?}", config_path);

        let mut buf = Vec::new();
        let mut config = if let Ok(mut file) = File::open(&config_path) {
            if let Ok(metadata) = file.metadata() {
                buf.reserve(metadata.len() as usize);
            }
            file.read_to_end(&mut buf)?;

            let config_str = String::from_utf8_lossy(buf.as_slice());
            let mut config = toml::from_str::<Config>(&config_str)?;
            config.config_file = config_path;
            config
        } else {
            Config {
                config_file: config_path.clone(),
                data_dir: get_data_dir(),
                discord_token: String::default(),
                interval: 600,
            }
        };

        // Now override the loaded file with env vars
        if let Ok(data_dir) = env::var("RSSPAL_DATA_DIR") {
            config.data_dir = PathBuf::from(data_dir);
        }

        let env_token = get_token();

        // Now override the config with the cmd line arguments
        if !args.data_dir.is_empty() {
            config.data_dir = PathBuf::from(args.data_dir);
        }

        if let Some(i) = args.interval {
            config.interval = i;
        }

        if let Err(e) = create_dir_all(&config.data_dir) {
            error!("Could not create data directory: {}.", e);
            anyhow::bail!("Could not create data directory: {}.", e);
        };
        if let Some(p) = config.config_file.parent() {
            if let Err(e) = create_dir_all(p) {
                error!("Could not create config directory: {}.", e);
                anyhow::bail!("Could not create config directory: {}.", e);
            }
        }

        if !args.token.is_empty() {
            config.discord_token = args.token;
        } else if let Ok(t) = env_token {
            config.discord_token = t.clone();
        } else {
            anyhow::bail!("no discord token found: {}", env_token.err().unwrap());
        }

        Ok(config)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        info!("Saving configuration file to {:?}", self.config_file);
        let out_str = toml::to_string_pretty(self)?;
        let mut file = File::create(&self.config_file)?;
        file.write_all(out_str.as_bytes())?;
        Ok(())
    }
}
