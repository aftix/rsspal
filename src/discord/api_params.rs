use serenity::json::JsonMap;

pub(super) fn modify_channel(
    name: Option<&str>,
    parent_id: Option<u64>,
    topic: Option<&str>,
    remove_parent: bool,
) -> JsonMap {
    let mut map = String::from("{");
    if let Some(name) = name {
        map.push_str(&format!(r#""name": "{}","#, name));
    }

    if let Some(parent_id) = parent_id {
        map.push_str(&format!(r#""parent_id": "{}","#, parent_id));
    } else if remove_parent {
        map.push_str(r#""parent_id": null,"#);
    }

    if let Some(topic) = topic {
        map.push_str(&format!(r#""topic": "{}""#, topic))
    }

    map = map.trim_end_matches(",").to_string();
    map.push('}');

    serde_json::from_str(&map).expect("failed to make JsonMap")
}

pub(super) fn create_channel(
    name: &str,
    category: bool,
    topic: Option<&str>,
    parent_id: Option<u64>,
) -> JsonMap {
    let mut map = format!(r#"{{"name": "{}","#, name);
    if category {
        map.push_str(r#""type": 4,"#);
    } else {
        map.push_str(r#""type": 0,"#);
    }

    if let Some(topic) = topic {
        map.push_str(&format!(r#""topic": "{}""#, topic))
    }

    if let Some(parent_id) = parent_id {
        map.push_str(&format!(r#""parent_id": "{}""#, parent_id))
    }

    map = map.trim_end_matches(",").to_string();
    map.push('}');

    serde_json::from_str(&map).expect("failed to make JsonMap")
}

pub(super) fn create_thread(name: &str) -> JsonMap {
    serde_json::from_str(&format!(
        r#"{{"name": "read-{}", "type": 11}}"#,
        &name[..95]
    ))
    .expect("failed to make JsonMap")
}
