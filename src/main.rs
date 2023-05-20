mod config;
mod handlers;

use futures_util::StreamExt;
use irc::{
    client::{data::config::Config, Client},
    proto::{Capability, Command, Message},
};
use tokio::signal::unix::{signal, SignalKind};

use crate::handlers::{
    handle_clear_chat, handle_clear_msg, handle_notice, handle_priv_msg, print_message,
};
use config::{load_config, FileHandleManager};

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
            irc_message = irc_stream.next() => {
                match irc_message {
                    Some(irc_event) => handle_irc_event(&mut file_handles, irc_event).await,
                    _ => break
                }
            }
            else => break
        }
    }
}

async fn handle_irc_event(
    file_handles: &mut FileHandleManager,
    irc_event: Result<Message, irc::error::Error>,
) {
    let message = irc_event.expect("get IRC message");
    match message.clone().command {
        Command::PRIVMSG(ref channel_name, ref msg) => {
            handle_priv_msg(
                file_handles,
                message.source_nickname().unwrap_or("???"),
                channel_name,
                msg,
                message.tags.clone(),
            )
            .await
        }

        Command::Raw(command, value) => match command.as_str() {
            "CLEARCHAT" => handle_clear_chat(file_handles, &value, message.tags).await,
            "CLEARMSG" => handle_clear_msg(file_handles, &value, message.tags).await,
            "ROOMSTATE" | "USERNOTICE" => {
                handle_notice(file_handles, &command, &value, message.tags).await
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
