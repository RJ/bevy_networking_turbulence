use bevy::{
    log,
    app::{AppBuilder, Events, Plugin},
    ecs::prelude::*,
    tasks::{IoTaskPool, TaskPool},
};

#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::Task;

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::{unbounded, Receiver, Sender, SendError as CrossbeamSendError};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::RwLock;

use std::{
    collections::HashMap,
    error::Error,
    fmt::Debug,
    net::SocketAddr,
    sync::{atomic, Arc, Mutex},
};

use naia_client_socket::ClientSocket;

#[cfg(not(target_arch = "wasm32"))]
use naia_server_socket::{MessageSender as ServerSender, ServerSocket};

pub use naia_client_socket::LinkConditionerConfig;

#[cfg(not(target_arch = "wasm32"))]
pub use naia_server_socket::find_my_ip_address;

mod packet_stats;
pub use packet_stats::*;

mod transport;
pub use transport::{Connection, Packet};

pub type ConnectionHandle = u32;


#[derive(Default)]
pub struct NetworkingPlugin {
    pub link_conditioner: Option<LinkConditionerConfig>,
}

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut AppBuilder) {
        let task_pool = app
            .world()
            .get_resource::<IoTaskPool>()
            .expect("`IoTaskPool` resource not found.")
            .0
            .clone();

        app.insert_resource(NetworkResource::new(
            task_pool,
            self.link_conditioner.clone(),
        ))
        .add_event::<NetworkEvent>()
        .add_system(receive_packets.system());
    }
}

pub struct NetworkResource {
    task_pool: TaskPool,

    pending_connections: Arc<Mutex<Vec<Box<dyn Connection>>>>,
    connection_sequence: atomic::AtomicU32,
    pub connections: HashMap<ConnectionHandle, Box<dyn Connection>>,

    #[cfg(not(target_arch = "wasm32"))]
    listeners: Vec<ServerListener>,
    #[cfg(not(target_arch = "wasm32"))]
    server_channels: Arc<RwLock<HashMap<SocketAddr, Sender<Result<Packet, NetworkError>>>>>,

    link_conditioner: Option<LinkConditionerConfig>,
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // FIXME: remove this struct?
struct ServerListener {
    receiver_task: Task<()>,
    // needed to keep receiver_task alive
    sender: ServerSender,
    socket_address: SocketAddr,
}

#[derive(Debug)]
pub enum NetworkEvent {
    Connected(ConnectionHandle),
    Disconnected(ConnectionHandle),
    Packet(ConnectionHandle, Packet),
    Error(ConnectionHandle, NetworkError),
}

#[derive(Debug)]
pub enum NetworkError {
    IoError(Box<dyn Error + Sync + Send>),
    /// if we haven't seen a packet for the specified timeout
    MissedHeartbeat,
    Disconnected,
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for NetworkResource {}

#[cfg(target_arch = "wasm32")]
unsafe impl Sync for NetworkResource {}

impl NetworkResource {
    pub fn new( task_pool: TaskPool,
                link_conditioner: Option<LinkConditionerConfig>,
            ) -> Self
    {
        NetworkResource {
            task_pool,
            connections: HashMap::new(),
            connection_sequence: atomic::AtomicU32::new(0),
            pending_connections: Arc::new(Mutex::new(Vec::new())),
            #[cfg(not(target_arch = "wasm32"))]
            listeners: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            server_channels: Arc::new(RwLock::new(HashMap::new())),
            link_conditioner,
        }
    }

