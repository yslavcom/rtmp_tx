use std::env;

mod rtmp_client;

use rtmp_client::MyClientSession;

fn main() {
    env::set_var("RUST_BACKTRACE", "1");

    let mut client_session = MyClientSession::new();
    let ret = client_session.new_session_and_successful_connect_creates_set_chunk_size_message();
    match ret {
        Ok(_) => {
            println!("ok");
        },
        Err(err) => {
            println!("Error {:?}", err);
        }
    }

    println!("end");
}

/*
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::net::{ ToSocketAddrs};

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

fn main() {
    env::set_var("RUST_BACKTRACE", "1");

    //let server_details = "stackoverflow.com:80";
    let server_details = "a.rtmp.youtube.com:1935";

    let server: Vec<_>= server_details
        .to_socket_addrs()
        .expect("Unable to resolve domain")
        .collect();

    let socket_addr = server[0];
    println!("socket_addr={:?}", socket_addr);
}
*/
/*

    // let url_str = "a.rtmp.youtube.com";
    let url_str = "https://www.google.com";
    let url = Url::parse(url_str);
    let host = url.clone().unwrap();
    //let ip = host.parse();//::<IpAddr>();

    println!("ip={:?}/ {:?}", url, host);
*/

//    let listener = TcpListener::bind("127.0.0.1:8000"/*socket_addr*/).expect("Failed to bind");
//
//    println!("Server listening on {:?}", listener);



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