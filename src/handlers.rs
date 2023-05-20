use std::collections::BTreeMap;

use irc::proto::{message::Tag, Message};
use serde::Serialize;

use crate::config::FileHandleManager;

#[derive(Debug, Serialize)]
struct PrivMsg<'a> {
    sender: &'a str,
    message: &'a String,
    tags: BTreeMap<String, String>,
}

pub(crate) async fn handle_priv_msg(
    file_handles: &mut FileHandleManager,
    sender: &str,
    channel_name: &String,
    message: &String,
    tags: Option<Vec<Tag>>,
) {
    let priv_msg = PrivMsg {
        sender,
        message,
        tags: tags_to_map(tags),
    };

    file_handles
        .write_to_log(
            channel_name,
            format!("PRIVMSG{}", ron::to_string(&priv_msg).unwrap()),
        )
        .await;
}

pub(crate) async fn handle_clear_chat(
    file_handles: &mut FileHandleManager,
    value: &[String],
    tags: Option<Vec<Tag>>,
) {
    match value.len() {
        1 => {
            file_handles
                .write_to_log(
                    value.get(0).unwrap(),
                    format!(
                        "CLEARCHAT(tags:{})",
                        ron::to_string(&tags_to_map(tags)).unwrap()
                    ),
                )
                .await;
        }
        2 => {
            file_handles
                .write_to_log(
                    value.get(0).unwrap(),
                    format!(
                        "CLEARCHAT(user:\"{}\",tags:{})",
                        value.get(1).unwrap(),
                        ron::to_string(&tags_to_map(tags)).unwrap()
                    ),
                )
                .await;
        }
        _ => {
            panic!(
                "unexpected number of params for CLEARCHAT: {}",
                value.join(" ")
            )
        }
    }
}

pub(crate) async fn handle_clear_msg(
    file_handles: &mut FileHandleManager,
    value: &[String],
    tags: Option<Vec<Tag>>,
) {
    file_handles
        .write_to_log(
            value.get(0).unwrap(),
            format!(
                "CLEARMSG(message:\"{}\",tags:{})",
                value.get(1).unwrap(),
                ron::to_string(&tags_to_map(tags)).unwrap()
            ),
        )
        .await;
}

pub(crate) async fn handle_notice(
    file_handles: &mut FileHandleManager,
    command: &String,
    value: &[String],
    tags: Option<Vec<Tag>>,
) {
    file_handles
        .write_to_log(
            value.get(0).unwrap(),
            format!(
                "{}(tags:{})",
                command,
                ron::to_string(&tags_to_map(tags)).unwrap()
            ),
        )
        .await;
}

pub(crate) fn print_message(message: &Message) {
    println!(
        "{:?} {}{:?}",
        message.command,
        message
            .prefix
            .clone()
            .map(|p| format!("from {:?} ", p))
            .unwrap_or("".to_owned()),
        tags_to_map(message.clone().tags),
    )
}

fn tags_to_map(tags: Option<Vec<Tag>>) -> BTreeMap<String, String> {
    tags.unwrap_or_default()
        .into_iter()
        .map(|t| (t.0, t.1.unwrap_or_default()))
        .collect()
}