    /// The 3 listening addresses aren't strictly necessary, you can put the same IP address with a different port for the socket address; Unless you have some configuration issues with public and private addresses that need to be connected to.
    /// They also aren't necessary if you're using UDP, so you can put anything if that's the case.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn listen(
        &mut self,
        socket_address: SocketAddr,
        webrtc_listen_address: Option<SocketAddr>,
        public_webrtc_address: Option<SocketAddr>,
    ) {
        let mut server_socket = {
            let webrtc_listen_address = webrtc_listen_address.unwrap_or_else(|| {
                let mut listen_addr = socket_address;
                listen_addr.set_port(socket_address.port() + 1);
                listen_addr
            });
            let public_webrtc_address = public_webrtc_address.unwrap_or(webrtc_listen_address);
            let socket = futures_lite::future::block_on(ServerSocket::listen(
                socket_address,
                webrtc_listen_address,
                public_webrtc_address,
            ));

            if let Some(ref conditioner) = self.link_conditioner {
                socket.with_link_conditioner(conditioner)
            } else {
                socket
            }
        };
        let sender = server_socket.get_sender();
        let server_channels = self.server_channels.clone();
        let pending_connections = self.pending_connections.clone();

        let receiver_task = self.task_pool.spawn(async move {
            loop {
                match server_socket.receive().await {
                    Ok(packet) => {
                        let address = packet.address();
                        let message = String::from_utf8_lossy(packet.payload());
                        log::debug!(
                            "Server recv <- {}:{}: {}",
                            address,
                            packet.payload().len(),
                            message
                        );

                        let needs_new_channel = match server_channels
                            .read()
                            .expect("server channels lock is poisoned")
                            .get(&address)
                            .map(|channel| {
                                channel.send(Ok(Packet::copy_from_slice(packet.payload())))
                            }) {
                            Some(Ok(())) => false,
                            Some(Err(CrossbeamSendError(_packet))) => {
                                log::error!("Server can't send to channel, recreating");
                                // If we can't send to a channel, it's disconnected.
                                // We need to re-create the channel and re-try sending the message.
                                true
                            }
                            // This is a new connection, so we need to create a channel.
                            None => true,
                        };

                        if !needs_new_channel {
                            continue;
                        }

                        // We try to do a write lock only in case when a channel doesn't exist or
                        // has to be re-created. Trying to acquire a channel even for new
                        // connections is kind of a positive prediction to avoid doing a write
                        // lock.
                        let mut server_channels = server_channels
                            .write()
                            .expect("server channels lock is poisoned");
                        let (packet_tx, packet_rx): (
                            Sender<Result<Packet, NetworkError>>,
                            Receiver<Result<Packet, NetworkError>>,
                        ) = unbounded();
                        match packet_tx.send(Ok(Packet::copy_from_slice(packet.payload()))) {
                            Ok(()) => {
                                // It makes sense to store the channel only if it's healthy.
                                pending_connections.lock().unwrap().push(Box::new(
                                    transport::ServerConnection::new(
                                        packet_rx,
                                        server_socket.get_sender(),
                                        address,
                                    ),
                                ));
                                server_channels.insert(address, packet_tx);
                            }
                            Err(error) => {
                                // This branch is unlikely to get called the second time (after
                                // re-creating a channel), but if for some strange reason it does,
                                // we'll just lose the message this time.
                                log::error!("Server Send Error (retry): {}", error);
                            }
                        }
                    }
                    Err(error) => {
                        log::error!("Server Receive Error: {}", error);
                    }
                }
            }
        });

        self.listeners.push(ServerListener {
            receiver_task,
            sender,
            socket_address,
        });
    }

    pub fn connect(&mut self, socket_address: SocketAddr) {
        let mut client_socket = {
            let socket = ClientSocket::connect(socket_address);

            if let Some(ref conditioner) = self.link_conditioner {
                socket.with_link_conditioner(conditioner)
            } else {
                socket
            }
        };
        let sender = client_socket.get_sender();

        self.pending_connections
            .lock()
            .unwrap()
            .push(Box::new(transport::ClientConnection::new(
                client_socket,
                sender,
            )));
    }

    // removes handle and connection, but doesn't signal peer in any way.
    pub fn disconnect(&mut self, handle: ConnectionHandle) {
        // on wasm32 we can't be a webrtc server, so cleanup is simpler
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                self.connections.remove(&handle);
            } else {
                if let Some(removed_connection) = self.connections.remove(&handle) {
                    if let Some(client_addr) = removed_connection.remote_address() {
                        self.server_channels.write().expect("server connections lock poisoned").remove(&client_addr);
                    }
                }
            }
        }
    }

    pub fn send(
        &mut self,
        handle: ConnectionHandle,
        payload: Packet,
    ) -> Result<(), Box<dyn Error + Sync + Send + 'static>> {
        match self.connections.get_mut(&handle) {
            Some(connection) => connection.send(payload),
            None => Err(Box::new(std::io::Error::new(
                // FIXME: move to enum Error
                std::io::ErrorKind::NotFound,
                "No such connection",
            ))),
        }
    }

    pub fn broadcast(&mut self, payload: Packet) {
        for (_handle, connection) in self.connections.iter_mut() {
            connection.send(payload.clone()).unwrap();
        }
    }
}

pub fn receive_packets(
    mut net: ResMut<NetworkResource>,
    mut network_events: ResMut<Events<NetworkEvent>>,
) {
    let pending_connections: Vec<Box<dyn Connection>> =
        net.pending_connections.lock().unwrap().drain(..).collect();
    for conn in pending_connections {
        let handle: ConnectionHandle = net
            .connection_sequence
            .fetch_add(1, atomic::Ordering::Relaxed);
        net.connections.insert(handle, conn);
        network_events.send(NetworkEvent::Connected(handle));
    }

    for (handle, connection) in net.connections.iter_mut() {
        while let Some(result) = connection.receive() {
            match result {
                Ok(packet) => {
                    // heartbeat packets are empty
                    if packet.len() == 0 {
                        log::debug!("Received heartbeat packet");
                        // discard without sending a NetworkEvent
                        continue;
                    }
                    let message = String::from_utf8_lossy(&packet);
                    log::debug!("Received on [{}] {} RAW: {}", handle, packet.len(), message);                    
                    network_events.send(NetworkEvent::Packet(*handle, packet));
                }
                Err(err) => {
                    log::error!("Receive Error: {:?}", err);
                    network_events.send(NetworkEvent::Error(*handle, err));
                }
            }
        }
    }
}
