use std::fs::File;
use std::io::{BufRead, BufReader};

use tokio::runtime::Handle;
use tokio::task::block_in_place;

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

use serde::{Deserialize, Serialize};

use quick_xml::de::from_reader;

use reqwest::{self, Url};

// Atom Feed file
#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct AtomFeed {
    pub id: String,
    pub title: String,
    pub updated: DateTime<Utc>,
    pub author: Option<Author>,
    pub link: Option<Link>,
    #[serde(default)]
    pub category: Vec<Category>,
    pub icon: Option<String>,
    pub logo: Option<String>,
    pub rights: Option<String>,
    pub subtitle: Option<String>,
    #[serde(default)]
    pub entry: Vec<Entry>,
    pub ttl: Option<usize>,
    #[serde(default)]
    pub skip_days: Vec<super::rss::Day>,
    #[serde(default)]
    pub skip_hours: Vec<super::rss::Hour>,
    #[serde(default)]
    pub last_updated: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Link {
    #[serde(rename = "@href")]
    href: String,
    #[serde(rename = "@rel", default)]
    rel: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Category {
    #[serde(rename = "@term")]
    term: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub updated: DateTime<Utc>,
    pub author: Option<Author>,
    pub contributer: Option<Contributer>,
    pub published: Option<DateTime<Utc>>,
    pub rights: Option<String>,
    pub source: Option<Source>,
    pub summary: Option<String>,
    pub read: Option<()>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Author {
    name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Contributer {
    #[serde(default)]
    contributers: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Source {
    id: String,
    title: String,
    updated: DateTime<Utc>,
}

pub fn xml_from_reader(url: impl Into<String>, read: impl BufRead) -> anyhow::Result<AtomFeed> {
    from_reader(read).map_err(|e| anyhow::anyhow!("{}: {}", url.into(), e))
}

impl AtomFeed {
    // Create a feed item from a URL to an RSS feed,
    // Filling the title and category fields if given
    // (title defaults to title from rss feed)
    pub fn from_url(url: impl AsRef<str>) -> anyhow::Result<Self> {
        let url: Url = Url::parse(url.as_ref())?;

        // Stream rss feed to the write end of the pipe in a new task
        let feed = if url.scheme() == "http" || url.scheme() == "https" {
            let bytes = {
                let url = url.clone();
                block_in_place(move || {
                    Handle::current()
                        .block_on(async move { reqwest::get(url.clone()).await?.bytes().await })
                })
            }?;
            xml_from_reader(url, BufReader::new(bytes.as_ref()))
        } else if url.scheme() == "file" {
            let path = url.path();
            let file = File::open(path)?;
            xml_from_reader(url, BufReader::new(file))
        } else {
            anyhow::bail!("{}: unsupported url schema", url)
        }?;

        Ok(feed)
    }

    // Use metadata on channel to figure out if it's time to update
    pub fn should_update(&self) -> bool {
        if let Some(last_update) = self.last_updated {
            let now = chrono::offset::Utc::now();
            if self.skip_days.contains(&now.date_naive().weekday().into()) {
                return false;
            }
            if self.skip_hours.contains(&now.time().hour().into()) {
                return false;
            }

            if let Some(ttl) = self.ttl {
                let duration_since = now.signed_duration_since(last_update);
                match Duration::from_std(std::time::Duration::from_secs(ttl as u64 * 60)) {
                    Ok(ttl) => duration_since >= ttl,
                    _ => true,
                }
            } else {
                true
            }
        } else {
            true
        }
    }
}

#[cfg(test)]
mod test {
    use chrono::{DateTime, FixedOffset};
    use std::path::PathBuf;

    use super::{AtomFeed, Author, Entry, Link};

    fn get_test_dir() -> PathBuf {
        std::env::current_dir()
            .expect("failed to get current directory")
            .join("test")
    }

    #[test]
    fn empty_file() {
        let url = get_test_dir().join("empty.xml");
        let feed = AtomFeed::from_url(format!("file://{}", url.to_string_lossy()));
        assert!(feed.is_err());
    }

    #[test]
    fn full_file() {
        let url = get_test_dir().join("atomfeed.xml");
        let feed = AtomFeed::from_url(format!("file://{}", url.to_string_lossy()));
        assert!(feed.is_ok());

        let feed = feed.unwrap();
        let expected_feed = AtomFeed {
            id: "urn:uuid:60a76c80-d399-11d9-b93C-0003939e0af6".to_owned(),
            title: "Example Feed".to_owned(),
            updated: DateTime::<FixedOffset>::parse_from_rfc3339("2003-12-13T18:30:02Z")
                .unwrap()
                .into(),
            author: Some(Author {
                name: "John Doe".to_owned(),
            }),
            link: Some(Link {
                href: "http://example.org/".to_owned(),
                rel: String::default(),
            }),
            entry: vec![Entry {
                id: "urn:uuid:1225c695-cfb8-4ebb-aaaa-80da344efa6a".to_owned(),
                title: "Atom-Powered Robots Run Amok".to_owned(),
                updated: DateTime::<FixedOffset>::parse_from_rfc3339("2003-12-13T18:30:02Z")
                    .unwrap()
                    .into(),
                summary: Some("Some text.".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(expected_feed, feed);
    }
}
