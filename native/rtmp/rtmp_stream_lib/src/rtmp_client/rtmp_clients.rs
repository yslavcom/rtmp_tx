
use rml_rtmp::handshake::Handshake;
use rml_rtmp::handshake::PeerType;
use slab::Slab;
use std::net::SocketAddr;
use std::str::FromStr;

use mio::net::TcpStream;
use mio::{Poll, Token, Events};
use mio::*;

use log::error;
use rml_amf0::{serialize, Amf0Value};
use rml_rtmp::{
    chunk_io::{ChunkDeserializer, ChunkSerializer, Packet},
    messages::{MessagePayload, RtmpMessage},
    sessions::{
        ClientSession, ClientSessionConfig, ClientSessionEvent, ClientSessionResult,
        PublishRequestType,
    },
    time::RtmpTimestamp,
};
use std::{
    collections::{HashMap, HashSet},
    fs, result,
    net::ToSocketAddrs
};

// mod connection;
use super::connection::{Connection, ReadResult, ConnectionError};

macro_rules! trace {
    ($($args: expr),*) => {
        print!("TRACE: file: {}, line: {}", file!(), line!());
        $(
            print!(", {}: {}", stringify!($args), $args);
        )*
        println!(""); // to get a new line at the end
    }
}

#[derive(Clone)]
pub struct MyClientSessionConfig {
    pub config: ClientSessionConfig,
    stream_key: Option<String>,
}

static LOG_DEBUG_LOGIC: bool = true;

static YOUTUBE_CHUNK_SIZE: u32 = 128;

//static YOUTUBE_DEFAULT_SERVER: &str = "rtmp://a.rtmp.youtube.com:1935";
static YOUTUBE_DEFAULT_SERVER: &str = "a.rtmp.youtube.com:1935";
static YOUTUBE_APP: &str = "live2/x";
static YOUTUBE_KEY: &str = "0kjx-g7uh-82dh-vbqc-ct1p";
//static YOUTUBE_APP_OPTION: &str = " app=live2";
static YOUTUBE_APP_OPTION: &str = "live2";

/*
        defaultProtocol = Protocol::RTMP;
        defaultServer = "a.rtmp.youtube.com";
        mApp = "live2/x";
        if (APP_OPTIONS_IN_URL)
        {
            mOptions = " app=live2";
        }
        else
        {
            mAppOption = "live2";
        }

void StreamingConfig::composeURL()
{
    mURL = std::string(protocolString()).append(mServer)
                        .append("/").append(mApp)
                        .append("/").append(mKey)
                        .append(mOptions);
}
*/

type ClosedTokens = HashSet<usize>;

impl MyClientSessionConfig {
    fn new() -> Self {
        Self {
            config: ClientSessionConfig::new(),
            stream_key: None,
        }
    }

    pub fn default() -> Self {
        let mut var = Self::new();
        var.set_url(YOUTUBE_DEFAULT_SERVER);
        var.set_stream_key(Some(YOUTUBE_KEY));
        return var;
    }
/*
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
*/
    pub fn set_url(&mut self, tc_url: &str) {
        self.config.tc_url = Some(tc_url.to_string());
    }

    pub fn get_url(&self) -> Option<String> {
        self.config.tc_url.clone()
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

fn consume_results(deserializer: &mut ChunkDeserializer, results: Vec<ClientSessionResult>) {
    // Needed to keep the deserializer up to date
    let results = split_results(deserializer, results);
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
                    other => {}
                }
                responses.push((payload, message));
            }

