#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::Task;

use bytes::Bytes;
use std::{error::Error, net::SocketAddr, sync::{Arc, RwLock}};

use naia_client_socket::{
    ClientSocketTrait, MessageSender as ClientSender, Packet as ClientPacket,
};

#[cfg(not(target_arch = "wasm32"))]
use naia_server_socket::{MessageSender as ServerSender, Packet as ServerPacket};

#[cfg(not(target_arch = "wasm32"))]
use futures_lite::future::block_on;

pub use super::packet_stats::*;

use super::NetworkError;

pub type Packet = Bytes;


pub trait Connection: Send + Sync {
    fn remote_address(&self) -> Option<SocketAddr>;

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Sync + Send>>;

    fn receive(&mut self) -> Option<Result<Packet, NetworkError>>;

    fn stats(&self) -> PacketStats;

    /// returns milliseconds since last (rx, tx)
    fn last_packet_timings(&self) -> (u128, u128);
}

#[cfg(not(target_arch = "wasm32"))]
pub struct ServerConnection {
    packet_rx: crossbeam_channel::Receiver<Result<Packet, NetworkError>>,
    sender: Option<ServerSender>,
    client_address: SocketAddr,
    stats: Arc<RwLock<PacketStats>>,

    // channels_task is used to keep spawned Tasks in-scope so they are not dropped.
    // we set it, but never read from it, which the compiler warns about.
    // so we use allow(dead_code), even though it's needed for reference keeping.
    #[allow(dead_code)]
    #[cfg(not(target_arch = "wasm32"))]
    channels_task: Option<Task<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ServerConnection {
    pub fn new(
        packet_rx: crossbeam_channel::Receiver<Result<Packet, NetworkError>>,
        sender: ServerSender,
        client_address: SocketAddr,
    ) -> Self {
        ServerConnection {
            packet_rx,
            sender: Some(sender),
            client_address,
            stats: Arc::new(RwLock::new(PacketStats::default())),
            channels_task: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Connection for ServerConnection {
    fn remote_address(&self) -> Option<SocketAddr> {
        Some(self.client_address)
    }

    fn stats(&self) -> PacketStats {
        self.stats.read().expect("stats lock poisoned").clone()
    }

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Sync + Send>> {
        self.stats.write().expect("stats lock poisoned").add_tx(payload.len());
        block_on(
            self.sender
                .as_mut()
                .unwrap()
                .send(ServerPacket::new(self.client_address, payload.to_vec())),
        )
    }

    fn last_packet_timings(&self) -> (u128, u128) {
        let (rx_dur, tx_dur) = self.stats.read().expect("stats lock poisoned").idle_durations();
        (rx_dur.as_millis(), tx_dur.as_millis())
    }

    fn receive(&mut self) -> Option<Result<Packet, NetworkError>> {
        match self.packet_rx.try_recv() {
            Ok(payload) => match payload {
                Ok(packet) => {
                    self.stats.write().expect("stats lock poisoned").add_rx(packet.len());
                    Some(Ok(packet))
                },
                Err(err) => Some(Err(err))
            },
            Err(error) => match error {
                crossbeam_channel::TryRecvError::Empty => None,
                crossbeam_channel::TryRecvError::Disconnected => {
                    Some(Err(NetworkError::Disconnected))
                }
            },
        }
    }
}

pub struct ClientConnection {
    socket: Box<dyn ClientSocketTrait>,
    sender: Option<ClientSender>,
    stats: Arc<RwLock<PacketStats>>,

    // channels_task is used to keep spawned Tasks in-scope so they are not dropped.
    // we set it, but never read from it, which the compiler warns about.
    // so we use allow(dead_code), even though it's needed for reference keeping.
    #[allow(dead_code)]
    #[cfg(not(target_arch = "wasm32"))]
    channels_task: Option<Task<()>>,
}

impl ClientConnection {
    pub fn new(
        socket: Box<dyn ClientSocketTrait>,
        sender: ClientSender,
    ) -> Self {
        ClientConnection {
            socket,
            sender: Some(sender),
            stats: Arc::new(RwLock::new(PacketStats::default())),
            #[cfg(not(target_arch = "wasm32"))]
            channels_task: None,
        }
    }
}

impl Connection for ClientConnection {
    fn remote_address(&self) -> Option<SocketAddr> {
        None
    }

    fn stats(&self) -> PacketStats {
        self.stats.read().expect("stats lock poisoned").clone()
    }

    fn last_packet_timings(&self) -> (u128, u128) {
        let (rx_dur, tx_dur) = self.stats.read().expect("stats lock poisoned").idle_durations();
        (rx_dur.as_millis(), tx_dur.as_millis())
    }

    fn send(&mut self, payload: Packet) -> Result<(), Box<dyn Error + Sync + Send>> {
        self.stats.write().expect("stats lock poisoned").add_tx(payload.len());
        self.sender
            .as_mut()
            .unwrap()
            .send(ClientPacket::new(payload.to_vec()))
    }

    fn receive(&mut self) -> Option<Result<Packet, NetworkError>> {
        match self.socket.receive() {
            Ok(event) => event.map(|packet| {
                self.stats.write().expect("stats lock poisoned").add_rx(packet.payload().len());
                Ok(Packet::copy_from_slice(packet.payload()))
            }),
            Err(err) => Some(Err(NetworkError::IoError(Box::new(err)))),
        }
    }
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for ClientConnection {}

#[cfg(target_arch = "wasm32")]
unsafe impl Sync for ClientConnection {}
