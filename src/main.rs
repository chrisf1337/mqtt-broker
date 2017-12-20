#![feature(use_nested_groups)]
extern crate libmqtt;
extern crate mqttc;
extern crate netopt;

use netopt::{NetworkOptions};
use mqttc::{ClientOptions, PubSub};
use libmqtt::{ctrlpkt::*, ctrlpkt::CtrlPkt::*, error::*};
use std::collections::hash_map::HashMap;
use std::sync::{Mutex, Arc};
use std::io::Write;
use std::net::{TcpStream, TcpListener};
use std::thread;

fn handle_client(mut stream: TcpStream, sessions: Arc<Mutex<HashMap<String, Session>>>) -> Result<()> {
    loop {
        match CtrlPkt::deserialize(&mut stream) {
            Ok(Connect {
                connect_flags,
                keep_alive,
                client_id,
                will_topic,
                will_message,
                username,
                password
            }) => {
                let mut sessions = sessions.lock().unwrap();
                if connect_flags.contains(ConnectFlags::CLEAN_SESSION) {
                    sessions.insert(client_id.clone(), Session::new());
                }
                let (session_present, return_code) =
                    if connect_flags.contains(ConnectFlags::CLEAN_SESSION) ||
                        !sessions.contains_key(&client_id) {
                            (false, ConnAckRetCode::Accepted)
                        } else {
                            (true, ConnAckRetCode::Accepted)
                        };
                let buf = CtrlPkt::ConnAck { session_present, return_code }.serialize()?;
                stream.write_all(&buf)?;
            }
            Ok(PingReq) => {
                println!("received PingReq");
                stream.write_all(&(PingResp.serialize()?))?
            }
            Err(Error::InvalidProtocol) => {
                stream.write_all(&(CtrlPkt::ConnAck {
                    session_present: false,
                    return_code: ConnAckRetCode::UnacceptableProtocolVer
                }.serialize()?))?;
                return Err(Error::CloseNetworkConn);
            },
            e@_ => {
                println!("{:?}", e);
                return Err(Error::CloseNetworkConn);
            }
        }
    }
    Ok(())
}

struct Session {
    subscriptions: Vec<i32>
}

impl Session {
    fn new() -> Session {
        Session {
            subscriptions: vec![]
        }
    }
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:1883").unwrap();
    let sessions: Arc<Mutex<HashMap<String, Session>>> = Arc::new(Mutex::new(HashMap::new()));
    let th = thread::spawn(move || {
        for stream in listener.incoming() {
            let sessions = Arc::clone(&sessions);
            match stream {
                Ok(stream) => {
                    // Make read calls block
                    stream.set_read_timeout(None);
                    thread::spawn(move || {
                        match handle_client(stream, sessions) {
                            Ok(_) => println!("handle_client exited with Ok"),
                            Err(e) => println!("handle_client exited with error: {:?}", e)
                        }
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
        .set_client_id("".to_string())
        .set_keep_alive(1);
    let mut client = opts.connect("127.0.0.1:1883", netopt).expect("Can't connect to server");
    loop {
        match client.await().unwrap() {
            Some(message) => println!("{:?}", message),
            None => {
                println!(".");
            }
        }
    }
    let _ = th.join();
}
