use std::collections::HashMap;

use futures_util::future::join_all;
use serde::Deserialize;
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncWriteExt,
};

#[derive(Debug, Deserialize)]
pub(crate) struct GhostConfig {
    pub(crate) channels: Vec<String>,
    pub(crate) log_path: String,
}
pub(crate) struct FileHandleManager(pub(crate) HashMap<String, File>);

impl FileHandleManager {
    pub async fn write_to_log(&mut self, channel_name: &String, line: String) {
        match self.0.get_mut(channel_name) {
            Some(file) => file
                .write_all(
                    format!("{} // {}\n", line, chrono::Local::now().to_rfc3339()).as_bytes(),
                )
                .await
                .expect("append to file"),
            None => eprintln!(
                "No file opened for {}, would have logged:\n{:?}",
                channel_name, line
            ),
        }
    }
}

pub(crate) async fn load_config() -> (Vec<String>, FileHandleManager) {
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
