/*
1. Request a connection to an "application"
2. Request to open a stream to publish on
3. Request access to publish with a particular stream name
*/

use rml_rtmp::{
    sessions::{
        ClientSession, ClientSessionConfig, ClientSessionResult, ClientSessionEvent},
    messages::{
        MessagePayload, RtmpMessage, UserControlEventType},
    chunk_io::{ChunkSerializer, ChunkDeserializer, Packet},
    time::RtmpTimestamp,
};
use rml_amf0::Amf0Value;
use std::collections::HashMap;

use lazy_static::lazy_static;
use std::sync::Mutex;

#[macro_use] extern crate log;

enum SendDataType {
    Audio,
    Video,
}

#[derive(Clone)]
pub struct MyClientSessionConfig {
    pub config: ClientSessionConfig,
    stream_key: Option<String>,
}

static APP_NAME: &str = "test rtmp";

static YOUTUBE_CHUNK_SIZE: u32 = 128;
static YOUTUBE_URL: &str = "rtmp://x.rtmp.youtube.com/live2";  // "rtmp://a.rtmp.youtube.com/live2";  ?
static YOUTUBE_KEY: &str = "0kjx-g7uh-82dh-vbqc-ct1p";

impl MyClientSessionConfig {
    fn new() -> Self {
        Self {
            config: ClientSessionConfig::new(),
            stream_key: None,
        }
    }

    pub fn default() ->Self {
        let mut var = Self::custom_config(Some(YOUTUBE_CHUNK_SIZE), Some(YOUTUBE_URL));
        var.set_stream_key(Some(YOUTUBE_KEY));
        return var;
    }

    pub fn custom_config(chunk_size: Option<u32>, tc_url: Option<&str>) ->Self {
        let mut var = Self::new();

        if let Some(chunk_size) = chunk_size {
            var.set_chunk_size(chunk_size);
        }

        if let Some(tc_url) = tc_url {
            var.set_url(tc_url);
        }
        return var;
    }

    pub fn set_url(&mut self, tc_url: &str) {
        self.config.tc_url = Some(tc_url.to_string());
    }

    pub fn set_chunk_size(&mut self, chunk_size: u32) {
        self.config.chunk_size = chunk_size;
    }

    pub fn set_stream_key(&mut self, stream_key: Option<&str>) {
        if stream_key.is_some(){
            self.stream_key = Some(stream_key.unwrap().to_string());
        } else {
            self.stream_key = None;
        }
    }

    pub fn set_playback_buffer_len(&mut self, buffer_len: u32) {
        self.config.playback_buffer_length_ms = buffer_len;
    }

    pub fn set_window_ack_size(&mut self, window_ack_size: u32) {
        self.config.window_ack_size = window_ack_size;
    }

    pub fn set_flash_version(&mut self, flash_version: &str) {
        self.config.flash_version = flash_version.to_string();
    }
}

lazy_static! {
    static ref CLIENT_CONFIG: Mutex<MyClientSessionConfig> = Mutex::new(MyClientSessionConfig::default());
}

fn main() {
    println!("Hello, world!");

    new_session_and_successful_connect_creates_set_chunk_size_message();
}


fn new_session_and_successful_connect_creates_set_chunk_size_message() {
    let app_name = "test".to_string();
    let mut config = CLIENT_CONFIG.lock().unwrap();
    config.set_chunk_size(4096);
    config.set_flash_version(app_name.as_str());

    let mut deserializer = ChunkDeserializer::new();
    let mut serializer = ChunkSerializer::new();
    let results = ClientSession::new(config.config.clone());
    match results {
        Ok((mut session, initial_results)) => {
            consume_results(&mut deserializer, initial_results);
            perform_successful_connect(
                app_name.clone(),
                &mut session,
                &mut serializer,
                &mut deserializer,
            );
        },
        Err(err) => {
            debug!("ClientSessionError: {:?}", err);
        }
    }
}

fn consume_results(deserializer: &mut ChunkDeserializer, results: Vec<ClientSessionResult>) {
    // Needed to keep the deserializer up to date
    split_results(deserializer, results);
}


fn split_results(
    deserializer: &mut ChunkDeserializer,
    mut results: Vec<ClientSessionResult>,
) -> (Vec<(MessagePayload, RtmpMessage)>, Vec<ClientSessionEvent>) {
    let mut responses = Vec::new();
    let mut events = Vec::new();

    for result in results.drain(..) {
        match result {
            ClientSessionResult::OutboundResponse(packet) => {
                let payload = deserializer
                    .get_next_message(&packet.bytes[..])
                    .unwrap()
                    .unwrap();
                let message = payload.to_rtmp_message().unwrap();
                match message {
                    RtmpMessage::SetChunkSize { size } => {
                        deserializer.set_max_chunk_size(size as usize).unwrap()
                    }
                    _ => (),
                }

                println!("response received: {:?}", message);
                responses.push((payload, message));
            }

            ClientSessionResult::RaisedEvent(event) => {
                println!("event received: {:?}", event);
                events.push(event);
            }

            ClientSessionResult::UnhandleableMessageReceived(payload) => {
                println!("unhandleable message: {:?}", payload);
            }
        }
    }

    (responses, events)
}

fn perform_successful_connect(
    app_name: String,
    session: &mut ClientSession,
    serializer: &mut ChunkSerializer,
    deserializer: &mut ChunkDeserializer,
) {
    let results = session.request_connection(app_name);
    match results {
        Ok(results) => {
            consume_results(deserializer, vec![results]);
            let response = get_connect_success_response(serializer);
            let results = session.handle_input(&response.bytes[..]);
            match results {
                Ok(results) => {
                    let (_, mut events) = split_results(deserializer, results);
                    assert_eq!(events.len(), 1, "Expected one event returned");
                    match events.remove(0) {
                        ClientSessionEvent::ConnectionRequestAccepted => (),
                        x => panic!(
                            "Expected connection accepted event, instead received: {:?}",
                            x
                        ),
                    }
                }
                Err(err) => {
                    debug!("session.handle_input error: {:?}", err);
                }
            }
        },
        Err(err) => {
            debug!("session.request_connection error: {:?}", err);
        }
    }
}

fn get_connect_success_response(serializer: &mut ChunkSerializer) -> Packet {
    let mut command_properties = HashMap::new();
    command_properties.insert(
        "fmsVer".to_string(),
        Amf0Value::Utf8String("fms".to_string()),
    );
    command_properties.insert("capabilities".to_string(), Amf0Value::Number(31.0));

    let mut additional_properties = HashMap::new();
    additional_properties.insert(
        "level".to_string(),
        Amf0Value::Utf8String("status".to_string()),
    );
    additional_properties.insert(
        "code".to_string(),
        Amf0Value::Utf8String("NetConnection.Connect.Success".to_string()),
    );
    additional_properties.insert(
        "description".to_string(),
        Amf0Value::Utf8String("hi".to_string()),
    );
    additional_properties.insert("objectEncoding".to_string(), Amf0Value::Number(0.0));

    let message = RtmpMessage::Amf0Command {
        command_name: "_result".to_string(),
        transaction_id: 1.0,
        command_object: Amf0Value::Object(command_properties),
        additional_arguments: vec![Amf0Value::Object(additional_properties)],
    };

    let payload = message
        .into_message_payload(RtmpTimestamp::new(0), 0)
        .unwrap();
    serializer.serialize(&payload, false, false).unwrap()
}