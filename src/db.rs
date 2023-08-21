use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use sqlx::{self, FromRow, SqlitePool};

use tokio::fs::{create_dir_all, File};

use chrono::NaiveDateTime;

// data types to use for the table
#[derive(Clone, Debug, Hash, PartialEq, Eq, Default, FromRow)]
pub struct RssFeedSQL {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub description: String,
    pub copyright: Option<String>,
    pub managing_editor: Option<String>,
    pub web_master: Option<String>,
    pub pub_date: Option<NaiveDateTime>,
    pub category: String,
    pub docs: Option<String>,
    pub ttl: Option<i64>,
    pub image: Option<String>,
    pub skip_hours: String,
    pub skip_days: String,
}

use crate::feed::rss::{RssChannel, RssFeed, RssItem};
impl TryFrom<RssFeedSQL> for RssFeed {
    type Error = anyhow::Error;

    fn try_from(sql: RssFeedSQL) -> Result<Self, Self::Error> {
        Ok(Self {
            channel: RssChannel {
                title: sql.title,
                description: sql.description,
                link: (),
                copyright: sql.copyright,
                managing_editor: sql.managing_editor,
                web_master: sql.web_master,
                pub_date: sql.pub_date.map(|naive| naive.and_utc().to_owned()),
                category: toml::from_str(&sql.category)?,
                docs: sql.docs,
                ttl: sql.ttl.map(|i| i as usize),
                image: sql.image,
                skip_hours: toml::from_str(&sql.skip_hours)?,
                skip_days: toml::from_str(&sql.skip_days)?,
                item: vec![],
            },
        })
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Default, FromRow)]
pub struct RssItemSQL {
    pub id: i64,
    pub feed_id: i64,
    pub title: Option<String>,
    pub link: String,
    pub description: String,
    pub date: Option<NaiveDateTime>,
    pub author: Option<String>,
    pub category: String,
    pub comments: Option<String>,
    pub enclosure: String,
    pub guid: Option<String>,
    pub source: String,
}

impl TryFrom<RssItemSQL> for RssItem {
    type Error = anyhow::Error;

    fn try_from(sql: RssItemSQL) -> Result<Self, Self::Error> {
        Ok(RssItem {
            title: sql.title,
            link: sql.link,
            description: sql.description,
            date: sql.date.map(|naive| naive.and_utc().to_owned()),
            author: sql.author,
            category: toml::from_str(&sql.category)?,
            comments: sql.comments,
            enclosure: toml::from_str(&sql.enclosure)?,
            guid: sql.guid,
            source: toml::from_str(&sql.source)?,
        })
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Default, FromRow)]
pub struct AtomFeedSQL {
    pub id: i64,
    pub atom_id: String,
    pub title: String,
    pub updated: NaiveDateTime,
    pub author: String,
    pub link: String,
    pub category: String,
    pub icon: Option<String>,
    pub logo: Option<String>,
    pub rights: Option<String>,
    pub subtitle: Option<String>,
    pub ttl: Option<i64>,
    pub skip_days: String,
    pub skip_hours: String,
}

use crate::feed::atom::{AtomFeed, Entry};
impl TryFrom<AtomFeedSQL> for AtomFeed {
    type Error = anyhow::Error;

    fn try_from(sql: AtomFeedSQL) -> Result<Self, Self::Error> {
        Ok(AtomFeed {
            id: sql.atom_id,
            title: sql.title,
            updated: sql.updated.and_utc().to_owned(),
            author: toml::from_str(&sql.author)?,
            link: toml::from_str(&sql.link)?,
            category: toml::from_str(&sql.category)?,
            icon: sql.icon,
            logo: sql.logo,
            rights: sql.rights,
            subtitle: sql.subtitle,
            entry: vec![],
            ttl: sql.ttl.map(|i| i as usize),
            skip_days: toml::from_str(&sql.skip_days)?,
            skip_hours: toml::from_str(&sql.skip_hours)?,
        })
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Default, FromRow)]
pub struct AtomEntrySQL {
    pub id: i64,
    pub feed_id: i64,
    pub entry_id: String,
    pub title: String,
    pub updated: NaiveDateTime,
    pub author: String,
    pub contributer: String,
    pub published: Option<NaiveDateTime>,
    pub rights: Option<String>,
    pub source: String,
    pub summary: Option<String>,
}

