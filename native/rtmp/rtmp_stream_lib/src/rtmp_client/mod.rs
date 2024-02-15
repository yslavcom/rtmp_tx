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
        ClientSession, ClientSessionConfig, ClientSessionEvent, ClientSessionResult, PublishRequestType
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
    connection_count: usize,
    push_client: Option<PushClient>,
    config: MyClientSessionConfig,
}

impl MyClientSession {

    pub fn new() -> Self {
        let s = Self{
            connection_count: 0,
            push_client: Some(PushClient::new()),
            config: MyClientSessionConfig::default(),
        };

        match s.push_client{
            Some(_) => println!("!!! new, push_client Some"),
            None => println!("!!! new, push_client None"),
        };
        return s;
    }

    pub fn new_session_and_successful_connect_creates_set_chunk_size_message(&mut self
    ) -> Result<(ClientSession, ChunkSerializer, ChunkDeserializer), RtmpError> {
        self.config.set_chunk_size(4096);
        self.config.set_flash_version("test");

        //client.push_app = YOUTUBE_APP.to_string();
        self.push_client.as_mut().map(|c|c.push_app = self.config.get_app());

        let mut connections = Slab::new();
        let mut poll = Poll::new().unwrap();
    //    poll.register(&listener, CLIENT, Ready::readable(), PollOpt::edge()).unwrap();


        println!("Listening for connections");

        let mut deserializer = ChunkDeserializer::new();
        let mut serializer = ChunkSerializer::new();
        let results = ClientSession::new(self.config.config.clone());

        match results {
            Ok((mut session, initial_results)) => {
                if self.config.get_url().is_some_and(|val| val.len() > 1) {
                    let push_host = self.config.get_url();
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
                            self.connection_count = 1;
                            let connection: Connection = Connection::new(stream, self.connection_count, LOG_DEBUG_LOGIC);
                            let token = connections.insert(connection);

                            println!("Pull client started with connection id {}", token);
                            connections[token].token = Some(Token(token));
                            connections[token].register(&mut poll).unwrap();
                            self.push_client.as_mut().map(|c| c.connection_id = Some(token));
                            self.push_client.as_mut().map(|s| s.state = PushState::Handshaking);
                        }
                        else {
                            trace!("Failed to match SocketAddr with push_host = {}", push_host);
                            return Err(RtmpError::SocketAddrFailure);
                        }
                    }
                }

                // handshaking here, we know the push_client & the token have been set
                let client_token = self.push_client.as_ref().map(|s| s.connection_id.unwrap()).unwrap();
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
                                                rtmp_connections = self.handle_read_bytes(
                                                    &buffer[..byte_count],
                                                    token,
                                                    &mut connections,
                                                    &mut poll,
                                                );
                                            },
                                            ReadResult::NoBytesReceived => (),
                                            ReadResult::BytesReceived{buffer, byte_count} =>{
                                                println!("BytesReceived, byte_count:{}", byte_count);
                                                rtmp_connections = self.handle_read_bytes(
                                                    &buffer[..byte_count],
                                                    token,
                                                    &mut connections,
                                                    &mut poll,
                                                );
                                            },
                                        };
                                    },
                                };
                            },
                        };
                    }
                }

                self.perform_successful_connect(
                    self.push_client.unwrap().push_app.clone(),
                    &mut session,
                    &mut serializer,
                    &mut deserializer,
                );
                self.push_client.unwrap().session = Some(session);

                let stream_key = self.config.get_stream_key().unwrap_or_else(|| {
                    panic!("Missing Stream Key, error");
                });

                let mut session = self.push_client.unwrap().session;
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


    fn handle_read_bytes(
        &mut self,
        bytes: &[u8],
        from_token: usize,
        connections: &mut Slab<Connection>,
        poll: &mut Poll,
    ) -> ClosedTokens {
        let mut closed_tokens = ClosedTokens::new();

        let mut server_results = match self.bytes_received(from_token, bytes) {
            Ok(results) => {
                println!("results:{:#?}", results);
                results
            },
            Err(error) => {
                println!("Input caused the following server error: {}", error);
                closed_tokens.insert(from_token);
                return closed_tokens;
            }
        };

        for result in server_results.drain(..) {
            match result {
                ServerResult::OutboundPacket {
                    target_connection_id,
                    packet,
                } => match connections.get_mut(target_connection_id) {
                    Some(connection) => connection.enqueue_packet(poll, packet).unwrap(),
                    None => (),
                },

                ServerResult::DisconnectConnection { connection_id } => {
                    closed_tokens.insert(connection_id);
                }

                ServerResult::StartPushing => {
/*
                    if let Some(ref push) = app_options.push {
                        println!(
                            "Starting push to rtmp://{}/{}/{}",
                            push.host, push.app, push.target_stream
                        );

                        let mut push_host = push.host.clone();
                        if !push_host.contains(":") {
                            push_host = push_host + ":1935";
                        }

                        let addr = SocketAddr::from_str(&push_host).unwrap();
                        let stream = TcpStream::connect(&addr).unwrap();
                        let connection =
                            Connection::new(stream, *connection_count, app_options.log_io, false);
                        let token = connections.insert(connection);
                        *connection_count += 1;

                        println!("Push client started with connection id {}", token);
                        connections[token].token = Some(Token(token));
                        connections[token].register(poll).unwrap();
                        self.register_push_client(token);
                    }
*/
                }
            }
        }

        closed_tokens
    }

    pub fn bytes_received(
        &mut self,
        connection_id: usize,
        bytes: &[u8],
    ) -> Result<Vec<ServerResult>, String> {
        let mut server_results = Vec::new();

        let push_client_connection_id = self.push_client.as_ref().map_or(None, |c| {
            if let Some(connection_id) = c.connection_id {
                Some(connection_id)
            } else {
                None
            }
        });


        match self.push_client{
            Some(_) => println!("!!! bytes_received, push_client Some: {:?}, {:?}", push_client_connection_id, connection_id),
            None => println!("!!! bytes_received, push_client None"),
        };

        if push_client_connection_id
            .as_ref()
            .map_or(false, |id| *id == connection_id)
        {
            // These bytes were received by the current push client
            let mut initial_session_results = Vec::new();

            let session_results = if let Some(ref mut push_client) = self.push_client {
                if push_client.session.is_none() {
                    let (session, session_results) =
                        ClientSession::new(ClientSessionConfig::new()).unwrap();
                    push_client.session = Some(session);

                    for result in session_results {
                        initial_session_results.push(result);
                    }
                }
                match push_client.session.as_mut().map(|s| s.handle_input(bytes)).unwrap() {
                    Ok(results) => results,
                    Err(error) => return Err(error.to_string()),
                }
            } else {
                Vec::new()
            };

            if initial_session_results.len() > 0 {
                self.handle_push_session_results(initial_session_results, &mut server_results);
            }

            self.handle_push_session_results(session_results, &mut server_results);
        }

        Ok(server_results)
    }

    fn handle_push_session_results(
        &mut self,
        session_results: Vec<ClientSessionResult>,
        server_results: &mut Vec<ServerResult>,
    ) {
        println!("!!! handle_push_session_results\n
            session_results:{:#?}\n
            server_results:{:#?}\n", session_results, server_results);

        let mut new_results = Vec::new();
        let mut events = Vec::new();
        if let Some(ref mut client) = self.push_client {
            for result in session_results {
                match result {
                    ClientSessionResult::OutboundResponse(packet) => {
                        server_results.push(ServerResult::OutboundPacket {
                            target_connection_id: client.connection_id.unwrap(),
                            packet,
                        });
                    }

                    ClientSessionResult::RaisedEvent(event) => {
                        events.push(event);
                    }

                    x => println!("Push client result received: {:?}", x),
                }
            }

            match client.state {
                PushState::Handshaking => {
                    // Since we got here we know handshaking was successful, so we need
                    // to initiate the connection process
                    client.state = PushState::Connecting;

                    let result = match client
                        .session
                        .as_mut()
                        .unwrap()
                        .request_connection(client.push_app.clone())
                    {
                        Ok(result) => result,
                        Err(error) => {
                            println!("Failed to request connection for push client: {:?}", error);
                            return;
                        }
                    };

                    new_results.push(result);
                }
                _ => (),
            }
        }

        if !new_results.is_empty() {
            self.handle_push_session_results(new_results, server_results);
        }

        for event in events {
            match event {
                ClientSessionEvent::ConnectionRequestAccepted => {
                    self.handle_push_connection_accepted_event(server_results);
                }

                ClientSessionEvent::PublishRequestAccepted => {
                    self.handle_push_publish_accepted_event(server_results);
                }

                x => println!("Push event raised: {:?}", x),
            }
        }
    }

    fn handle_push_connection_accepted_event(&mut self, server_results: &mut Vec<ServerResult>) {
        let mut new_results = Vec::new();
        if let Some(ref mut client) = self.push_client {
            println!("push accepted for app '{}'", client.push_app);
            client.state = PushState::Connected;

            let result = client
                .session
                .as_mut()
                .unwrap()
                .request_publishing(client.push_target_stream.clone(), PublishRequestType::Live)
                .unwrap();

            let mut results = vec![result];
            new_results.append(&mut results);
        }

        if !new_results.is_empty() {
            self.handle_push_session_results(new_results, server_results);
        }
    }

    fn handle_push_publish_accepted_event(&mut self, server_results: &mut Vec<ServerResult>) {
        let mut new_results = Vec::new();
        if let Some(ref mut client) = self.push_client {
            println!(
                "Publish accepted for push stream key {}",
                client.push_target_stream
            );
            client.state = PushState::Publishing;
/*
            // Send out any metadata or header information if we have any
            if let Some(ref channel) = self.channels.get(&client.push_source_stream) {
                if let Some(ref metadata) = channel.metadata {
                    let result = client
                        .session
                        .as_mut()
                        .unwrap()
                        .publish_metadata(&metadata)
                        .unwrap();
                    new_results.push(result);
                }

                if let Some(ref bytes) = channel.video_sequence_header {
                    let result = client
                        .session
                        .as_mut()
                        .unwrap()
                        .publish_video_data(bytes.clone(), RtmpTimestamp::new(0), false)
                        .unwrap();

                    new_results.push(result);
                }

                if let Some(ref bytes) = channel.audio_sequence_header {
                    let result = client
                        .session
                        .as_mut()
                        .unwrap()
                        .publish_audio_data(bytes.clone(), RtmpTimestamp::new(0), false)
                        .unwrap();

                    new_results.push(result);
                }
            }
*/
        }

        if !new_results.is_empty() {
            self.handle_push_session_results(new_results, server_results);
        }
    }
}

#[derive(Debug)]
pub enum RtmpError {
    RtmpErrorUnknown,
    SocketAddrFailure,
    HandshakeStartFailure,
}

enum PushState {
    Idle,
    Handshaking,
    Connecting,
    Connected,
    Publishing,
}

struct PushClient {
    session: Option<ClientSession>,
    connection_id: Option<usize>,
    push_app: String,
    push_target_stream: String,
    state: PushState,
}

impl PushClient {
    pub fn new() -> Self {
        return Self {
            session: None,
            connection_id: None,
            push_app: "".to_string(),
            push_target_stream: "".to_string(),
            state: PushState::Idle,
        };
    }
}

enum EventResult {
    None,
    ReadResult(ReadResult),
    DisconnectConnection,
}

#[derive(Debug)]
pub enum ServerResult {
    DisconnectConnection {
        connection_id: usize,
    },
    OutboundPacket {
        target_connection_id: usize,
        packet: Packet,
    },
    StartPushing,
}