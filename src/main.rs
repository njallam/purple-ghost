use std::collections::{BTreeMap, HashMap};

use futures_util::{future::join_all, StreamExt};
use irc::{
    client::{data::config::Config, Client},
    proto::{message::Tag, Capability, Command, Message},
};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncWriteExt,
};

#[derive(Debug, Deserialize)]
struct GhostConfig {
    channels: Vec<String>,
    log_path: String,
}

struct FileHandleManager(HashMap<String, File>);

impl FileHandleManager {
    pub async fn write_to_log(&mut self, channel_name: &String, line: String) {
        match self.0.get_mut(channel_name) {
            Some(file) => file
                .write_all(line.as_bytes())
                .await
                .expect("append to file"),
            None => eprintln!(
                "No file opened for {}, would have logged:\n{:?}",
                channel_name, line
            ),
        }
    }
}

#[derive(Debug, Serialize)]
struct PrivMsg<'a> {
    sender: &'a str,
    message: &'a String,
    tags: BTreeMap<String, String>,
}

#[tokio::main]
async fn main() {
    let ghost_config = ron::from_str::<GhostConfig>(
        &tokio::fs::read_to_string("config.ron")
            .await
            .expect("read config.ron"),
    )
    .expect("parse config");

    let irc_channels: Vec<String> = ghost_config
        .channels
        .into_iter()
        .map(|c| {
            if !c.chars().all(|ch| ch.is_ascii_alphanumeric()) {
                panic!("Invalid channel name: {}", c);
            }
            format!("#{}", c.to_ascii_lowercase())
        })
        .collect();

    tokio::fs::create_dir_all(ghost_config.log_path)
        .await
        .expect("create log directory");

    let startup_time = chrono::Local::now().to_rfc3339();

    let mut file_handles = FileHandleManager(
        join_all(irc_channels.clone().into_iter().map(|c| async {
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(format!("logs/{}.txt", c))
                .await
                .expect("open/create log file");
            file.write_all(format!("// Started at {}\n", startup_time).as_bytes())
                .await
                .expect("write initial line");
            (c, file)
        }))
        .await
        .into_iter()
        .collect(),
    );

    let config = Config {
        nickname: Some("justinfan12345".to_owned()),
        server: Some("irc.chat.twitch.tv".to_owned()),
        use_tls: Some(true),
        channels: irc_channels,
        ..Default::default()
    };

    let mut client = Client::from_config(config)
        .await
        .expect("valid IRC client config");

    client
        .send_cap_req(&[
            Capability::Custom("twitch.tv/tags"),
            Capability::Custom("twitch.tv/commands"),
        ])
        .expect("send capability request");
    client.identify().expect("IRC identify");

    let mut stream = client.stream().expect("IRC stream");

    while let Some(message) = stream.next().await.transpose().expect("stream IRC stream") {
        match message.clone().command {
            Command::PRIVMSG(ref channel_name, ref msg) => {
                let priv_msg = PrivMsg {
                    sender: message.source_nickname().unwrap_or("???"),
                    message: msg,
                    tags: tags_to_map(message.clone().tags),
                };

                file_handles
                    .write_to_log(
                        channel_name,
                        format!("PRIVMSG{}\n", ron::to_string(&priv_msg).unwrap()),
                    )
                    .await;
            }

            Command::Raw(command, value) => match command.as_str() {
                "CLEARCHAT" => match value.len() {
                    1 => {
                        file_handles
                            .write_to_log(
                                value.get(0).unwrap(),
                                format!(
                                    "{}(tags:{})\n",
                                    command,
                                    ron::to_string(&tags_to_map(message.tags)).unwrap()
                                ),
                            )
                            .await;
                    }
                    2 => {
                        file_handles
                            .write_to_log(
                                value.get(0).unwrap(),
                                format!(
                                    "{}(user:\"{}\",tags:{})\n",
                                    command,
                                    value.get(1).unwrap(),
                                    ron::to_string(&tags_to_map(message.tags)).unwrap()
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
                },
                "CLEARMSG" => {
                    file_handles
                        .write_to_log(
                            value.get(0).unwrap(),
                            format!(
                                "{}(message:\"{}\",tags:{})\n",
                                command,
                                value.get(1).unwrap(),
                                ron::to_string(&tags_to_map(message.tags)).unwrap()
                            ),
                        )
                        .await;
                }
                "ROOMSTATE"|"USERNOTICE" => {
                    file_handles
                        .write_to_log(
                            value.get(0).unwrap(),
                            format!(
                                "{}(tags:{})\n",
                                command,
                                ron::to_string(&tags_to_map(message.tags)).unwrap()
                            ),
                        )
                        .await;
                }
                _ => {
                    print_message(&message);
                }
            },
            _ => {
                print_message(&message);
            }
        }
    }
}

fn print_message(message: &Message) {
    println!(
        "{:?} {}{:?}",
        tags_to_map(message.clone().tags),
        message.prefix.clone().map(|p| format!("{:?} | ", p)).unwrap_or("".to_owned()),
        message.command
    );
}

fn tags_to_map(tags: Option<Vec<Tag>>) -> BTreeMap<String, String> {
    tags.unwrap_or_default()
        .into_iter()
        .map(|t| (t.0, t.1.unwrap_or_default()))
        .collect()
}
