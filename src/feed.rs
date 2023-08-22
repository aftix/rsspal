use chrono::{DateTime, Utc};
use std::path::PathBuf;

use typestate::typestate;

use serde::{Deserialize, Serialize};

use tokio::fs::{try_exists, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub mod atom;
pub mod rss;

use atom::AtomFeed;
use rss::RssFeed;

pub async fn import(data_dir: &PathBuf) -> anyhow::Result<Vec<Feed>> {
    let db_path = data_dir.join("database.toml");
    if !try_exists(&db_path).await? {
        return Ok(Vec::new());
    }

    let mut file = File::open(&db_path).await?;
    let mut s = String::new();
    file.read_to_string(&mut s).await?;

    Ok(toml::from_str(&s)?)
}

pub async fn export(data_dir: &PathBuf, feeds: &Vec<Feed>) -> anyhow::Result<()> {
    let db_path = data_dir.join("database.toml");
    let out_str = toml::to_string(feeds)?;

    let mut file = File::create(&db_path).await?;
    file.write_all(out_str.as_bytes()).await?;
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

    pub fn last_updated(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::RSS(rss) => rss.channel.last_updated.clone(),
            Self::ATOM(atom) => atom.last_updated.clone(),
        }
    }

    pub fn should_update(&self) -> bool {
        match self {
            Self::RSS(rss) => rss.should_update(),
            Self::ATOM(atom) => atom.should_update(),
        }
    }
}

impl Default for Feed {
    fn default() -> Self {
        Self::RSS(RssFeed::default())
    }
}

// Struct for reading/writing feed configurations
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct FeedItem {
    category: Option<String>,
    url: String,
    title: String,
    description: String,
    #[serde(skip, default)]
    feed: Option<Feed>,
}

// Typestate stuff for the RssFeed builder api
mod seal {
    pub trait Seal {}
}

#[typestate]
mod feed_builder {
    #[derive(Debug, Clone, Hash, PartialEq, Eq)]
    #[automaton]
    pub struct FeedBuilder {
        pub category: Option<String>,
        pub url: String,
        pub title: String,
        pub description: String,
    }

    #[state]
    pub struct Start;
    #[state]
    pub struct Url;
    #[state]
    pub struct Title;
    #[state]
    pub struct Description;
    #[state]
    pub struct UrlTitle;
    #[state]
    pub struct UrlDescription;
    #[state]
    pub struct TitleDescription;
    #[state]
    pub struct End;

    pub trait Start {
        fn build() -> Start;

        fn url(self, url: impl Into<String>) -> Url;
        fn title(self, title: impl Into<String>) -> Title;
        fn description(self, description: impl Into<String>) -> Description;

        fn category(self, category: impl Into<String>) -> Start;
    }

    impl StartState for FeedBuilder<Start> {
        fn build() -> Self {
            Self {
                category: None,
                url: String::default(),
                title: String::default(),
                description: String::default(),
                state: Start,
            }
        }

        fn url(self, url: impl Into<String>) -> FeedBuilder<Url> {
            FeedBuilder::<Url> {
                category: self.category,
                url: url.into(),
                title: self.title,
                description: self.description,
                state: Url,
            }
        }

        fn description(self, description: impl Into<String>) -> FeedBuilder<Description> {
            FeedBuilder::<Description> {
                category: self.category,
                url: self.url,
                title: self.title,
                description: description.into(),
                state: Description,
            }
        }

        fn title(self, title: impl Into<String>) -> FeedBuilder<Title> {
            FeedBuilder::<Title> {
                category: self.category,
                url: self.url,
                title: title.into(),
                description: self.description,
                state: Title,
            }
        }

        fn category(mut self, category: impl Into<String>) -> Self {
            self.category = Some(category.into());
            self
        }
    }

    pub trait End {
        fn category(self, category: impl Into<String>) -> End;
        fn create(self) -> super::FeedItem;
    }

    impl EndState for FeedBuilder<End> {
        fn category(mut self, category: impl Into<String>) -> Self {
            self.category = Some(category.into());
            self
        }

        fn create(self) -> super::FeedItem {
            super::FeedItem {
                category: self.category,
                url: self.url,
                title: self.title,
                description: self.description,
                feed: None,
            }
        }
    }

    pub trait Url {
        fn category(self, category: impl Into<String>) -> Url;
        fn title(self, title: impl Into<String>) -> UrlTitle;
        fn description(self, description: impl Into<String>) -> UrlDescription;
    }

    impl UrlState for FeedBuilder<Url> {
        fn category(mut self, category: impl Into<String>) -> Self {
            self.category = Some(category.into());
            self
        }

        fn title(self, title: impl Into<String>) -> FeedBuilder<UrlTitle> {
            FeedBuilder::<UrlTitle> {
                category: self.category,
                url: self.url,
                title: title.into(),
                description: self.description,
                state: UrlTitle,
            }
        }

