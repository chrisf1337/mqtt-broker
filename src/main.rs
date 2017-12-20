#![feature(use_nested_groups)]
extern crate libmqtt;
extern crate mqttc;
extern crate netopt;

use netopt::{NetworkOptions};
use mqttc::{ClientOptions, PubSub, PubOpt};
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
    pub waiting_for_ack: VecDeque<(u16, Message)>,
    pub pending_tx: VecDeque<(u16, Message)>,
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
    payload: Vec<u8>
}

fn publish_msg(sender_id: &str,
               topic_name: &str,
               payload: &Vec<u8>,
               streams: &Arc<Mutex<HashMap<String, TcpStream>>>,
               sessions: &Arc<RwLock<HashMap<String, Session>>>,
               subscriptions: &Arc<RwLock<HashMap<String, HashMap<String, QosLv>>>>,
               pkt_id_gen: &Arc<Mutex<PktIdGen>>) -> Result<()> {
    let subscriptions = subscriptions.read().unwrap();
    let mut sessions = sessions.write().unwrap();
    let mut pkt_id_gen = pkt_id_gen.lock().unwrap();
    match subscriptions.get(topic_name) {
        Some(client_id_to_qos) => {
            for (client_id, qos_lv) in client_id_to_qos.iter() {
                if client_id == sender_id {
                    continue;
                }
                let pkt_id = if *qos_lv == QosLv::AtMostOnce {
                    None
                } else {
                    match pkt_id_gen.gen() {
                        None => return Err(Error::PublishOutOfPktIds),
                        pkt_id => pkt_id
                    }
                };
                match streams.lock().unwrap().get(client_id) {
                    Some(mut stream) => {
                        stream.write_all(&(Publish {
                            dup: false,
                            qos_lv: *qos_lv,
                            retain: false,
                            topic_name: topic_name.to_string(),
                            pkt_id,
                            payload: payload.clone()
                        }.serialize()?))?;
                        match sessions.get_mut(client_id) {
                            Some(session) => {
                                if pkt_id.is_some() {
                                    session.waiting_for_ack.push_back((pkt_id.unwrap(),
                                        Message { qos_lv: *qos_lv, payload: payload.clone() }));
                                }
                            }
                            None => ()
                        }
                    }
                    None => ()
                }
            }
            Ok(())
        }
        None => Ok(())
    }
}

fn check_for_session(client_id: &Option<String>,
                     sessions: &Arc<RwLock<HashMap<String, Session>>>) -> Result<()> {
    match client_id {
        &Some(ref client_id) =>
            if sessions.read().unwrap().contains_key(client_id) {
                Ok(())
            } else {
                Err(Error::NoSession)
            }
        &None => Err(Error::NoSession)
    }
}

