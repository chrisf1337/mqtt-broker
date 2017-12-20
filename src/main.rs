#![feature(use_nested_groups)]
extern crate libmqtt;
extern crate mqttc;
extern crate netopt;

use netopt::{NetworkOptions};
use mqttc::{ClientOptions, PubSub};
use libmqtt::{ctrlpkt::*, ctrlpkt::CtrlPkt::*, error::*, pktid::*};
use std::collections::{hash_map::HashMap, vec_deque::VecDeque};
use std::sync::{RwLock, Arc, Mutex};
use std::io::Write;
use std::net::{TcpStream, TcpListener};
use std::thread;

#[derive(Debug, Clone)]
struct Session {
    pub client_id: String,
    pub subscriptions: HashMap<String, QosLv>,
    pub waiting_for_ack: VecDeque<Message>,
    pub pending_tx: VecDeque<Message>,
    pub clean_session: bool
}

impl Session {
    fn new(client_id: String, clean_session: bool) -> Session {
        Session {
            client_id,
            subscriptions: HashMap::new(),
            waiting_for_ack: VecDeque::new(),
            pending_tx: VecDeque::new(),
            clean_session
        }
    }
}

#[derive(Debug, Clone)]
struct Message {
    qos_lv: QosLv,
    data: Vec<u8>
}

fn handle_client(mut stream: TcpStream,
                 sessions: Arc<RwLock<HashMap<String, Session>>>,
                 retained_msgs: Arc<RwLock<HashMap<String, Message>>>,
                 subscriptions: Arc<RwLock<HashMap<String, HashMap<String, QosLv>>>>,
                 pkt_id_gen: Arc<Mutex<PktIdGen>>) -> Result<()> {
    let mut session: Option<Session> = None;
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
                let (session_present, return_code) =
                    if connect_flags.contains(ConnectFlags::CLEAN_SESSION) ||
                        !sessions.contains_key(&client_id) {
                            (false, ConnAckRetCode::Accepted)
                        } else {
                            (true, ConnAckRetCode::Accepted)
                        };
                if connect_flags.contains(ConnectFlags::CLEAN_SESSION) {
                    // Clear old session
                    sessions.remove(&client_id);
                    session = Some(Session::new(client_id,
                        connect_flags.contains(ConnectFlags::CLEAN_SESSION)));
                } else {
                    // Get old session or create a new one
                    session = match sessions.get(&client_id) {
                        Some(sess) => Some(sess.clone()),
                        None => Some(Session::new(client_id,
                            connect_flags.contains(ConnectFlags::CLEAN_SESSION)))
                    };
                }
                let buf = CtrlPkt::ConnAck { session_present, return_code }.serialize()?;
                stream.write_all(&buf)
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
                if session.is_none() {
                    return Err(Error::NoSession);
                }
                if retain {
                    let mut retained_msgs = retained_msgs.write().unwrap();
                    retained_msgs.insert(topic_name.clone(), Message { qos_lv, data: payload });
                }
                match qos_lv {
                    QosLv::AtMostOnce => Ok(()),
                    QosLv::AtLeastOnce => stream.write_all(&(PubAck(pkt_id.unwrap())
                        .serialize()?)),
                    QosLv::ExactlyOnce => stream.write_all(&(PubRec(pkt_id.unwrap())
                        .serialize()?))
                }
            }
            Ok(Subscribe { pkt_id, subs }) => {
                println!("Received {:?}", Subscribe {
                    pkt_id,
                    subs: subs.clone()
                });
                if session.is_none() {
                    return Err(Error::NoSession);
                }
                let session = session.as_mut().unwrap();
                let mut subscriptions = subscriptions.write().unwrap();
                let mut sub_ack_ret_codes: Vec<SubAckRetCode> = vec![];
                for (topic_name, requested_qos_lv) in subs {
                    sub_ack_ret_codes.push(if topic_name.contains("*") {
                        SubAckRetCode::Failure
                    } else {
                        session.subscriptions.insert(topic_name.clone(), requested_qos_lv);
                        match match subscriptions.get_mut(&topic_name) {
                            Some(client_to_qos) => {
                                client_to_qos.insert(session.client_id.clone(), requested_qos_lv);
                                None
                            }
                            None => {
                                let mut hm = HashMap::new();
                                hm.insert(session.client_id.clone(), requested_qos_lv);
                                Some(hm)
                            }
                        } {
                            Some(hm) => {
                                subscriptions.insert(topic_name.clone(), hm);
                            }
                            None => ()
                        }
                        SubAckRetCode::from(requested_qos_lv)
                    });
                }
                let pkt = SubAck { pkt_id, sub_ack_ret_codes };
                println!("Response: {:?}", pkt);
                println!("{:?}", session);
                println!("{:?}", subscriptions.clone());
                println!("{:?}", pkt.serialize()?);
                stream.write_all(&(pkt.serialize()?))
            }
            Ok(pkt@PingReq) => {
                println!("Received {:?}", pkt);
                if session.is_none() {
                    return Err(Error::NoSession);
                }
                stream.write_all(&(PingResp.serialize()?))
            }
            Ok(pkt@Disconnect) => {
                println!("Received {:?}", pkt);
                if session.is_none() {
                    return Err(Error::NoSession);
                }
                return Ok(());
            }
            Ok(pkt@_) => {
                println!("Received {:?}", pkt);
                if session.is_none() {
                    return Err(Error::NoSession);
                }
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
    let pkt_id_gen: Arc<Mutex<PktIdGen>> = Arc::new(Mutex::new(PktIdGen::new()));
    let subscriptions: Arc<RwLock<HashMap<String, HashMap<String, QosLv>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let th = thread::spawn(move || {
        for stream in listener.incoming() {
            let sessions = Arc::clone(&sessions);
            let retained_msgs = Arc::clone(&retained_msgs);
            let pkt_id_gen = Arc::clone(&pkt_id_gen);
            let subscriptions = Arc::clone(&subscriptions);
            match stream {
                Ok(stream) => {
                    // Make read calls block
                    let _ = stream.set_read_timeout(None).unwrap();
                    thread::spawn(move || {
                        match handle_client(stream, sessions, retained_msgs, subscriptions,
                            pkt_id_gen) {
                            Ok(_) => println!("handle_client exited with Ok"),
                            Err(e) => println!("handle_client exited with error: {:?}", e)
                        }
                    });
                }
                Err(e) => println!("{}", e)
            }
        }
    });
    let mut client_threads: Vec<thread::JoinHandle<_>> = vec![];
    for i in 0..2 {
        client_threads.push(
            thread::spawn(move || {
                let netopt = NetworkOptions::new();
                let mut opts = ClientOptions::new();
                opts.set_username("username".to_string())
                    .set_password("password".to_string())
                    .set_client_id(format!("client{}", i))
                    .set_keep_alive(1);
                let mut client = opts.connect("127.0.0.1:1883", netopt).expect("Can't connect to server");
                client.subscribe("test_topic").unwrap();
                println!("{:?}", client.await().unwrap());
                loop {
                    match client.await().unwrap() {
                        Some(message) => println!("{:?}", message),
                        None => {
                            println!(".");
                        }
                    }
                }
            })
        )
    }
    for ct in client_threads {
        let _ = ct.join();
    }
    let _ = th.join();
}
