pub mod connection;
pub mod rtmp_clients;

macro_rules! trace {
    ($($args: expr),*) => {
        print!("TRACE: file: {}, line: {}", file!(), line!());
        $(
            print!(", {}: {}", stringify!($args), $args);
        )*
        println!(""); // to get a new line at the end
    }
}

use connection::{Connection, ReadResult, ConnectionError};
use rtmp_clients::MyClientSessionConfig;

use rml_amf0::Amf0Value;
use rml_rtmp::{
    chunk_io::{ChunkDeserializer, ChunkSerializer, Packet},
    messages::{MessagePayload, RtmpMessage},
    sessions::{
        ClientSession, ClientSessionEvent, ClientSessionResult,
        PublishRequestType,
    },
    time::RtmpTimestamp,
};

use slab::Slab;
use mio::{Poll, Token, Events};
use mio::net::TcpStream;
use mio::*;

use std::{
    collections::{HashMap, HashSet},
    net::ToSocketAddrs
};

type ClosedTokens = HashSet<usize>;

static LOG_DEBUG_LOGIC: bool = true;

pub struct MyClientSession{

}

impl MyClientSession {

    pub fn new() -> Self {
        Self{}
    }

    pub fn new_session_and_successful_connect_creates_set_chunk_size_message(&mut self
    ) -> Result<(ClientSession, ChunkSerializer, ChunkDeserializer), RtmpError> {
        let mut client = PushClient::new();

        let mut config = MyClientSessionConfig::default();
        config.set_chunk_size(4096);
        config.set_flash_version("test");

        //client.push_app = YOUTUBE_APP.to_string();
        client.push_app = config.get_app();

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
                                match self.process_event(&event.readiness(), &mut connections, token, &mut poll) {
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
    /*
                                                rtmp_connections = handle_read_bytes(
                                                    &buffer[..byte_count],
                                                    token,
                                                    &mut connections,
                                                    &mut poll
                                                );
    */
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

                self.perform_successful_connect(
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
                            self.consume_results(&mut deserializer, vec![results]);
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

    fn consume_results(&mut self, deserializer: &mut ChunkDeserializer, results: Vec<ClientSessionResult>) {
        // Needed to keep the deserializer up to date
        let results = self.split_results(deserializer, results);
    }

    fn split_results(&mut self,
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

    fn process_event(
        &mut self,
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

    fn perform_successful_connect(
        &mut self,
        app_name: String,
        session: &mut ClientSession,
        serializer: &mut ChunkSerializer,
        deserializer: &mut ChunkDeserializer,
    ) {
        let results = session.request_connection(app_name);
        match results {
            Ok(results) => {
                println!("\n\nrequest_connection ok:{:02x?}", results);
                self.consume_results(deserializer, vec![results]);
                let response = self.get_connect_success_response(serializer);
                let results = session.handle_input(&response.bytes[..]);
                match results {
                    Ok(results) => {
                        let (_, mut events) = self.split_results(deserializer, results);
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

    fn get_connect_success_response(&mut self, serializer: &mut ChunkSerializer) -> Packet {
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

    /*
    /// Takes in bytes that are encoding RTMP chunks and returns any responses or events that can
    /// be reacted to.
    pub fn handle_input(
        &mut self,
        bytes: &[u8],
    ) -> Result<Vec<ServerSessionResult>, ServerSessionError> {
        let mut results = Vec::new();
        self.bytes_received += bytes.len() as u64;

        if let Some(peer_ack_size) = self.peer_window_ack_size {
            self.bytes_received_since_last_ack += bytes.len() as u32;
            if self.bytes_received_since_last_ack >= peer_ack_size {
                let ack_message = RtmpMessage::Acknowledgement {
                    sequence_number: self.bytes_received_since_last_ack,
                };
                let ack_payload = ack_message.into_message_payload(self.get_epoch(), 0)?;
                let ack_packet = self.serializer.serialize(&ack_payload, false, false)?;

                self.bytes_received_since_last_ack = 0;
                results.push(ServerSessionResult::OutboundResponse(ack_packet));
            }
        }

        let mut bytes_to_process = bytes;

        loop {
            match self.deserializer.get_next_message(bytes_to_process)? {
                None => break,
                Some(payload) => {
                    let message = payload.to_rtmp_message()?;

                    let mut message_results = match message {
                        RtmpMessage::Abort { stream_id } => self.handle_abort_message(stream_id)?,

                        RtmpMessage::Acknowledgement { sequence_number } => {
                            self.handle_acknowledgement_message(sequence_number)?
                        }

                        RtmpMessage::Amf0Command {
                            command_name,
                            transaction_id,
                            command_object,
                            additional_arguments,
                        } => self.handle_amf0_command(
                            payload.message_stream_id,
                            command_name,
                            transaction_id,
                            command_object,
                            additional_arguments,
                        )?,

                        RtmpMessage::Amf0Data { values } => {
                            self.handle_amf0_data(values, payload.message_stream_id)?
                        }

                        RtmpMessage::AudioData { data } => self.handle_audio_data(
                            data,
                            payload.message_stream_id,
                            payload.timestamp,
                        )?,

                        RtmpMessage::SetChunkSize { size } => self.handle_set_chunk_size(size)?,

                        RtmpMessage::SetPeerBandwidth { size, limit_type } => {
                            self.handle_set_peer_bandwidth(size, limit_type)?
                        }

                        RtmpMessage::UserControl {
                            event_type,
                            stream_id,
                            buffer_length,
                            timestamp,
                        } => self.handle_user_control(
                            event_type,
                            stream_id,
                            buffer_length,
                            timestamp,
                        )?,

                        RtmpMessage::VideoData { data } => self.handle_video_data(
                            data,
                            payload.message_stream_id,
                            payload.timestamp,
                        )?,

                        RtmpMessage::WindowAcknowledgement { size } => {
                            self.handle_window_acknowledgement(size)?
                        }

                        _ => vec![ServerSessionResult::UnhandleableMessageReceived(payload)],
                    };

                    results.append(&mut message_results);
                    bytes_to_process = &[];
                }
            }
        }

        Ok(results)
    }
    */
}

#[derive(Debug)]
pub enum RtmpError {
    RtmpErrorUnknown,
    SocketAddrFailure,
    HandshakeStartFailure,
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