mod rtmp_client;
use rtmp_client::new_session_and_successful_connect_creates_set_chunk_size_message;

fn main() {
    new_session_and_successful_connect_creates_set_chunk_size_message();
    println!("end");
}


/*
use std::io::prelude::*;
use std::net::TcpStream;

fn main() -> std::io::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:8080")?;

    stream.write(&[1])?;
    stream.read(&mut [0; 128])?;
    Ok(())
} // the stream is closed here
*/