/*
1. Request a connection to an "application"
2. Request to open a stream to publish on
3. Request access to publish with a particular stream name
*/

use log::error;
use rml_amf0::Amf0Value;
use rml_rtmp::{
    chunk_io::{ChunkDeserializer, ChunkSerializer, Packet},
    messages::{MessagePayload, RtmpMessage},
    sessions::{
        ClientSession, ClientSessionConfig, ClientSessionEvent, ClientSessionResult,
        PublishRequestType,
    },
    time::RtmpTimestamp,
};
use std::{collections::HashMap, fs};

use lazy_static::lazy_static;
use std::sync::Mutex;

#[derive(Clone)]
pub struct MyClientSessionConfig {
    pub config: ClientSessionConfig,
    stream_key: Option<String>,
}

// static APP_NAME: &str = "test rtmp";

static YOUTUBE_CHUNK_SIZE: u32 = 128;
static YOUTUBE_URL: &str = "a.rtmp.youtube.com";
static YOUTUBE_APP: &str = "live2/x";
static YOUTUBE_KEY: &str = "0kjx-g7uh-82dh-vbqc-ct1p";

impl MyClientSessionConfig {
    fn new() -> Self {
        Self {
            config: ClientSessionConfig::new(),
            stream_key: None,
        }
    }

    pub fn default() -> Self {
        let mut var = Self::custom_config(Some(YOUTUBE_CHUNK_SIZE), Some(YOUTUBE_URL));
        var.set_stream_key(Some(YOUTUBE_KEY));
        return var;
    }

    pub fn custom_config(chunk_size: Option<u32>, tc_url: Option<&str>) -> Self {
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
        if stream_key.is_some() {
            self.stream_key = Some(stream_key.unwrap().to_string());
        } else {
            self.stream_key = None;
        }
    }

    pub fn get_stream_key(&self) -> Option<String> {
        return self.stream_key.clone();
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
    static ref CLIENT_CONFIG: Mutex<MyClientSessionConfig> =
        Mutex::new(MyClientSessionConfig::default());
}

fn new_session_and_successful_connect_creates_set_chunk_size_message() {
    let app_name = YOUTUBE_APP.to_string();
    let mut config = CLIENT_CONFIG.lock().unwrap();
    config.set_chunk_size(4096);
    config.set_flash_version("test");

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

            let stream_key = config.get_stream_key().unwrap_or_else(|| {
                panic!("Missing Stream Key, error");
            });

            println!("Go to `request_publishing`: {}", stream_key);

            let results = session.request_publishing(stream_key, PublishRequestType::Live);
            match results {
                Ok(results) => {
                    println!("Consume results after 'request_publishing'");
                    consume_results(&mut deserializer, vec![results]);
                }
                Err(err) => {
                    println!("session.request_publishing error: {:?}", err);
                }
            }
        }
        Err(err) => {
            println!("ClientSessionError: {:?}", err);
        }
    }
}

fn consume_results(deserializer: &mut ChunkDeserializer, results: Vec<ClientSessionResult>) {
    // Needed to keep the deserializer up to date
    let results = split_results(deserializer, results);

    let (messages, events) = results;
    for event in events {
        println!("consume_results, event:{:?}", event);
    }
    for message in messages {
        println!("consume_results, message:{:?}", message);
    }
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
                match message.clone() {
                    RtmpMessage::SetChunkSize { size } => {
                        deserializer.set_max_chunk_size(size as usize).unwrap()
                    }
                    other => {
                        println!("\n**********\nother message\n**********\n")
                    }
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
            println!("\n\nrequest_connection ok:{:02x?}", results);
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
                    println!("session.handle_input error: {:?}", err);
                }
            }
        }
        Err(err) => {
            println!("session.request_connection error: {:?}", err);
        }
    }
}

