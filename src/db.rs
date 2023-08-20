use std::path::PathBuf;
use std::str::FromStr;

use sqlx::{self, FromRow, SqlitePool};

use tokio::fs::{create_dir_all, File};

use chrono::{DateTime, Utc};

// data types to use for the table
#[derive(Clone, Debug, Hash, PartialEq, Eq, Default, FromRow)]
pub struct RssFeedSQL {
    pub title: String,
    pub description: String,
    pub copyright: Option<String>,
    pub managing_editor: Option<String>,
    pub web_master: Option<String>,
    pub pub_date: Option<DateTime<Utc>>,
    pub category: String,
    pub docs: Option<String>,
    pub ttl: Option<usize>,
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
                pub_date: sql.pub_date,
                category: toml::from_str(&sql.category)?,
                docs: sql.docs,
                ttl: sql.ttl,
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
    pub id: u32,
    pub feed_id: u32,
    pub title: Option<String>,
    pub link: String,
    pub description: String,
    pub date: Option<DateTime<Utc>>,
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
            date: sql.date,
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
    pub id: u32,
    pub atom_id: String,
    pub title: String,
    pub updated: DateTime<Utc>,
    pub author: String,
    pub link: String,
    pub category: String,
    pub icon: Option<String>,
    pub logo: Option<String>,
    pub rights: Option<String>,
    pub subtitle: Option<String>,
    pub ttl: Option<usize>,
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
            updated: sql.updated,
            author: toml::from_str(&sql.author)?,
            link: toml::from_str(&sql.link)?,
            category: toml::from_str(&sql.category)?,
            icon: sql.icon,
            logo: sql.logo,
            rights: sql.rights,
            subtitle: sql.subtitle,
            entry: vec![],
            ttl: sql.ttl,
            skip_days: toml::from_str(&sql.skip_days)?,
            skip_hours: toml::from_str(&sql.skip_hours)?,
        })
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Default, FromRow)]
pub struct AtomEntrySQL {
    pub id: u32,
    pub feed_id: u32,
    pub entry_id: String,
    pub title: String,
    pub updated: DateTime<Utc>,
    pub author: String,
    pub contributer: String,
    pub published: Option<DateTime<Utc>>,
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
            updated: sql.updated,
            author: toml::from_str(&sql.author)?,
            contributer: toml::from_str(&sql.contributer)?,
            published: sql.published,
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
    sqlx::query!(
        r#"
    PRAGMA foreign_keys;
    CREATE TABLE IF NOT EXISTS rss_feeds (
        id INTEGER NOT NULL PRIMARY KEY,
        title VARCHAR(512) NOT NULL,
        description TEXT NOT NULL,
        copyright VARCHAR(128),
        managing_editor VARCHAR(64),
        web_master VARCHAR(64), 
        pub_date REAL,
        category TEXT NOT NULL,
        docs VARCHAR(256),
        ttl INTEGER,
        image VARCHAR(256),
        skip_hours TEXT NOT NULL,
        skip_days TEXT NOT NULL
    );
    
    CREATE TABLE IF NOT EXISTS rss_items (
        id INTEGER NOT NULL PRIMARY KEY,
        feed_id INTEGER NOT NULL,
        title VARCHAR(512) NOT NULL,
        link VARCHAR(256) NOT NULL,
        description TEXT NOT NULL,
        date REAL NOT NULL,
        author VARCHAR(64),
        category TEXT NOT NULL,
        comments TEXT,
        enclosure VARCHAR(512) NOT NULL,
        guid TEXT,
        source TEXT NOT NULL,
        FOREIGN KEY (feed_id) REFERENCES rss_feeds (id)
    );
    
    CREATE TABLE IF NOT EXISTS atom_feeds (
        id INTEGER NOT NULL PRIMARY KEY,
        atom_id VARCHAR(128) NOT NULL,
        title VARCHAR(512) NOT NULL,
        updated REAL NOT NULL,
        author VARCHAR(128) NOT NULL,
        link VARCHAR(256) NOT NULL,
        category TEXT NOT NULL,
        icon VARCHAR(256),
        logo VARCHAR(256),
        rights VARCHAR(128),
        subtitle TEXT,
        ttl INTEGER,
        skip_days VARCHAR(64) NOT NULL,
        skip_hours VARCHAR(64) NOT NULL
    );
    
    CREATE TABLE IF NOT EXISTS atom_items (
        id INTEGER NOT NULL PRIMARY KEY,
        feed_id INTEGER NOT NULL,
        entry_id VARCHAR(128) NOT NULL,
        title VARCHAR(512) NOT NULL,
        updated REAL NOT NULL,
        author VARCHAR(64) NOT NULL,
        contributer VARCHAR(64) NOT NULL,
        published REAL,
        rights VARCHAR(128),
        source TEXT NOT NULL,
        summary TEXT,
        FOREIGN KEY (feed_id) REFERENCES atom_feeds (id)
    );"#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| anyhow::anyhow!("creating tables: {}", e))
    .map(|_| ())
}
