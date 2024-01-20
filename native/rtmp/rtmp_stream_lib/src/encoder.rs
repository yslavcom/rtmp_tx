fn encode(rx_enc: Receiver<MyFrame>) {
    let (tx, rx) = unbounded();
    thread::spawn(move || {
        receiver(rx);
    });

    let encoder = create_encoder();

    if let Err(err) = encoder {
        println!("Failed to invoke encoder:{}", err);
    } else {
        let mut encoder = encoder.unwrap();
        loop {
            let yuv_source = rx_enc.recv();
            if let Err(err) = yuv_source {
                println!("yuv_source rx fail: {:#?}", err);
            } else {
                let yuv_source = yuv_source.unwrap();
                let bitstream = encoder.encode(&yuv_source.payload);
                if let Err(err) = bitstream {
                    println!("failed bitstream:{}", err);
                } else {
                    let bs = bitstream.unwrap();

                    // print frame type
                    //          println!("{:#?}", bs.frame_type());

                    // send to a different thread
                    let frame = MyEncodedFrame::build(bs.to_vec(), yuv_source.timestamp);
                    let tx_res = tx.send(frame);
                    if let Err(err) = tx_res {
                        println!("tx fail: {:#?}", err);
                    }
                }
            }
        }
    }
}

fn receiver(rx: Receiver<MyEncodedFrame>) {
/*
    let res = new_session_and_successful_connect_creates_set_chunk_size_message();
    match res {
        Ok((mut session, mut serializer, mut deserializer)) => {
            let mut file = fs::File::create("rx-thread.h264").unwrap();
            let results = session.send_ping_request();
            if let Err(err) = results {
                println!("Ping fail:{}", err);
            }

            loop {
                let frame = rx.recv();

                if let Err(err) = frame {
                    println!("rx fail: {:#?}", err);
                } else {
                    let frame = frame.unwrap();
                    file.write_all(&frame.payload);
                    let bytes = Bytes::from(frame.payload);
                    let len = bytes.len();
                    let timestamp = (frame.timestamp/1000) as u32;
/*
                    let results = session.publish_video_data(bytes, RtmpTimestamp::new(timestamp), false);
                    println!("publishing:{}", len);
                    match results {
                        Ok(results) => {
                            println!("!!! published:{}", len);
                            consume_results(&mut deserializer, vec![results]);
                        },
                        Err(err) => {
                            println!("Failed to publish video, {}", err);
                        }
                    }
*/
                }
            }
        },
        Err(err) => {
            println!("Failed to create rtmp client");
        }
    }
*/
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
            timestamp: ts,
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
            timestamp: ts,
        }
    }
}