            ClientSessionResult::RaisedEvent(event) => {
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
                    trace!("session.handle_input error: {:?}", err);
                }
            }
        }
        Err(err) => {
            trace!("session.request_connection error: {:?}", err);
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

#[derive(Debug)]
pub enum RtmpError {
    RtmpErrorUnknown,
    SocketAddrFailure,
    HandshakeStartFailure,
}

/*******************************************************
 *
*/
//const CLIENT: Token = Token(std::usize::MAX - 1);

pub fn new_session_and_successful_connect_creates_set_chunk_size_message(
) -> Result<(ClientSession, ChunkSerializer, ChunkDeserializer), RtmpError> {
    let mut client = PushClient::new();

    let mut config = MyClientSessionConfig::default();
    config.set_chunk_size(4096);
    config.set_flash_version("test");

    client.push_app = YOUTUBE_APP.to_string();

    let mut connections = Slab::new();
    let mut poll = Poll::new().unwrap();
//    poll.register(&listener, CLIENT, Ready::readable(), PollOpt::edge()).unwrap();


    println!("Listening for connections");

    let mut deserializer = ChunkDeserializer::new();
    let mut serializer = ChunkSerializer::new();
    let results = ClientSession::new(config.config.clone());

    match results {
        Ok((mut session, initial_results)) => {
            //            println!("initial_results:{:#?}", initial_results);
            //            consume_results(&mut deserializer, initial_results);
            if config.get_url().is_some_and(|val| val.len() > 1) {
                let push_host = config.get_url();
                if push_host.is_none() {
                    println!("Failed to retrieve url");
                    return Err(RtmpError::RtmpErrorUnknown);
                } else {
                    let push_host = push_host.unwrap();

                    // Gracefully itenrate through available SocketAddr
                    let server: Vec<_>= push_host
                        .to_socket_addrs()
                        .expect("Unable to resolve domain")
                        .collect();
                    if !server.is_empty() {
                        let addr = server[0] ;
                        //let addr = SocketAddr::from_str(&push_host);

                        let stream = TcpStream::connect(&addr).unwrap();
                        let mut connection_count = 1;
                        let connection = Connection::new(stream, connection_count, LOG_DEBUG_LOGIC, false);
                        let token = connections.insert(connection);

                        println!("Pull client started with connection id {}", token);
                        connections[token].token = Some(Token(token));
                        connections[token].register(&mut poll).unwrap();

                        client.state = PushState::Handshaking;
                        client.set_token(token);
                    }
                    else {
                        trace!("Failed to match SocketAddr with push_host = {}", push_host);
                        return Err(RtmpError::SocketAddrFailure);
                    }
                }
            }

            // handshaking here
            let client_token = client.get_token().unwrap();
            let res = connections[client_token].writable(&mut poll);
            match res {
                Ok(_) => {
                    println!("writable OK");
                },
                Err(error) => {
                    trace!("writable failed:{}", error);
                    return Err(RtmpError::HandshakeStartFailure)
                },
            }

            let mut events = Events::with_capacity(1024);

            loop {
                poll.poll(&mut events, None).unwrap();

                for event in events.iter() {
                    println!("event.token:{:#?}", event.token());

                    let mut rtmp_connections = ClosedTokens::new();

                    match event.token() {
                        Token(token) => {
                            match process_event(&event.readiness(), &mut connections, token, &mut poll) {
                                EventResult::None => (),
                                EventResult::DisconnectConnection => {
                                    println!("EventResult::DisconnectConnection");
                                },

                                EventResult::ReadResult(result) => {
                                    match result {
                                        ReadResult::HandshakingInProgress => {
                                            println!("HandshakingInProgress");
                                        },
                                        ReadResult::HandshakeCompleted{buffer, byte_count} => {
                                            println!("HandshakeCompleted, byte_count:{}", byte_count);
                                            rtmp_connections = handle_read_bytes(
                                                &buffer[..byte_count],
                                                token,
                                                &mut connections,
                                                &mut poll
                                            );

                                        },
                                        ReadResult::NoBytesReceived => (),
                                        ReadResult::BytesReceived{buffer, byte_count} =>{
                                            println!("BytesReceived, byte_count:{}", byte_count);
                                        },
                                    };
                                },
                            };
                        },
                    };
                }
            }

            perform_successful_connect(
                client.push_app.clone(),
                &mut session,
                &mut serializer,
                &mut deserializer,
            );
            client.session = Some(session);

            let stream_key = config.get_stream_key().unwrap_or_else(|| {
                panic!("Missing Stream Key, error");
            });

            let mut session = client.session;
            if let Some(mut session) = session {
                println!("Go to `request_publishing`: {}", stream_key);

                let results = session.request_publishing(stream_key, PublishRequestType::Live);
                match results {
                    Ok(results) => {
                        println!("Consume results after 'request_publishing'");
                        consume_results(&mut deserializer, vec![results]);
                        return Ok((session, serializer, deserializer));
                    }
                    Err(err) => {
                        trace!("session.request_publishing error: {:?}", err);
                        return Err(RtmpError::RtmpErrorUnknown);
                    }
                }
            }
        }
        Err(err) => {
            trace!("ClientSessionError: {:?}", err);
            return Err(RtmpError::RtmpErrorUnknown);
        }
    }
    Err(RtmpError::RtmpErrorUnknown)
}

enum PushState {
    Idle,
    WaitingConnection,
    Handshaking,
    Connecting,
    Connected,
    Publishing,
}

struct PushClient {
    session: Option<ClientSession>,
    connection_id: Option<usize>,
    push_app: String,
    push_source_stream: String,
    push_target_stream: String,
    state: PushState,
    token: Option<usize>,
}

impl PushClient {
    pub fn new() -> Self {
        return Self {
            session: None,
            connection_id: None,
            push_app: "".to_string(),
            push_source_stream: "".to_string(),
            push_target_stream: "".to_string(),
            state: PushState::Idle,
            token: None,
        };
    }

    pub fn set_token(&mut self, token: usize) {
        self.token = Some(token);
    }

    pub fn get_token(&self) -> Option<usize> {
        return self.token;
    }
}

enum EventResult {
    None,
    ReadResult(ReadResult),
    DisconnectConnection,
}

fn process_event(
    event: &Ready,
    connections: &mut Slab<Connection>,
    token: usize,
    poll: &mut Poll,
) -> EventResult {
    let connection = match connections.get_mut(token) {
        Some(connection) => connection,
        None => return EventResult::None,
    };

    if event.is_writable() {
        match connection.writable(poll) {
            Ok(_) => (),
            Err(error) => {
                println!("Error occurred while writing: {:?}", error);
                return EventResult::DisconnectConnection;
            }
        }
    }

    if event.is_readable() {
        match connection.readable(poll) {
            Ok(result) => return EventResult::ReadResult(result),
            Err(ConnectionError::SocketClosed) => return EventResult::DisconnectConnection,
            Err(x) => {
                println!("Error occurred: {:?}", x);
                return EventResult::DisconnectConnection;
            }
        }
    }

    EventResult::None
}

fn handle_read_bytes(
    bytes: &[u8],
    from_token: usize,
    connections: &mut Slab<Connection>,
    poll: &mut Poll,
) -> ClosedTokens {
    let mut closed_tokens = ClosedTokens::new();

    handle_input(bytes);

    closed_tokens
}