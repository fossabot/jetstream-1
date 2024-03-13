use crate::async_wire_format::{AsyncWireFormat, AsyncWireFormatExt};
use anyhow::Ok;
use futures_util::AsyncReadExt;
use p9::{Rframe, Tframe, WireFormat};
use s2n_quic::client::{Client, Connect};
use s2n_quic::provider::tls;
use serde::de;
use slog_scope::{debug, error};
use std::io::Write;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use termcolor::{BufferWriter, Color, ColorChoice, ColorSpec, WriteColor};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::{log, ConvertWireFormat};

#[derive(Debug, Clone)]
pub struct DialQuic {
    host: String,
    port: u16,
    client_cert: Box<Path>,
    key: Box<Path>,
    ca_cert: Box<Path>,
    hostname: String,
}

impl DialQuic {
    pub fn new(
        host: String,
        port: u16,
        cert: Box<Path>,
        key: Box<Path>,
        ca_cert: Box<Path>,
        hostname: String,
    ) -> Self {
        Self {
            host,
            port,
            client_cert: cert,
            key,
            ca_cert,
            hostname,
        }
    }
}

impl DialQuic {
    async fn dial(self) -> anyhow::Result<s2n_quic::Connection> {
        let ca_cert = self.ca_cert.to_str().unwrap();
        let client_cert = self.client_cert.to_str().unwrap();
        let client_key = self.key.to_str().unwrap();
        let tls = tls::default::Client::builder()
            .with_certificate(Path::new(ca_cert))?
            .with_client_identity(
                Path::new(client_cert),
                Path::new(client_key),
            )?
            .build()?;

        let client = Client::builder()
            .with_tls(tls)?
            .with_io("0.0.0.0:0")?
            .start()?;

        let host_port = format!("{}:{}", self.host, self.port);

        let addr: SocketAddr = host_port.parse()?;
        let connect = Connect::new(addr).with_server_name(&*self.hostname);
        let mut connection = client.connect(connect).await?;

        // ensure the connection doesn't time out with inactivity
        connection.keep_alive(true)?;
        Ok(connection)
    }
}

pub struct Proxy {
    dial: DialQuic,
    listen: Box<Path>,
}

impl Proxy {
    pub fn new(dial: DialQuic, listen: Box<Path>) -> Self {
        Self { dial, listen }
    }
}

impl Proxy {
    pub async fn run(&self) {
        debug!("Listening on {:?}", self.listen);
        let listener = tokio::net::UnixListener::bind(&self.listen).unwrap();

        while let std::result::Result::Ok((mut down_stream, _)) =
            listener.accept().await
        {
            debug!("Accepted connection from {:?}", down_stream.peer_addr());
            async move {
                let down_stream = down_stream;
                let dial = self.dial.clone();
                debug!("Dialing {:?}", dial);
                let mut dial = dial.clone().dial().await.unwrap();
                debug!("Connected to {:?}", dial.remote_addr());
                let up_stream = dial.open_bidirectional_stream().await.unwrap();
                tokio::task::spawn(async move {
                    up_stream.connection().ping().unwrap();
                    let (rx, mut tx) = up_stream.split();
                    let (read, mut write) = down_stream.into_split();
                    let mut upstream_reader = tokio::io::BufReader::new(rx);
                    //let mut upstream_writer = tokio::io::BufWriter::new(tx);
                    // let mut downstream_writer =
                    //     tokio::io::BufWriter::new(write);
                    let mut downstream_reader = tokio::io::BufReader::new(read);
                    loop {
                        // read and send to up_stream
                        {
                            debug!("Reading from down_stream");
                            let tframe =
                                Tframe::decode_async(&mut downstream_reader)
                                    .await;
                            if let Err(e) = tframe {
                                // if error is eof, break
                                if e.kind() == std::io::ErrorKind::UnexpectedEof
                                {
                                    break;
                                } else {
                                    error!(
                                        "Error decoding from down_stream: {:?}",
                                        e
                                    );
                                    break;
                                }
                            } else if let std::io::Result::Ok(tframe) = tframe {
                                debug!("Sending to up_stream {:?}", tframe);
                                let _ =
                                    tframe.encode_async(&mut tx).await.unwrap();
                            }
                        }
                        // write and send to down_stream
                        {
                            debug!("Reading from up_stream");
                            let rframe =
                                Rframe::decode_async(&mut upstream_reader)
                                    .await
                                    .unwrap();
                            debug!("Sending to down_stream");
                            rframe.encode_async(&mut write).await.unwrap();
                        }
                    }
                });
            }
            .await;
        }
    }
}
