pub mod connection;
pub mod rtmp_clients;

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
