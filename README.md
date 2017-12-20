# mqtt-broker

mqtt-broker is a work-in-process MQTT broker. It aims to implement the MQTT
protocol version 3.1.1.

mqtt-broker is written in Rust, a systems programming language that emphasizes
memory and concurrency safety. In order to build the project, you will need a
Rust compiler. The easiest way to get started is to install
[`rustup`](https://www.rustup.rs/), a tool that will install the necessary build
tools. Once `rustup` is installed, install the nightly compiler:

```bash
$ rustup install nightly
info: syncing channel updates for 'nightly'
info: downloading toolchain manifest
info: downloading component 'rustc'
info: downloading component 'rust-std'
info: downloading component 'rust-docs'
info: downloading component 'cargo'
info: installing component 'rustc'
info: installing component 'rust-std'
info: installing component 'rust-docs'
info: installing component 'cargo'

  nightly installed: rustc 1.9.0-nightly (02310fd31 2016-03-19)
```

You should now be able to build the project by going to the project root and
running `cargo build`. Run the project with `cargo run`.

Right now, the `main()` method listens to port 1883 for client connections. I
have set up two clients using [`mqttc`](https://github.com/inre/rust-mq), a Rust
MQTT client library. The two clients connect to the broker and subscribe to the
topic `test-topic`, and then each publishes a message with QoS 1. The broker
then publishes each client's message to the topic, and both clients receive the
other's message.

## Work done
- All MQTT broker code was written from scratch. There are no dependencies other
  than the Rust standard library, crates (Rust packages) for bitflag processing,
  UUID generation, and random number generation, and `mqttc` for testing. I
  wrote code to read from the TCP socket, deserialize packets from clients,
  and serialize and send packets back to clients.
- CONNECT, CONNACK, PUBLISH, PUBACK, SUBSCRIBE, SUBACK, PINGREQ, and PINGRESP
  packets are handled.
- Some session logic is implemented.
- QoS 0 and 1 messages are received and published.

## Work to be done
- Handle QoS 2 messages (and PUBREC, PUBREL, and PUBCOMP packets)
- Handle UNSUBSCRIBE and UNSUBACK
- Handle DISCONNECT and cleaning up sessions after clients disconnect
- Client authentication
- And lots more... the specification is quite broad.
