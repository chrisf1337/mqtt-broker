#![feature(use_nested_groups)]
extern crate libmqtt;
extern crate mqttc;
extern crate netopt;

use netopt::{NetworkOptions};
use mqttc::{ClientOptions};
use libmqtt::{ctrlpkt::*, error::*};
use std::net::{TcpStream, TcpListener};
use std::thread;

fn handle_client(stream: TcpStream) -> Result<CtrlPkt> {
    let res = CtrlPkt::deserialize(stream);
    println!("{:?}", res);
    res
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:1883").unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    thread::spawn(move || {
                        handle_client(stream)
                    });
                }
                Err(e) => println!("{}", e)
            }
        }
    });
    let netopt = NetworkOptions::new();
    let mut opts = ClientOptions::new();
    opts.set_username("username".to_string())
        .set_password("password".to_string())
        .set_client_id("".to_string());
    let _ = opts.connect("127.0.0.1:1883", netopt).expect("Can't connect to server");
}
