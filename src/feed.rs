use std::fs::File;

use flate2::write::{GzDecoder, GzEncoder};
use flate2::Compression;

use log::{info, warn};

use serde::{Deserialize, Serialize};

use tokio::fs::try_exists;

pub mod atom;
pub mod rss;

use crate::CONFIG;
use atom::AtomFeed;
use rss::RssFeed;

pub async fn import() -> anyhow::Result<Vec<Feed>> {
    let db_path = match CONFIG.read() {
        Err(e) => anyhow::bail!("error reading CONFIG static: {}", e),
        Ok(cfg) => cfg.data_dir.join("database.json.gz"),
    };

    info!("Loading database from {:?}", db_path);
    if !try_exists(&db_path).await? {
        warn!("{:?} does not exist, using an empty feed vector.", db_path);
        return Ok(Vec::new());
    }

    serde_json::from_reader(GzDecoder::new(File::open(&db_path)?))
        .map_err(|e| anyhow::anyhow!("error reading JSON: {}", e))
}

pub async fn export(feeds: &[Feed]) -> anyhow::Result<()> {
    let db_path = match CONFIG.read() {
        Err(e) => anyhow::bail!("error reading CONFIG static: {}", e),
        Ok(cfg) => cfg.data_dir.join("database.json.gz"),
    };

    info!("Writing database to {:?}", db_path);
    let file = File::create(&db_path)?;
    serde_json::to_writer_pretty(GzEncoder::new(file, Compression::best()), &Vec::from(feeds))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum Feed {
    Rss(RssFeed),
    Atom(AtomFeed),
}

impl Feed {
    pub fn description(&self) -> String {
        match self {
            Self::Rss(rss) => rss.channel.description.clone(),
            Self::Atom(atom) => atom.subtitle.clone().unwrap_or_default(),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Self::Rss(rss) => rss.channel.title.clone(),
            Self::Atom(atom) => atom.title.clone(),
        }
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        match self {
            Self::Rss(ref mut rss) => rss.channel.title = title.into(),
            Self::Atom(ref mut atom) => atom.title = title.into(),
        };
    }

    pub fn url(&self) -> String {
        match self {
            Self::Rss(rss) => rss.channel.url.clone(),
            Self::Atom(atom) => atom.url.clone(),
        }
    }

    pub fn set_url(&mut self, url: impl Into<String>) {
        match self {
            Self::Rss(ref mut rss) => rss.channel.url = url.into(),
            Self::Atom(ref mut atom) => atom.url = url.into(),
        };
    }

    pub fn should_update(&self) -> bool {
        match self {
            Self::Rss(rss) => rss.should_update(),
            Self::Atom(atom) => atom.should_update(),
        }
    }

    pub fn discord_category(&self) -> Option<String> {
        match self {
            Self::Rss(rss) => rss.channel.discord_category.clone(),
            Self::Atom(atom) => atom.discord_category.clone(),
        }
    }

    pub fn set_discord_category(&mut self, url: &Option<String>) {
        match self {
            Self::Rss(ref mut rss) => rss.channel.discord_category = url.clone(),
            Self::Atom(ref mut atom) => atom.discord_category = url.clone(),
        };
    }
}

impl Default for Feed {
    fn default() -> Self {
        Self::Rss(RssFeed::default())
    }
}

pub fn from_url(
    url: impl AsRef<str>,
    title: Option<String>,
    category: Option<String>,
) -> anyhow::Result<Feed> {
    info!("Retrieving feed from url {}", url.as_ref());

    let mut feed = match AtomFeed::from_url(&url) {
        Ok(f) => Ok(Feed::Atom(f)),
        _ => RssFeed::from_url(&url).map(Feed::Rss),
    }?;

    match &mut feed {
        Feed::Rss(rss) => {
            if let Some(title) = title {
                rss.channel.title = title;
            }
            rss.channel.discord_category = category;
        }
        Feed::Atom(atom) => {
            if let Some(title) = title {
                atom.title = title;
            }
            atom.discord_category = category;
        }
    };

    Ok(feed)
}

fn is_image_mime_type(mime: impl AsRef<str>) -> bool {
    matches!(
        mime.as_ref(),
        "image/jpeg" | "image/jpg" | "image/png" | "image/gif"
    )
}
