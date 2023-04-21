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
    signal::unix::{signal, SignalKind},
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
                .write_all(format!("{} // {}\n", line, chrono::Local::now().to_rfc3339()).as_bytes())
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
    let (mut irc_channels, mut file_handles) = load_config().await;

    let mut sighup_stream = signal(SignalKind::hangup()).expect("create sighup stream");

    let irc_config = Config {
        nickname: Some("justinfan12345".to_owned()),
        server: Some("irc.chat.twitch.tv".to_owned()),
        use_tls: Some(true),
        channels: irc_channels.clone(),
        ..Default::default()
    };

    let mut irc_client = Client::from_config(irc_config)
        .await
        .expect("valid IRC client config");

    irc_client
        .send_cap_req(&[
            Capability::Custom("twitch.tv/tags"),
            Capability::Custom("twitch.tv/commands"),
        ])
        .expect("send capability request");
    irc_client.identify().expect("IRC identify");

    let mut irc_stream = irc_client.stream().expect("IRC stream");

    loop {
        tokio::select! {
            _ = sighup_stream.recv() => {
                let (new_irc_channels, new_file_handles) = load_config().await;
                let removed_irc_channels: Vec<_> = irc_channels.clone().into_iter().filter(|c| !new_irc_channels.contains(c)).collect();
                let added_irc_channels: Vec<_> = new_irc_channels.clone().into_iter().filter(|c| !irc_channels.contains(c)).collect();
                if !removed_irc_channels.is_empty() {
                    irc_client.send_part(removed_irc_channels.join(",")).expect("leave removed chanels");
                }
                file_handles = new_file_handles;
                if !added_irc_channels.is_empty() {
                    irc_client.send_join(added_irc_channels.join(",")).expect("join added channels");
                }
                irc_channels = new_irc_channels;
                println!("Reloaded config.");
            }
            Some(irc_event) = irc_stream.next() => {
                let message = irc_event.expect("get IRC message");
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
                                format!("PRIVMSG{}", ron::to_string(&priv_msg).unwrap()),
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
                                            "{}(tags:{})",
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
                                            "{}(user:\"{}\",tags:{})",
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
                                        "{}(message:\"{}\",tags:{})",
                                        command,
                                        value.get(1).unwrap(),
                                        ron::to_string(&tags_to_map(message.tags)).unwrap()
                                    ),
                                )
                                .await;
                        }
                        "ROOMSTATE" | "USERNOTICE" => {
                            file_handles
                                .write_to_log(
                                    value.get(0).unwrap(),
                                    format!(
                                        "{}(tags:{})",
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
            else => break
        }
    }
}

async fn load_config() -> (Vec<String>, FileHandleManager) {
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
            if !c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                panic!("Invalid channel name: {}", c);
            }
            format!("#{}", c.to_ascii_lowercase())
        })
        .collect();

    tokio::fs::create_dir_all(ghost_config.log_path)
        .await
        .expect("create log directory");

    let file_handles = open_log_files(&irc_channels.clone()).await;
    (irc_channels, file_handles)
}

async fn open_log_files(irc_channels: &[String]) -> FileHandleManager {
    let startup_time = chrono::Local::now().to_rfc3339();

    FileHandleManager(
        join_all(irc_channels.iter().map(|c| async {
            let c = c.to_owned();
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(format!("logs/{}.txt", c))
                .await
                .expect("open/create log file");
            file.write_all(format!("// File opened at {}\n", startup_time).as_bytes())
                .await
                .expect("write initial line");
            (c, file)
        }))
        .await
        .into_iter()
        .collect(),
    )
}

fn print_message(message: &Message) {
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
