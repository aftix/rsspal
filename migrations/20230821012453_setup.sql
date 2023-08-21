-- Add migration script here
PRAGMA foreign_keys;
CREATE TABLE IF NOT EXISTS rss_feeds (
    id INTEGER NOT NULL PRIMARY KEY,
    url VARCHAR(256) NOT NULL,
    title VARCHAR(512) NOT NULL,
    description TEXT NOT NULL,
    copyright VARCHAR(128),
    managing_editor VARCHAR(64),
    web_master VARCHAR(64), 
    pub_date DATETIME DEFAULT (datetime('now')),
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
    date DATETIME NOT NULL DEFAULT (datetime('now')),
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
    updated DATETIME NOT NULL DEFAULT(datetime('now')),
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
    updated DATETIME NOT NULL DEFAULT(datetime('now')),
    author VARCHAR(64) NOT NULL,
    contributer VARCHAR(64) NOT NULL,
    published DATETIME DEFAULT(datetime('now')),
    rights VARCHAR(128),
    source TEXT NOT NULL,
    summary TEXT,
    FOREIGN KEY (feed_id) REFERENCES atom_feeds (id)
);
