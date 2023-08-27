use std::fs::File;

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
        Ok(cfg) => cfg.data_dir.join("database.json"),
    };

    info!("Loading database from {:?}", db_path);
    if !try_exists(&db_path).await? {
        warn!("{:?} does not exist, using an empty feed vector.", db_path);
        return Ok(Vec::new());
    }

    serde_json::from_reader(File::open(&db_path)?)
        .map_err(|e| anyhow::anyhow!("error reading JSON: {}", e))
}

pub async fn export(feeds: &Vec<Feed>) -> anyhow::Result<()> {
    let db_path = match CONFIG.read() {
        Err(e) => anyhow::bail!("error reading CONFIG static: {}", e),
        Ok(cfg) => cfg.data_dir.join("database.json"),
    };

    info!("Writing database to {:?}", db_path);
    let file = File::create(&db_path)?;
    serde_json::to_writer_pretty(file, feeds)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum Feed {
    RSS(RssFeed),
    ATOM(AtomFeed),
}

impl Feed {
    pub fn description(&self) -> String {
        match self {
            Self::RSS(rss) => rss.channel.description.clone(),
            Self::ATOM(atom) => atom.subtitle.clone().unwrap_or_else(String::default),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Self::RSS(rss) => rss.channel.title.clone(),
            Self::ATOM(atom) => atom.title.clone(),
        }
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        match self {
            Self::RSS(ref mut rss) => rss.channel.title = title.into(),
            Self::ATOM(ref mut atom) => atom.title = title.into(),
        };
    }

    pub fn url(&self) -> String {
        match self {
            Self::RSS(rss) => rss.channel.url.clone(),
            Self::ATOM(atom) => atom.url.clone(),
        }
    }

    pub fn set_url(&mut self, url: impl Into<String>) {
        match self {
            Self::RSS(ref mut rss) => rss.channel.url = url.into(),
            Self::ATOM(ref mut atom) => atom.url = url.into(),
        };
    }

    pub fn should_update(&self) -> bool {
        match self {
            Self::RSS(rss) => rss.should_update(),
            Self::ATOM(atom) => atom.should_update(),
        }
    }

    pub fn discord_category(&self) -> Option<String> {
        match self {
            Self::RSS(rss) => rss.channel.discord_category.clone(),
            Self::ATOM(atom) => atom.discord_category.clone(),
        }
    }

    pub fn set_discord_category(&mut self, url: &Option<String>) {
        match self {
            Self::RSS(ref mut rss) => rss.channel.discord_category = url.clone(),
            Self::ATOM(ref mut atom) => atom.discord_category = url.clone(),
        };
    }
}

impl Default for Feed {
    fn default() -> Self {
        Self::RSS(RssFeed::default())
    }
}

pub fn from_url(
    url: impl AsRef<str>,
    title: Option<String>,
    category: Option<String>,
) -> anyhow::Result<Feed> {
    info!("Retrieving feed from url {}", url.as_ref());

    let mut feed = match AtomFeed::from_url(&url) {
        Ok(f) => Ok(Feed::ATOM(f)),
        _ => RssFeed::from_url(&url).map(Feed::RSS),
    }?;

    match &mut feed {
        Feed::RSS(rss) => {
            if let Some(title) = title {
                rss.channel.title = title;
            }
            rss.channel.discord_category = category;
        }
        Feed::ATOM(atom) => {
            if let Some(title) = title {
                atom.title = title;
            }
            atom.discord_category = category;
        }
    };

    Ok(feed)
}

fn is_image_mime_type(mime: impl AsRef<str>) -> bool {
    match mime.as_ref() {
        "image/jpeg" | "image/jpg" | "image/png" | "image/gif" => true,
        _ => false,
    }
}
