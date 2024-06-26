use chrono::{DateTime, Datelike, Duration, Timelike, Utc, Weekday};
use quick_xml::de::from_reader;
use reqwest::{self, Url};
use serde::{Deserialize, Serialize};
use serenity::builder::CreateEmbed;
use std::{
    fs::File,
    io::{BufRead, BufReader},
};
use tracing::{debug, info_span, instrument, Instrument};

// RSS Feed file
#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct RssFeed {
    pub channel: RssChannel,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct RssChannel {
    pub title: String,
    pub description: String,
    #[serde(skip, default)]
    pub link: String,
    #[serde(default)]
    pub url: String,
    pub copyright: Option<String>,
    #[serde(rename = "managingEditor")]
    pub managing_editor: Option<String>,
    #[serde(rename = "webMaster")]
    pub web_master: Option<String>,
    #[serde(rename = "pubDate", with = "rfc822", default)]
    pub pub_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub category: Vec<Category>,
    pub docs: Option<String>,
    pub ttl: Option<usize>,
    pub image: Option<String>,
    #[serde(rename = "skipHours", default)]
    pub skip_hours: Vec<Hour>,
    #[serde(rename = "skipDays", default)]
    pub skip_days: Vec<Day>,
    #[serde(default)]
    pub item: Vec<RssItem>,
    pub last_updated: Option<DateTime<Utc>>,
    pub discord_category: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct RssItem {
    pub title: Option<String>,
    pub link: String,
    pub description: String,
    #[serde(
        rename = "pubDate",
        with = "rfc822",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub date: Option<DateTime<Utc>>,
    pub author: Option<String>,
    #[serde(default)]
    pub category: Vec<Category>,
    pub comments: Option<String>,
    pub enclosure: Option<Enclosure>,
    pub guid: Option<String>,
    pub source: Option<Source>,
    pub read: Option<()>,
}

impl RssItem {
    #[instrument(level = "debug")]
    pub fn to_embed(&self) -> impl Fn(&mut CreateEmbed) -> &mut CreateEmbed {
        let author = self.author.clone();
        let description = self.description.clone();
        let title = self.title.clone();
        let enclosure = self.enclosure.clone();
        let date = self.date;
        let link = self.link.clone();
        let comments = self.comments.clone();
        let source = self.source.clone();

        move |embed: &mut CreateEmbed| {
            if let Some(ref a) = author {
                embed.author(|author| author.name(a));
            }

            if let Some(ref enc) = enclosure {
                if super::is_image_mime_type(&enc.content_type) {
                    embed.image(&enc.url);
                }
            }

            if let Some(ref t) = title {
                embed.title(t);
            } else {
                embed.title("(Untitled)");
            }

            if let Some(d) = date {
                embed.timestamp(d);
            }

            embed.description(&description);
            embed.field("link", &link, false);

            if let Some(ref c) = comments {
                embed.field("comments", c, true);
            }

            if let Some(ref s) = source {
                embed.field("source", &s.source, true);
                embed.field("source url", &s.url, true);
            }

            embed
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Category {
    #[serde(rename = "@domain")]
    domain: Option<String>,
    #[serde(rename = "$text")]
    value: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Hour {
    #[serde(rename = "$text")]
    hour: u8,
}

impl From<u32> for Hour {
    fn from(u: u32) -> Self {
        Self {
            hour: (u % 24) as u8,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Day {
    #[serde(rename = "$text")]
    day: DaysOfWeek,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub enum DaysOfWeek {
    #[default]
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl From<Weekday> for Day {
    fn from(day: Weekday) -> Self {
        Self { day: day.into() }
    }
}

impl From<Weekday> for DaysOfWeek {
    fn from(day: Weekday) -> Self {
        match day {
            Weekday::Mon => DaysOfWeek::Monday,
            Weekday::Tue => DaysOfWeek::Tuesday,
            Weekday::Wed => DaysOfWeek::Wednesday,
            Weekday::Thu => DaysOfWeek::Thursday,
            Weekday::Fri => DaysOfWeek::Friday,
            Weekday::Sat => DaysOfWeek::Saturday,
            Weekday::Sun => DaysOfWeek::Sunday,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Enclosure {
    #[serde(rename = "@url")]
    pub url: String,
    #[serde(rename = "@length")]
    pub length: u64,
    #[serde(rename = "@type")]
    pub content_type: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Source {
    #[serde(rename = "@url")]
    url: String,
    #[serde(rename = "$text")]
    source: String,
}

pub fn xml_from_reader(url: impl Into<String>, read: impl BufRead) -> anyhow::Result<RssFeed> {
    from_reader(read).map_err(|e| anyhow::anyhow!("{}: {}", url.into(), e))
}

pub mod rfc822 {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    const FORMAT: &str = "%a, %d %b %Y %H:%M:%S %z";
    const FORMAT_SHORT: &str = "%a, %d %b %Y %H:%M %z";

    pub fn into_datetime(str: impl AsRef<str>) -> Result<DateTime<Utc>, chrono::ParseError> {
        let parsed_long = DateTime::parse_from_str(str.as_ref(), FORMAT);
        if parsed_long.is_ok() {
            parsed_long.map(Into::into)
        } else {
            let next = DateTime::parse_from_str(str.as_ref(), FORMAT_SHORT).map(Into::into);
            next
        }
    }

    pub fn serialize<S>(date: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match date {
            Some(dt) => {
                let s = format!("{}", dt.format(FORMAT));
                serializer.serialize_some(&s)
            }
            _ => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Option::<String>::deserialize(deserializer)?;
        if let Some(s) = s {
            into_datetime(s).map(Some).map_err(serde::de::Error::custom)
        } else {
            Ok(None)
        }
    }
}

impl RssFeed {
    // Create a feed item from a URL to an RSS feed,
    // Filling the title and category fields if given
    // (title defaults to title from rss feed)
    #[instrument(level = "debug", skip(url, user_agent))]
    pub async fn from_url(
        url: impl AsRef<str>,
        user_agent: Option<impl AsRef<str>>,
    ) -> anyhow::Result<Self> {
        let url: Url = Url::parse(url.as_ref())?;

        // Stream rss feed to the write end of the pipe in a new task
        let mut feed = if url.scheme() == "http" || url.scheme() == "https" {
            let bytes = (async {
                let client = if let Some(user) = user_agent {
                    reqwest::ClientBuilder::new()
                        .user_agent(user.as_ref())
                        .build()?
                } else {
                    reqwest::ClientBuilder::new().build()?
                };
                let req = client.get(url.clone()).build()?;
                client.execute(req).await?.bytes().await
            })
            .instrument(info_span!("RssFeed::reqwest"))
            .await?;

            xml_from_reader(url.as_str(), BufReader::new(bytes.as_ref()))
        } else if url.scheme() == "file" {
            let _ = info_span!("RssFeed::File");
            let path = url.path();
            let file = File::open(path)?;
            xml_from_reader(url.as_str(), BufReader::new(file))
        } else {
            anyhow::bail!("{}: unsupported url schema", url)
        }?;

        feed.channel.url = url.to_string();
        Ok(feed)
    }

    // Use metadata on channel to figure out if it's time to update
    #[instrument(level = "trace")]
    pub fn should_update(&self) -> bool {
        if let Some(last_update) = self.channel.last_updated {
            let now = chrono::offset::Utc::now();
            if self
                .channel
                .skip_days
                .contains(&now.date_naive().weekday().into())
            {
                debug!("Feed {} should be skipped today.", self.channel.title);
                return false;
            }
            if self.channel.skip_hours.contains(&now.time().hour().into()) {
                debug!("Feed {} should be skipped this hour.", self.channel.title);
                return false;
            }

            if let Some(ttl) = self.channel.ttl {
                debug!(
                    "Checking ttl to see if {} should be updated now.",
                    self.channel.title
                );
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
    use std::path::PathBuf;

    use tokio::runtime;

    use super::rfc822::into_datetime;

    #[allow(unused_imports)]
    use super::*;

    fn get_test_dir() -> PathBuf {
        std::env::current_dir()
            .expect("failed to get current directory")
            .join("test")
    }

    #[test]
    fn empty_file() {
        runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let url = get_test_dir().join("empty.xml");
                let feed = RssFeed::from_url(
                    format!("file://{}", url.to_string_lossy()),
                    Option::<String>::None,
                )
                .await;
                assert!(feed.is_err());
            });
    }

    #[test]
    fn full_file() {
        runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let url = get_test_dir().join("rssboard.xml");
                let feed = RssFeed::from_url(
                    format!("file://{}", url.to_string_lossy()),
                    Option::<String>::None,
                ).await;
                assert!(feed.is_ok());
                let feed = feed.unwrap();

                let expected_feed = RssFeed {
                    channel: RssChannel {
                        title: "NASA Space Station News".to_string(),
                        link: String::default(),
                        url: format!("file://{}", url.to_string_lossy()),
                        description: "A RSS news feed containing the latest NASA press releases on the International Space Station.".to_string(),
                        pub_date: Some(into_datetime("Tue, 10 Jun 2003 04:00:00 +0000").unwrap()),
                        docs: Some("https://www.rssboard.org/rss-specification".to_owned()),
                        managing_editor: Some("neil.armstrong@example.com (Neil Armstrong)".to_owned()),
                        web_master: Some("sally.ride@example.com (Sally Ride)".to_owned()),
                        item: vec![
                            RssItem {
                                title: Some("Louisiana Students to Hear from NASA Astronauts Aboard Space Station".to_owned()),
                                link: "http://www.nasa.gov/press-release/louisiana-students-to-hear-from-nasa-astronauts-aboard-space-station".to_owned(),
                                description: "As part of the state's first Earth-to-space call, students from Louisiana will have an opportunity soon to hear from NASA astronauts aboard the International Space Station.".to_owned(),
                                date: Some(into_datetime("Fri, 21 Jul 2023 09:04 -0400").unwrap()),
                                guid: Some("http://www.nasa.gov/press-release/louisiana-students-to-hear-from-nasa-astronauts-aboard-space-station".to_owned()),
                                ..Default::default()
                            },
                            RssItem {
                                title: None,
                                link: "http://www.nasa.gov/press-release/nasa-awards-integrated-mission-operations-contract-iii".to_owned(),
                                description: "NASA has selected KBR Wyle Services, LLC, of Fulton, Maryland, to provide mission and flight crew operations support for the International Space Station and future human space exploration.".to_owned(),
                                date: Some(into_datetime("Thu, 20 Jul 2023 15:05 -0400").unwrap()),
                                guid: Some("http://www.nasa.gov/press-release/nasa-awards-integrated-mission-operations-contract-iii".to_owned()),
                                ..Default::default()
                            },
                            RssItem {
                                title: Some("NASA Expands Options for Spacewalking, Moonwalking Suits".to_owned()),
                                link: "http://www.nasa.gov/press-release/nasa-expands-options-for-spacewalking-moonwalking-suits-services".to_owned(),
                                description: "NASA has awarded Axiom Space and Collins Aerospace task orders under existing contracts to advance spacewalking capabilities in low Earth orbit, as well as moonwalking services for Artemis missions.".to_owned(),
                                date: Some(into_datetime("Mon, 10 Jul 2023 14:14 -0400").unwrap()),
                                guid: Some("http://www.nasa.gov/press-release/nasa-expands-options-for-spacewalking-moonwalking-suits-services".to_owned()),
                                enclosure: Some(Enclosure {
                                    url: "http://www.nasa.gov/sites/default/files/styles/1x1_cardfeed/public/thumbnails/image/iss068e027836orig.jpg?itok=ucNUaaGx".to_owned(),
                                    length: 1032272,
                                    content_type: "image/jpeg".to_owned(),
                                }),
                                ..Default::default()
                            },
                            RssItem {
                                title: Some("NASA to Provide Coverage as Dragon Departs Station".to_owned()),
                                link: "http://www.nasa.gov/press-release/nasa-to-provide-coverage-as-dragon-departs-station-with-science".to_owned(),
                                description: "NASA is set to receive scientific research samples and hardware as a SpaceX Dragon cargo resupply spacecraft departs the International Space Station on Thursday, June 29.".to_owned(),
                                date: Some(into_datetime("Tue, 20 May 2003 08:56:02 +0000").unwrap()),
                                guid: Some("http://www.nasa.gov/press-release/nasa-to-provide-coverage-as-dragon-departs-station-with-science".to_owned()),
                                ..Default::default()
                            },
                            RssItem {
                                title: Some("NASA Plans Coverage of Roscosmos Spacewalk Outside Space Station".to_owned()),
                                link: "http://liftoff.msfc.nasa.gov/news/2003/news-laundry.asp".to_owned(),
                                description: "Compared to earlier spacecraft, the International Space Station has many luxuries, but laundry facilities are not one of them.  Instead, astronauts have other options.".to_owned(),
                                date: Some(into_datetime("Mon, 26 Jun 2023 12:45 -0400").unwrap()),
                                guid: Some("http://liftoff.msfc.nasa.gov/2003/05/20.html#item570".to_owned()),
                                enclosure: Some(Enclosure {
                                    url: "http://www.nasa.gov/sites/default/files/styles/1x1_cardfeed/public/thumbnails/image/spacex_dragon_june_29.jpg?itok=nIYlBLme".to_owned(),
                                    length: 269866,
                                    content_type: "image/jpeg".to_owned(),
                                }),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
                };
                assert_eq!(expected_feed, feed);
            });
    }
}