        fn description(self, description: impl Into<String>) -> FeedBuilder<UrlDescription> {
            FeedBuilder::<UrlDescription> {
                category: self.category,
                url: self.url,
                title: self.title,
                description: description.into(),
                state: UrlDescription,
            }
        }
    }

    pub trait Title {
        fn category(self, category: impl Into<String>) -> Title;
        fn url(self, url: impl Into<String>) -> UrlTitle;
        fn description(self, description: impl Into<String>) -> TitleDescription;
    }

    impl TitleState for FeedBuilder<Title> {
        fn category(mut self, category: impl Into<String>) -> Self {
            self.category = Some(category.into());
            self
        }

        fn url(self, url: impl Into<String>) -> FeedBuilder<UrlTitle> {
            FeedBuilder::<UrlTitle> {
                category: self.category,
                url: url.into(),
                title: self.title,
                description: self.description,
                state: UrlTitle,
            }
        }

        fn description(self, description: impl Into<String>) -> FeedBuilder<TitleDescription> {
            FeedBuilder::<TitleDescription> {
                category: self.category,
                url: self.url,
                title: self.title,
                description: description.into(),
                state: TitleDescription,
            }
        }
    }

    pub trait Description {
        fn category(self, category: impl Into<String>) -> Description;
        fn url(self, url: impl Into<String>) -> UrlDescription;
        fn title(self, title: impl Into<String>) -> TitleDescription;
    }

    impl DescriptionState for FeedBuilder<Description> {
        fn category(mut self, category: impl Into<String>) -> Self {
            self.category = Some(category.into());
            self
        }

        fn url(self, url: impl Into<String>) -> FeedBuilder<UrlDescription> {
            FeedBuilder::<UrlDescription> {
                category: self.category,
                url: url.into(),
                title: self.title,
                description: self.description,
                state: UrlDescription,
            }
        }

        fn title(self, title: impl Into<String>) -> FeedBuilder<TitleDescription> {
            FeedBuilder::<TitleDescription> {
                category: self.category,
                url: self.url,
                title: title.into(),
                description: self.description,
                state: TitleDescription,
            }
        }
    }

    pub trait UrlTitle {
        fn category(self, category: impl Into<String>) -> UrlTitle;
        fn description(self, description: impl Into<String>) -> End;
    }

    impl UrlTitleState for FeedBuilder<UrlTitle> {
        fn category(mut self, category: impl Into<String>) -> Self {
            self.category = Some(category.into());
            self
        }

        fn description(self, description: impl Into<String>) -> FeedBuilder<End> {
            FeedBuilder::<End> {
                category: self.category,
                url: self.url,
                title: self.title,
                description: description.into(),
                state: End,
            }
        }
    }

    pub trait UrlDescription {
        fn category(self, category: impl Into<String>) -> UrlDescription;
        fn title(self, title: impl Into<String>) -> End;
    }

    impl UrlDescriptionState for FeedBuilder<UrlDescription> {
        fn category(mut self, category: impl Into<String>) -> Self {
            self.category = Some(category.into());
            self
        }

        fn title(self, title: impl Into<String>) -> FeedBuilder<End> {
            FeedBuilder::<End> {
                category: self.category,
                url: self.url,
                title: title.into(),
                description: self.description,
                state: End,
            }
        }
    }

    pub trait TitleDescription {
        fn category(self, category: impl Into<String>) -> TitleDescription;
        fn url(self, url: impl Into<String>) -> End;
    }

    impl TitleDescriptionState for FeedBuilder<TitleDescription> {
        fn category(mut self, category: impl Into<String>) -> Self {
            self.category = Some(category.into());
            self
        }

        fn url(self, url: impl Into<String>) -> FeedBuilder<End> {
            FeedBuilder::<End> {
                category: self.category,
                url: url.into(),
                title: self.title,
                description: self.description,
                state: End,
            }
        }
    }
}

use feed_builder::*;

impl FeedItem {
    pub fn builder() -> FeedBuilder<feed_builder::Start> {
        FeedBuilder::<feed_builder::Start>::build()
    }

    pub fn from_url(
        url: impl AsRef<str>,
        title: Option<impl AsRef<str>>,
        category: Option<impl AsRef<str>>,
    ) -> anyhow::Result<Self> {
        let builder = Self::builder().url(url.as_ref());

        let feed = match AtomFeed::from_url(&url) {
            Ok(f) => Ok(Feed::ATOM(f)),
            _ => RssFeed::from_url(&url).map(Feed::RSS),
        }?;

        let builder = if let Some(t) = title {
            builder.title(t.as_ref())
        } else {
            builder.title(feed.title())
        };
        let builder = if let Some(c) = category {
            builder.category(c.as_ref())
        } else {
            builder
        };

        Ok(builder.description(feed.description()).create())
    }
}