// subscriptions: topic -> client id -> QoS
fn handle_client(mut stream: TcpStream,
                 streams: Arc<Mutex<HashMap<String, TcpStream>>>,
                 sessions: Arc<RwLock<HashMap<String, Session>>>,
                 retained_msgs: Arc<RwLock<HashMap<String, Message>>>,
                 subscriptions: Arc<RwLock<HashMap<String, HashMap<String, QosLv>>>>,
                 pkt_id_gen: Arc<Mutex<PktIdGen>>) -> Result<()> {
    let mut client_id: Option<String> = None;
    loop {
        match match CtrlPkt::deserialize(&mut stream) {
            Ok(Connect {
                connect_flags,
                keep_alive,
                client_id: cid,
                will_topic,
                will_message,
                username,
                password
            }) => {
                println!("Received {:?}", Connect {
                    connect_flags,
                    keep_alive,
                    client_id: cid.clone(),
                    will_topic: will_topic.clone(),
                    will_message: will_message.clone(),
                    username: username.clone(),
                    password: password.clone()
                });
                client_id = Some(cid.clone());
                {
                    // Add stream to streams so that other threads can send to this client id
                    let mut streams = streams.lock().unwrap();
                    streams.insert(cid.clone(), stream.try_clone().unwrap());
                }
                let mut sessions = sessions.write().unwrap();
                let (session_present, return_code) =
                    if connect_flags.contains(ConnectFlags::CLEAN_SESSION) ||
                        !sessions.contains_key(&cid) {
                            (false, ConnAckRetCode::Accepted)
                        } else {
                            (true, ConnAckRetCode::Accepted)
                        };
                if connect_flags.contains(ConnectFlags::CLEAN_SESSION) {
                    // Clear old session and create new one
                    sessions.remove(&cid);
                    sessions.insert(cid.clone(), Session::new(cid,
                        connect_flags.contains(ConnectFlags::CLEAN_SESSION)));
                } else {
                    // Get old session or create a new one
                    let old_session_exists = sessions.get(&cid).is_some();
                    if !old_session_exists {
                        sessions.insert(cid.clone(), Session::new(cid,
                            connect_flags.contains(ConnectFlags::CLEAN_SESSION)));
                    }
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
                check_for_session(&client_id, &sessions)?;
                if retain {
                    let mut retained_msgs = retained_msgs.write().unwrap();
                    retained_msgs.insert(topic_name.clone(),
                        Message { qos_lv, payload: payload.clone() });
                }

                publish_msg(client_id.as_ref().unwrap(), &topic_name, &payload, &streams, &sessions, &subscriptions, &pkt_id_gen)?;

                match qos_lv {
                    QosLv::AtMostOnce => Ok(()),
                    QosLv::AtLeastOnce => stream.write_all(&(PubAck(pkt_id.unwrap())
                        .serialize()?)),
                    QosLv::ExactlyOnce => stream.write_all(&(PubRec(pkt_id.unwrap())
                        .serialize()?))
                }
            }
            Ok(PubAck(pkt_id)) => {
                println!("Received {:?}", PubAck(pkt_id));
                check_for_session(&client_id, &sessions)?;
                let mut sessions = sessions.write().unwrap();
                let mut session = sessions.get_mut(client_id.as_ref().unwrap()).unwrap();
                let mut pkt_id_gen = pkt_id_gen.lock().unwrap();
                pkt_id_gen.rm(pkt_id);
                let mut idx: Option<usize> = None;
                for (i, &(pi, _)) in session.waiting_for_ack.iter().enumerate() {
                    if pkt_id == pi {
                        idx = Some(i);
                    }
                }
                match idx {
                    Some(idx) => {
                        session.waiting_for_ack.remove(idx);
                    }
                    None => ()
                }
                Ok(())
            }
            Ok(Subscribe { pkt_id, subs }) => {
                println!("Received {:?}", Subscribe {
                    pkt_id,
                    subs: subs.clone()
                });
                check_for_session(&client_id, &sessions)?;
                let mut sessions = sessions.write().unwrap();
                let session = sessions.get_mut(client_id.as_ref().unwrap()).unwrap();
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
                check_for_session(&client_id, &sessions)?;
                stream.write_all(&(PingResp.serialize()?))
            }
            Ok(pkt@Disconnect) => {
                println!("Received {:?}", pkt);
                check_for_session(&client_id, &sessions)?;
                return Ok(());
            }
            Ok(pkt@_) => {
                println!("Received {:?}", pkt);
                check_for_session(&client_id, &sessions)?;
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
    let streams: Arc<Mutex<HashMap<String, TcpStream>>> = Arc::new(Mutex::new(HashMap::new()));
    let th = thread::spawn(move || {
        for stream in listener.incoming() {
            let sessions = Arc::clone(&sessions);
            let retained_msgs = Arc::clone(&retained_msgs);
            let pkt_id_gen = Arc::clone(&pkt_id_gen);
            let subscriptions = Arc::clone(&subscriptions);
            let streams = Arc::clone(&streams);
            match stream {
                Ok(stream) => {
                    // Make read calls block
                    let _ = stream.set_read_timeout(None).unwrap();
                    thread::spawn(move || {
                        match handle_client(stream, streams, sessions, retained_msgs, subscriptions,
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
    let t1 = thread::spawn(move || {
        let netopt = NetworkOptions::new();
        let mut opts = ClientOptions::new();
        opts.set_username("username".to_string())
            .set_password("password".to_string())
            .set_client_id("client1".to_string())
            .set_keep_alive(30);
        let mut client = opts.connect("127.0.0.1:1883", netopt).expect("Can't connect to server");
        client.subscribe("test_topic").unwrap();
        println!("{:?}", client.await().unwrap());
        client.publish("test_topic".to_string(), "hello from client 1!", PubOpt::at_least_once());
        client.publish("test_topic".to_string(), "hello again from client 1!", PubOpt::at_least_once());
        loop {
            match client.await().unwrap() {
                Some(message) => {
                    println!("client 1: {:?}", message);
                },
                None => {
                    println!(".");
                }
            }
        }
    });

    let t2 = thread::spawn(move || {
        let netopt = NetworkOptions::new();
        let mut opts = ClientOptions::new();
        opts.set_username("username".to_string())
            .set_password("password".to_string())
            .set_client_id("client2".to_string())
            .set_keep_alive(30);
        let mut client = opts.connect("127.0.0.1:1883", netopt).expect("Can't connect to server");
        client.subscribe("test_topic").unwrap();
        println!("{:?}", client.await().unwrap());
        client.publish("test_topic".to_string(), "hello from client 2!", PubOpt::at_least_once());
        loop {
            match client.await().unwrap() {
                Some(message) => {
                    println!("client 2: {:?}", message);
                },
                None => {
                    println!(".");
                }
            }
        }
    });

    let _ = th.join();
    t1.join();
    t2.join();
}