fn get_connect_success_response(serializer: &mut ChunkSerializer) -> Packet {
    print!("!!! get_connect_success_response");

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

/******************************************/

use std::io::Write;

use rscam::{Camera, Config, Frame};

use openh264::encoder::{Encoder, EncoderConfig, RateControlMode};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;

use crossbeam_channel::unbounded;
use crossbeam_channel::Receiver;
use std::thread;

static X_RES: u32 = 1280;
static Y_RES: u32 = 720;
static FRAME_RATE: (u32, u32) = (1, 30); // 30 fps.
static DEFAULT_BITRATE: u32 = 2_000_000;

fn receiver(rx: Receiver<MyEncodedFrame>) {
    //    new_session_and_successful_connect_creates_set_chunk_size_message();

    let mut file = fs::File::create("rx-thread.h264").unwrap();
    loop {
        let frame = rx.recv();

        if let Err(err) = frame {
            error!("rx fail: {:#?}", err);
        } else {
            file.write_all(&frame.unwrap().payload);
        }
    }
}

fn encode(rx_enc: Receiver<MyFrame>) {
    let (tx, rx) = unbounded();
    thread::spawn(move || {
        receiver(rx);
    });

    let encoder = create_encoder();

    if let Err(err) = encoder {
        error!("Failed to invoke encoder:{}", err);
    } else {
        let mut encoder = encoder.unwrap();
        loop {
            let yuv_source = rx_enc.recv();
            if let Err(err) = yuv_source {
                error!("yuv_source rx fail: {:#?}", err);
            } else {
                let yuv_source = yuv_source.unwrap();
                let bitstream = encoder.encode(&yuv_source.payload);
                if let Err(err) = bitstream {
                    error!("failed bitstream:{}", err);
                } else {
                    let bs = bitstream.unwrap();

                    // print frame type
                    //          println!("{:#?}", bs.frame_type());

                    // send to a different thread
                    let frame = MyEncodedFrame::build(bs.to_vec(), yuv_source.timestamp);
                    let tx_res = tx.send(frame);
                    if let Err(err) = tx_res {
                        error!("tx fail: {:#?}", err);
                    }
                }
            }
        }
    }
}

fn main() {
    let (tx_cam, rx_enc) = unbounded();
    thread::spawn(move || {
        encode(rx_enc);
    });

    let camera = create_camera().unwrap();

    loop {
        let frame = camera.capture().unwrap();
        let yuv_source = MyFrame::build(&frame, frame.get_timestamp());

        tx_cam.send(yuv_source);
    }
}

fn create_camera() -> Option<Camera> {
    let mut camera = Camera::new("/dev/video0").unwrap();

    let res = camera.start(&Config {
        interval: FRAME_RATE,
        resolution: (X_RES, Y_RES),
        format: b"RGB3", // b"YU12",  // b"MJPG",
        ..Default::default()
    });
    if let Err(_) = res {
        return None;
    }

    let formats = camera.formats();
    for format in formats {
        if let Ok(format) = format {
            println!("description: {}", format.description);
            let fourcc = std::str::from_utf8(&format.format).unwrap();
            println!("fourcc:{}", fourcc);
        }
    }

    Some(camera)
}

fn create_encoder() -> Result<Encoder, openh264::Error> {
    let encoder_config = EncoderConfig::new(X_RES, Y_RES);
    encoder_config.set_bitrate_bps(DEFAULT_BITRATE);
    encoder_config.max_frame_rate(30.0);
    encoder_config.rate_control_mode(RateControlMode::Quality);
    encoder_config.enable_skip_frame(true);

    let api = OpenH264API::from_source();
    let encoder = Encoder::with_config(api, encoder_config);

    return encoder;
}

struct MyFrame {
    payload: YUVBuffer,
    timestamp: u64,
}

impl MyFrame {
    pub fn build(frame: &Frame, ts: u64) -> Self {
        let w = frame.resolution.0 as usize;
        let h = frame.resolution.1 as usize;

        Self {
            payload: YUVBuffer::with_rgb(w, h, &frame[..]),
            timestamp: ts
        }
    }
}

struct MyEncodedFrame {
    payload: Vec<u8>,
    timestamp: u64,
}

impl MyEncodedFrame {
    pub fn build(frame: Vec<u8>, ts: u64) -> Self {
        Self {
            payload: frame,
            timestamp: ts
        }
    }
}