impl TryFrom<AtomEntrySQL> for Entry {
    type Error = anyhow::Error;

    fn try_from(sql: AtomEntrySQL) -> Result<Self, Self::Error> {
        Ok(Self {
            id: sql.entry_id,
            title: sql.title,
            updated: sql.updated.and_utc().to_owned(),
            author: toml::from_str(&sql.author)?,
            contributer: toml::from_str(&sql.contributer)?,
            published: sql.published.map(|naive| naive.and_utc().to_owned()),
            rights: sql.rights,
            source: toml::from_str(&sql.source)?,
            summary: sql.summary,
        })
    }
}

// Adds schema to path
pub async fn db_pool(path: impl AsRef<str>) -> anyhow::Result<SqlitePool> {
    let pathbuf = PathBuf::from_str(path.as_ref())?;
    create_dir_all(pathbuf.parent().unwrap()).await?;
    if File::open(&pathbuf).await.is_err() {
        File::create(&pathbuf).await?;
    }

    SqlitePool::connect(&format!("sqlite:{}", path.as_ref()))
        .await
        .map_err(|e| anyhow::anyhow!("sqlite connection: {}", e))
}

pub async fn setup_tables(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query_file!("migrations/20230821012453_setup.sql")
        .fetch_all(pool)
        .await
        .map_err(|e| anyhow::anyhow!("creating tables: {}", e))
        .map(|_| ())
}

pub async fn get_rss_feed(pool: &SqlitePool, url: impl AsRef<str>) -> anyhow::Result<RssFeed> {
    let url = String::from(url.as_ref());
    let feed: RssFeedSQL =
        sqlx::query_as!(RssFeedSQL, "SELECT * FROM rss_feeds WHERE url = $1", url)
            .fetch_one(pool)
            .await?;

    let feed_id = feed.id;
    let mut feed: RssFeed = feed.try_into()?;

    let items: Vec<RssItemSQL> = sqlx::query_as!(
        RssItemSQL,
        "SELECT * FROM rss_items WHERE feed_id = $1",
        feed_id
    )
    .fetch_all(pool)
    .await?;

    feed.channel.item = items
        .into_iter()
        .filter_map(|item| {
            if let Ok(item) = TryInto::<RssItem>::try_into(item) {
                Some(item)
            } else {
                None
            }
        })
        .collect();

    Ok(feed)
}

