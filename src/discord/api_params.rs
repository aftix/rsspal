use serenity::json::{json, JsonMap};

pub fn modify_channel(
    name: Option<&str>,
    parent_id: Option<u64>,
    topic: Option<&str>,
    remove_parent: bool,
) -> JsonMap {
    let mut map = JsonMap::new();
    if let Some(name) = name {
        map.insert(String::from("name"), json!(name));
    }

    if let Some(parent_id) = parent_id {
        map.insert(String::from("parent_id"), json!(parent_id));
    } else if remove_parent {
        map.insert(String::from("parent_id"), json!(null));
    }

    if let Some(topic) = topic {
        map.insert(String::from("topic"), json!(topic));
    }

    map
}

pub fn create_channel(
    name: &str,
    category: bool,
    topic: Option<&str>,
    parent_id: Option<u64>,
) -> JsonMap {
    let mut map = JsonMap::new();

    map.insert(String::from("name"), json!(name));
    if category {
        map.insert(String::from("type"), json!(4));
    } else {
        map.insert(String::from("type"), json!(0));
    }

    if let Some(topic) = topic {
        map.insert(String::from("topic"), json!(topic));
    }

    if let Some(parent_id) = parent_id {
        map.insert(String::from("parent_id"), json!(parent_id));
    }

    map
}

pub fn create_thread(name: &str) -> JsonMap {
    let name = super::truncate(name, 95);
    let mut map = JsonMap::new();
    map.insert(String::from("name"), json!(name));
    map.insert(String::from("type"), json!(12));
    map
}
