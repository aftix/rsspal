use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};

use crate::feed::Feed;

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Opml {
    #[serde(rename = "@version")]
    pub version: String,
    pub head: Head,
    pub body: Body,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Head {
    pub title: String,
    #[serde(rename = "dateCreated", with = "crate::feed::rss::rfc822", default)]
    pub date_created: Option<DateTime<Utc>>,
    #[serde(rename = "dateModified", with = "crate::feed::rss::rfc822", default)]
    pub date_modified: Option<DateTime<Utc>>,
    #[serde(rename = "ownerName")]
    pub owner_name: Option<String>,
    #[serde(rename = "ownerEmail")]
    pub owner_email: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Body {
    #[serde(default)]
    pub outline: Vec<Outline>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct Outline {
    #[serde(rename = "@text")]
    pub text: String,
    #[serde(rename = "@type")]
    pub content_type: String,
    #[serde(rename = "@xmlUrl")]
    pub xml_url: String,
    #[serde(rename = "@title")]
    pub title: Option<String>,
    #[serde(rename = "@description")]
    pub description: Option<String>,
    #[serde(rename = "@htmlUrl")]
    pub html_url: Option<String>,
}

impl From<&Feed> for Outline {
    fn from(feed: &Feed) -> Self {
        match feed {
            Feed::RSS(rss) => Self {
                text: rss.channel.title.clone(),
                content_type: "rss".into(),
                title: Some(rss.channel.title.clone()),
                description: Some(rss.channel.description.clone()),
                xml_url: rss.channel.url.clone(),
                html_url: None,
            },
            Feed::ATOM(atom) => {
                let link = atom
                    .link
                    .iter()
                    .find(|&link| link.rel.is_none() || link.rel == Some("alternate".to_string()));
                Self {
                    text: atom.title.clone(),
                    content_type: "atom".into(),
                    title: Some(atom.title.clone()),
                    description: atom.subtitle.clone(),
                    xml_url: atom.url.clone(),
                    html_url: link.map(|link| link.href.clone()),
                }
            }
        }
    }
}

impl<U, V> From<(U, V)> for Opml
where
    U: AsRef<str>,
    V: AsRef<[Feed]>,
{
    fn from((title, vec): (U, V)) -> Self {
        let feeds: &[Feed] = vec.as_ref();

        let mut opml = Self {
            version: "2.0".to_string(),
            head: Head {
                title: title.as_ref().to_string(),
                date_created: Some(chrono::offset::Utc::now()),
                date_modified: Some(chrono::offset::Utc::now()),
                owner_name: None,
                owner_email: None,
            },
            body: Body {
                outline: Vec::with_capacity(feeds.len()),
            },
        };

        for feed in feeds {
            opml.body.outline.push(feed.into());
        }

        opml
    }
}

use crate::feed::atom::{AtomFeed, Link};
use crate::feed::rss::{RssChannel, RssFeed};

// Does not pull from URL
impl From<Outline> for Feed {
    fn from(outline: Outline) -> Self {
        match outline.content_type.as_str() {
            "atom" => {
                let link = if let Some(link) = outline.html_url {
                    vec![Link {
                        href: link,
                        ..Default::default()
                    }]
                } else {
                    vec![]
                };
                Self::ATOM(AtomFeed {
                    title: outline.title.clone().unwrap(),
                    subtitle: outline.description.clone(),
                    link,
                    url: outline.xml_url.clone(),
                    ..Default::default()
                })
            }
            _ => Self::RSS(RssFeed {
                channel: RssChannel {
                    title: outline.title.clone().unwrap_or_else(String::default),
                    description: outline.description.clone().unwrap_or_else(String::default),
                    url: outline.xml_url.clone(),
                    ..Default::default()
                },
            }),
        }
    }
}

impl From<Opml> for Vec<Feed> {
    fn from(opml: Opml) -> Self {
        opml.body
            .outline
            .into_iter()
            .map(Into::<Feed>::into)
            .collect()
    }
}
