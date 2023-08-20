use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use clap::Parser;

#[derive(Clone, PartialEq, Eq, Hash, Parser)]
#[command(version = env!("CARGO_PKG_VERSION"))]
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
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip, default)]
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    #[serde(skip, default)]
    pub discord_token: String,
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

    let cwd = env::current_dir().expect("Can not access current directory");
    PathBuf::from(cwd)
}

pub fn get_config_path() -> PathBuf {
    let xdg_config_dir = env::var("XDG_CONFIG_HOME");
    if let Ok(dir) = xdg_config_dir {
        let path = PathBuf::from(dir).join("rsspal");
        return path;
    }

    let home = env::var("HOME");
    if let Ok(dir) = home {
        let path = PathBuf::from(dir).join(".rsspal");
        return path;
    }

    let cwd = env::current_dir().expect("Can not access current directory");
    PathBuf::from(cwd)
}

impl Config {
    pub fn new() -> anyhow::Result<Self> {
        let args = Args::parse();

        let config_path = if args.config == "" {
            get_config_path()
        } else {
            PathBuf::from(args.config)
        };
        println!("config path: {:?}", config_path);

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
            }
        };

        // Now override the loaded file with env vars
        if let Ok(data_dir) = env::var("RSSPAL_DATA_DIR") {
            config.data_dir = PathBuf::from(data_dir);
        }

        let env_token = get_token();

        // Now override the config with the cmd line arguments
        if args.data_dir != "" {
            config.data_dir = PathBuf::from(args.data_dir);
        }

        if args.token != "" {
            config.discord_token = args.token;
        } else if let Ok(t) = env_token {
            config.discord_token = t.clone();
        } else {
            anyhow::bail!("no discord token found: {}", env_token.err().unwrap());
        }

        Ok(config)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let out_str = toml::to_string_pretty(self)?;
        let mut file = File::create(&self.config_file)?;
        file.write_all(out_str.as_bytes())?;
        Ok(())
    }
}
