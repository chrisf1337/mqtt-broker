#![feature(use_nested_groups)]
extern crate libmqtt;
extern crate mqttc;
extern crate netopt;

use netopt::{NetworkOptions};
use mqttc::{ClientOptions};
use libmqtt::{ctrlpkt::*, ctrlpkt::CtrlPkt::*, error::*};
use std::collections::hash_map::HashMap;
use std::sync::{RwLock, Arc};
use std::io::Write;
use std::net::{TcpStream, TcpListener};
use std::thread;

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

struct Message {
    qos_lv: QosLv,
    data: Vec<u8>
}

fn handle_client(mut stream: TcpStream,
                 sessions: Arc<RwLock<HashMap<String, Session>>>,
                 retained_msgs: Arc<RwLock<HashMap<String, Message>>>) -> Result<()> {
    loop {
        match match CtrlPkt::deserialize(&mut stream) {
            Ok(Connect {
                connect_flags,
                keep_alive,
                client_id,
                will_topic,
                will_message,
                username,
                password
            }) => {
                println!("Received {:?}", Connect {
                    connect_flags,
                    keep_alive,
                    client_id: client_id.clone(),
                    will_topic: will_topic.clone(),
                    will_message: will_message.clone(),
                    username: username.clone(),
                    password: password.clone()
                });
                let mut sessions = sessions.write().unwrap();
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
                stream.write_all(&buf).and_then(|()| Ok(()))
            }
            Ok(Publish { dup, qos_lv, retain, topic_name, pkt_id, payload }) => {
                println!("Received {:?}", Publish {
                    dup,
                    qos_lv,
                    retain,
                    topic_name: topic_name.clone(),
                    pkt_id: pkt_id.clone(),
                    payload: payload.clone()
                });
                if retain {
                    let mut retained_msgs = retained_msgs.write().unwrap();
                    retained_msgs.insert(topic_name.clone(), Message { qos_lv, data: payload });
                }
                match qos_lv {
                    QosLv::AtMostOnce => Ok(()),
                    QosLv::AtLeastOnce => stream.write_all(&(PubAck(pkt_id.unwrap()).serialize()?))
                        .and_then(|()| Ok(())),
                    QosLv::ExactlyOnce => stream.write_all(&(PubRec(pkt_id.unwrap()).serialize()?))
                        .and_then(|()| Ok(()))
                }
            }
            Ok(pkt@PingReq) => {
                println!("Received {:?}", pkt);
                stream.write_all(&(PingResp.serialize()?)).and_then(|()| Ok(()))
            }
            Ok(pkt@Disconnect) => {
                println!("Received {:?}", pkt);
                return Ok(());
            }
            Ok(pkt@_) => {
                println!("Received {:?}", pkt);
                return Err(Error::UnimplementedPkt(pkt))
            }
            Err(e@Error::InvalidProtocol) => {
                stream.write_all(&(CtrlPkt::ConnAck {
                    session_present: false,
                    return_code: ConnAckRetCode::UnacceptableProtocolVer
                }.serialize()?))?;
                return Err(e);
            }
            Err(e) => {
                println!("{:?}", e);
                return Err(e);
            }
        } {
            Err(e) => return Err(Error::from(e)),
            _ => ()
        }
    }
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:1883").unwrap();
    let sessions: Arc<RwLock<HashMap<String, Session>>> = Arc::new(RwLock::new(HashMap::new()));
    let retained_msgs: Arc<RwLock<HashMap<String, Message>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let th = thread::spawn(move || {
        for stream in listener.incoming() {
            let sessions = Arc::clone(&sessions);
            let retained_msgs = Arc::clone(&retained_msgs);
            match stream {
                Ok(stream) => {
                    // Make read calls block
                    let _ = stream.set_read_timeout(None).unwrap();
                    thread::spawn(move || {
                        match handle_client(stream, sessions, retained_msgs) {
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
    // let _ = th.join();
}