pub async fn get_rss_feeds(pool: &SqlitePool) -> anyhow::Result<Vec<RssFeed>> {
    let feeds: Vec<RssFeedSQL> = sqlx::query_as!(RssFeedSQL, "SELECT * FROM rss_feeds")
        .fetch_all(pool)
        .await
        .map_err(|e| anyhow::anyhow!("failed to query rss feeds: {}", e))?;
    let mut feeds = feeds
        .into_iter()
        .map(|sql| (sql.id, TryInto::<RssFeed>::try_into(sql)))
        .filter_map(|result| {
            if let (id, Ok(r)) = result {
                Some((id, r))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let mut items: Vec<RssItemSQL> = sqlx::query_as!(RssItemSQL, "SELECT * FROM rss_items")
        .fetch_all(pool)
        .await
        .map_err(|e| anyhow::anyhow!("failed to query rss items: {}", e))?;

    let mut found_indices: HashMap<i64, usize> = HashMap::new();

    loop {
        if items.is_empty() {
            break;
        }

        let item = items.remove(0);
        if let Some(&index) = found_indices.get(&item.feed_id) {
            feeds.get_mut(index).and_then(|feed| {
                if let Ok(item) = item.try_into() {
                    feed.1.channel.item.push(item);
                }
                Option::<()>::None
            });
        } else {
            let index = feeds.iter().enumerate().find_map(|(index, (id, _))| {
                if *id == item.feed_id {
                    Some(index)
                } else {
                    None
                }
            });
            if let Some(index) = index {
                found_indices
                    .insert(item.feed_id, index)
                    .ok_or_else(|| anyhow::anyhow!("error inserting into hashmap"))?;
                feeds.get_mut(index).and_then(|feed| {
                    if let Ok(item) = item.try_into() {
                        feed.1.channel.item.push(item);
                    }
                    Option::<()>::None
                });
            }
        }
    }
    Ok(feeds.into_iter().map(|(_, feed)| feed).collect())
}

pub async fn get_atom_feed(pool: &SqlitePool, url: impl AsRef<str>) -> anyhow::Result<AtomFeed> {
    let url = String::from(url.as_ref());
    let feed: AtomFeedSQL =
        sqlx::query_as!(AtomFeedSQL, "SELECT * FROM atom_feeds WHERE link = $1", url)
            .fetch_one(pool)
            .await?;

    let feed_id = feed.id;
    let mut feed: AtomFeed = feed.try_into()?;

    let items: Vec<AtomEntrySQL> = sqlx::query_as!(
        AtomEntrySQL,
        "SELECT * FROM atom_items WHERE feed_id = $1",
        feed_id
    )
    .fetch_all(pool)
    .await?;

    feed.entry = items
        .into_iter()
        .filter_map(|item| {
            if let Ok(item) = TryInto::<Entry>::try_into(item) {
                Some(item)
            } else {
                None
            }
        })
        .collect();

    Ok(feed)
}

pub async fn get_atom_feeds(pool: &SqlitePool) -> anyhow::Result<Vec<AtomFeed>> {
    let feeds: Vec<AtomFeedSQL> = sqlx::query_as!(AtomFeedSQL, "SELECT * FROM atom_feeds")
        .fetch_all(pool)
        .await?;
    let mut feeds = feeds
        .into_iter()
        .map(|sql| (sql.id, TryInto::<AtomFeed>::try_into(sql)))
        .filter_map(|result| {
            if let (id, Ok(r)) = result {
                Some((id, r))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let mut items: Vec<AtomEntrySQL> = sqlx::query_as!(AtomEntrySQL, "SELECT * FROM atom_items")
        .fetch_all(pool)
        .await?;

    let mut found_indices: HashMap<i64, usize> = HashMap::new();

    loop {
        if items.is_empty() {
            break;
        }

        let item = items.remove(0);
        if let Some(&index) = found_indices.get(&item.feed_id) {
            feeds.get_mut(index).and_then(|feed| {
                if let Ok(item) = item.try_into() {
                    feed.1.entry.push(item);
                }
                Option::<()>::None
            });
        } else {
            let index = feeds.iter().enumerate().find_map(|(index, (id, _))| {
                if *id == item.feed_id {
                    Some(index)
                } else {
                    None
                }
            });
            if let Some(index) = index {
                found_indices
                    .insert(item.feed_id, index)
                    .ok_or_else(|| anyhow::anyhow!("error inserting into hashmap"))?;
                feeds.get_mut(index).and_then(|feed| {
                    if let Ok(item) = item.try_into() {
                        feed.1.entry.push(item);
                    }
                    Option::<()>::None
                });
            }
        }
    }
    Ok(feeds.into_iter().map(|(_, feed)| feed).collect())
}

use crate::feed::Feed;

pub async fn get_feeds(pool: &SqlitePool) -> anyhow::Result<Vec<Feed>> {
    let rss_feeds = get_rss_feeds(pool).await?;
    let atom_feeds = get_atom_feeds(pool).await?;

    let rss_iter = rss_feeds.into_iter().map(Feed::RSS);
    let atom_iter = atom_feeds.into_iter().map(Feed::ATOM);

    Ok(rss_iter.chain(atom_iter).collect())
}
