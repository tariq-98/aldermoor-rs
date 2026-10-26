//! Players over the network, on QUIC (`quinn`). Every message is postcard, the same as before
//! QUIC (#86); only the transport changed.
//!
//! A client opens one reliable two-way stream and says [`ClientMsg::Hello`]; the server answers
//! [`ServerMsg::Welcome`] (or `Refused`) on it, and the client's commands and acks follow on the
//! same stream, each a 4-byte little-endian length and then the message. Views go the other
//! way unreliably: a [`ServerMsg::View`] every [`SEND_EVERY`](crate::SEND_EVERY) steps as one
//! datagram, or on a one-way stream of its own when it's too big for a datagram (the first
//! view, a crowd coming into sight). Each view is a delta against the last one the client
//! acknowledged with [`ClientMsg::Ack`] (`protocol::wire`, #34), so a lost or late view costs
//! nothing but its own update: the next one is still against a base the client has.
//!
//! **Certificates.** A server has an [`Identity`]: a self-signed certificate for the name
//! `aldermoor` and its private key, made once and kept in files so it survives restarts.
//! Clients trust exactly that certificate (the `.der` file, which is public; the key stays on
//! the server), with ordinary TLS checks. Certificates from a real authority wait for accounts
//! and a host name (#88).

use crate::{Server, Viewer};
use protocol::wire::{Decoder, WireError, WireView};
use protocol::{CombatView, Command, Snapshot, UnitId};
pub use rustls::pki_types::CertificateDer;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::runtime::Runtime;

/// Views a connection may fall behind by (two seconds) before the server drops it.
pub const BACKLOG: usize = 120;

/// The largest message either side accepts.
pub const MAX_MESSAGE: usize = 16 << 20;

/// The name the server's certificate is made out to, and clients ask for.
pub const SERVER_NAME: &str = "aldermoor";

/// How long a silent connection lasts. Both sides ping well inside it.
const IDLE: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// The first message: who's playing, and which `protocol::VERSION` they speak.
    Hello {
        version: u32,
        name: String,
    },
    Command(Command),
    /// The client has decoded view `tick`, so later views may be deltas against it.
    Ack(u64),
}

// Views are nearly every message, so boxing them would only add an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome {
        version: u32,
        player: UnitId,
    },
    Refused(String),
    /// What the player sees now, with everything that happened since the last view.
    View(WireView),
}

/// A message as it goes on a stream: length, then postcard. A view frame from the server
/// loop is this too; the length is dropped when it goes as a datagram.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut out = vec![0; 4];
    postcard::to_io(msg, &mut out).expect("messages always serialise");
    let len = (out.len() - 4) as u32;
    out[..4].copy_from_slice(&len.to_le_bytes());
    out
}

/// Reads one length-prefixed message from a stream; `None` when the stream ended cleanly.
async fn read<T: for<'de> Deserialize<'de>>(r: &mut quinn::RecvStream) -> io::Result<Option<T>> {
    let mut len = [0; 4];
    match r.read_exact(&mut len).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(e) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, e)),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a {len}-byte message is too big")));
    }
    let mut buf = vec![0; len];
    r.read_exact(&mut buf).await.map_err(|e| io::Error::new(io::ErrorKind::UnexpectedEof, e))?;
    postcard::from_bytes(&buf).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

async fn write<T: Serialize>(w: &mut quinn::SendStream, msg: &T) -> io::Result<()> {
    w.write_all(&encode(msg)).await.map_err(io::Error::other)
}

/// One runtime for every connection in the process: the network threads (SERVER.md).
fn runtime() -> io::Result<Arc<Runtime>> {
    static RUNTIME: std::sync::OnceLock<Arc<Runtime>> = std::sync::OnceLock::new();
    if let Some(rt) = RUNTIME.get() {
        return Ok(rt.clone());
    }
    let rt = Arc::new(tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("Aldermoor network").enable_all().build()?);
    Ok(RUNTIME.get_or_init(|| rt).clone())
}

fn transport() -> quinn::TransportConfig {
    let mut t = quinn::TransportConfig::default();
    t.max_idle_timeout(Some(IDLE.try_into().expect("ten seconds is a valid timeout")));
    t.keep_alive_interval(Some(Duration::from_secs(2)));
    // Room for a second of views when the reader falls behind.
    t.datagram_receive_buffer_size(Some(1 << 20));
    t
}

/// A server's certificate and private key.
pub struct Identity {
    pub certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
}

impl Identity {
    /// A new self-signed certificate for [`SERVER_NAME`].
    pub fn generate() -> io::Result<Identity> {
        let made = rcgen::generate_simple_self_signed(vec![SERVER_NAME.into()]).map_err(io::Error::other)?;
        Ok(Identity { certificate: made.cert.der().clone(), key: PrivatePkcs8KeyDer::from(made.key_pair.serialize_der()) })
    }

    /// The identity kept at `cert` (the certificate, to hand to players) and `cert` with a
    /// `.key` extension (the private key, which stays here), made and saved the first time.
    pub fn load_or_create(cert: &Path) -> io::Result<Identity> {
        let key_path = cert.with_extension("key");
        if cert.exists() && key_path.exists() {
            return Ok(Identity {
                certificate: CertificateDer::from(std::fs::read(cert)?),
                key: PrivatePkcs8KeyDer::from(std::fs::read(&key_path)?),
            });
        }
        let identity = Identity::generate()?;
        if let Some(dir) = cert.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        write_private(&key_path, identity.key.secret_pkcs8_der())?;
        std::fs::write(cert, identity.certificate.as_ref())?;
        Ok(identity)
    }
}

/// Writes a file only its owner can read.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    io::Write::write_all(&mut options.open(path)?, bytes)
}

/// Accepting connections into a world.
pub struct Listener {
    addr: SocketAddr,
    certificate: CertificateDer<'static>,
    endpoint: quinn::Endpoint,
}

impl Listener {
    /// Where it listens (with the real port when bound to port 0).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The certificate clients must trust to connect.
    pub fn certificate(&self) -> &CertificateDer<'static> {
        &self.certificate
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.endpoint.close(0u32.into(), b"server closed");
    }
}

/// Starts accepting players into `server` on `addr` (UDP) with a new certificate, for tests
/// and in-process listeners; see [`Listener::certificate`].
pub fn listen(server: &Server, addr: impl ToSocketAddrs) -> io::Result<Listener> {
    listen_as(server, addr, Identity::generate()?)
}

/// Starts accepting players into `server` on `addr` (UDP) as `identity`, on the network
/// threads. Once the server stops, connections are turned away.
pub fn listen_as(server: &Server, addr: impl ToSocketAddrs, identity: Identity) -> io::Result<Listener> {
    let addr = addr.to_socket_addrs()?.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no address to listen on"))?;
    let certificate = identity.certificate.clone();
    let mut config =
        quinn::ServerConfig::with_single_cert(vec![identity.certificate], PrivateKeyDer::Pkcs8(identity.key)).map_err(io::Error::other)?;
    config.transport_config(Arc::new(transport()));
    let rt = runtime()?;
    let endpoint = {
        let _inside = rt.enter();
        quinn::Endpoint::server(config, addr)?
    };
    let addr = endpoint.local_addr()?;
    let (control, commands) = (server.control.clone(), server.commands.clone());
    let accepting = endpoint.clone();
    rt.spawn(async move {
        while let Some(incoming) = accepting.accept().await {
            let (control, commands) = (control.clone(), commands.clone());
            tokio::spawn(async move {
                let result = async { serve(incoming.await?, control, commands).await }.await;
                if let Err(e) = result {
                    eprintln!("connection closed: {e}");
                }
            });
        }
    });
    Ok(Listener { addr, certificate, endpoint })
}

async fn serve(
    conn: quinn::Connection,
    control: crossbeam_channel::Sender<crate::Control>,
    commands: crossbeam_channel::Sender<(UnitId, Command)>,
) -> io::Result<()> {
    let peer = conn.remote_address();
    // A connection that doesn't say hello promptly isn't a client.
    let hello = tokio::time::timeout(Duration::from_secs(5), async {
        let (send, mut recv) = conn.accept_bi().await.map_err(io::Error::other)?;
        let hello = read::<ClientMsg>(&mut recv).await?;
        Ok::<_, io::Error>((send, recv, hello))
    });
    let (mut send, mut recv, hello) = hello.await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no hello"))??;
    let name = match hello {
        Some(ClientMsg::Hello { version, name }) if version == protocol::VERSION => name,
        Some(ClientMsg::Hello { version, .. }) => {
            let why = format!("this server speaks protocol {}, not {version}", protocol::VERSION);
            write(&mut send, &ServerMsg::Refused(why.clone())).await?;
            let _ = send.finish();
            let _ = send.stopped().await;
            return Err(io::Error::other(why));
        }
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "no hello first")),
    };
    let (frames, outbox) = crossbeam_channel::bounded::<Vec<u8>>(BACKLOG);
    let (reply, answer) = crossbeam_channel::bounded(1);
    let acked = Arc::new(AtomicU64::new(0));
    let remote = crate::Remote { frames, acked: acked.clone(), encoder: protocol::wire::Encoder::new(), phase: 0 };
    control
        .send(crate::Control::Join { name: name.clone(), viewer: Viewer::Remote(remote), reply })
        .map_err(|_| io::Error::other("the server has stopped"))?;
    let player = tokio::task::spawn_blocking(move || answer.recv())
        .await
        .map_err(io::Error::other)?
        .map_err(|_| io::Error::other("the server has stopped"))?;
    eprintln!("{name} joined from {peer} as unit {player}");
    write(&mut send, &ServerMsg::Welcome { version: protocol::VERSION, player }).await?;
    // The sender hands views to QUIC until the connection or the server goes; the server drops
    // a connection that falls BACKLOG views behind by dropping its channel.
    let sending = conn.clone();
    let handle = tokio::runtime::Handle::current();
    std::thread::Builder::new().name("Aldermoor sender".into()).spawn(move || {
        while let Ok(frame) = outbox.recv() {
            if send_view(&sending, &handle, frame).is_err() {
                break;
            }
        }
        sending.close(0u32.into(), b"left");
    })?;
    let result = loop {
        match read::<ClientMsg>(&mut recv).await {
            Ok(Some(ClientMsg::Command(c))) => {
                if commands.send((player, c)).is_err() {
                    break Ok(());
                }
            }
            Ok(Some(ClientMsg::Ack(tick))) => {
                acked.fetch_max(tick.wrapping_add(1), Ordering::AcqRel);
            }
            Ok(Some(ClientMsg::Hello { .. })) => {
                break Err(io::Error::new(io::ErrorKind::InvalidData, "a second hello"));
            }
            Ok(None) => break Ok(()),
            Err(_) if conn.close_reason().is_some() => break Ok(()),
            Err(e) => break Err(e),
        }
    };
    // Closing the connection ends the sender, which drops the channel, and the server lets go.
    conn.close(0u32.into(), b"bye");
    eprintln!("{name} left");
    result
}

/// Sends one view frame (length and postcard, as `encode` made it): as a datagram when it
/// fits, else on a one-way stream of its own.
fn send_view(conn: &quinn::Connection, handle: &tokio::runtime::Handle, frame: Vec<u8>) -> Result<(), quinn::ConnectionError> {
    if let Some(reason) = conn.close_reason() {
        return Err(reason);
    }
    let body = bytes::Bytes::from(frame).slice(4..);
    if conn.max_datagram_size().is_some_and(|max| body.len() <= max) {
        match conn.send_datagram(body.clone()) {
            Ok(()) => return Ok(()),
            Err(quinn::SendDatagramError::ConnectionLost(e)) => return Err(e),
            // Too big after all (the path shrank), or datagrams off: use a stream.
            Err(_) => {}
        }
    }
    let conn = conn.clone();
    handle.spawn(async move {
        if let Ok(mut s) = conn.open_uni().await
            && s.write_all(&body).await.is_ok()
        {
            let _ = s.finish();
        }
    });
    Ok(())
}

/// Trusts exactly `certificate` (a server's [`Identity`]) for [`SERVER_NAME`].
fn client_config(certificate: &CertificateDer<'_>) -> io::Result<quinn::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate.clone().into_owned()).map_err(io::Error::other)?;
    let mut config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).map_err(io::Error::other)?;
    config.transport_config(Arc::new(transport()));
    Ok(config)
}

/// A player connected to a server: what bots, load tests and the client's `--connect` use.
pub struct Client {
    rt: Arc<Runtime>,
    conn: quinn::Connection,
    /// Commands and acks, in order, for the task that owns the reliable stream.
    out: tokio::sync::mpsc::UnboundedSender<ClientMsg>,
    /// View messages as they arrive, by datagram or stream, with their size on the wire.
    views: crossbeam_channel::Receiver<(Vec<u8>, usize)>,
    decoder: Decoder,
    /// The newest view decoded; older ones that arrive late are skipped.
    newest: Option<u64>,
    last_size: usize,
    received: Arc<AtomicU64>,
    pub player: UnitId,
}

/// What `Client` hands to other threads for sending commands.
#[derive(Clone)]
pub struct Commands(tokio::sync::mpsc::UnboundedSender<ClientMsg>);

impl Commands {
    pub fn send(&self, c: Command) -> io::Result<()> {
        self.0.send(ClientMsg::Command(c)).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "disconnected"))
    }
}

impl Client {
    /// Connects to the server whose certificate is `certificate` and says hello. Fails if the
    /// certificate doesn't match or the server refuses (another protocol version).
    pub fn connect(addr: impl ToSocketAddrs, certificate: &CertificateDer<'_>, name: &str) -> io::Result<Client> {
        Client::hello(addr, certificate, name, protocol::VERSION)
    }

    /// Connects saying it speaks protocol `version`, for testing refusals.
    #[doc(hidden)]
    pub fn hello(addr: impl ToSocketAddrs, certificate: &CertificateDer<'_>, name: &str, version: u32) -> io::Result<Client> {
        let addr = addr.to_socket_addrs()?.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no address to connect to"))?;
        let rt = runtime()?;
        let local: SocketAddr = if addr.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" }.parse().expect("a valid wildcard address");
        let mut endpoint = {
            let _inside = rt.enter();
            quinn::Endpoint::client(local)?
        };
        endpoint.set_default_client_config(client_config(certificate)?);
        let name = name.to_string();
        let (conn, send, mut recv, player) = rt.block_on(async {
            let conn = endpoint
                .connect(addr, SERVER_NAME)
                .map_err(io::Error::other)?
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;
            let (mut send, mut recv) = conn.open_bi().await.map_err(io::Error::other)?;
            write(&mut send, &ClientMsg::Hello { version, name }).await?;
            match read::<ServerMsg>(&mut recv).await? {
                Some(ServerMsg::Welcome { player, .. }) => Ok((conn, send, recv, player)),
                Some(ServerMsg::Refused(why)) => Err(io::Error::new(io::ErrorKind::ConnectionRefused, why)),
                Some(ServerMsg::View(_)) => Err(io::Error::new(io::ErrorKind::InvalidData, "a view before welcome")),
                None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the server closed the connection")),
            }
        })?;
        let (out, mut outgoing) = tokio::sync::mpsc::unbounded_channel::<ClientMsg>();
        let (arrived, views) = crossbeam_channel::unbounded();
        let received = Arc::new(AtomicU64::new(0));
        rt.spawn(async move {
            let mut send = send;
            while let Some(msg) = outgoing.recv().await {
                if write(&mut send, &msg).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });
        // Views by datagram.
        let (c, a, r) = (conn.clone(), arrived.clone(), received.clone());
        rt.spawn(async move {
            while let Ok(d) = c.read_datagram().await {
                r.fetch_add(d.len() as u64, Ordering::Relaxed);
                if a.send((d.to_vec(), d.len())).is_err() {
                    break;
                }
            }
        });
        // Views too big for a datagram, each on a stream of its own.
        let (c, a, r) = (conn.clone(), arrived, received.clone());
        rt.spawn(async move {
            while let Ok(mut s) = c.accept_uni().await {
                let (a, r) = (a.clone(), r.clone());
                tokio::spawn(async move {
                    if let Ok(body) = s.read_to_end(MAX_MESSAGE).await {
                        r.fetch_add(body.len() as u64, Ordering::Relaxed);
                        let size = body.len();
                        let _ = a.send((body, size));
                    }
                });
            }
        });
        // Nothing more comes on the reliable stream after the welcome, but reading it notices
        // the server closing it.
        rt.spawn(async move { while let Ok(Some(_)) = read::<ServerMsg>(&mut recv).await {} });
        Ok(Client { rt, conn, out, views, decoder: Decoder::new(), newest: None, last_size: 0, received, player })
    }

    pub fn send(&mut self, c: Command) -> io::Result<()> {
        self.commands().send(c)
    }

    /// A handle for sending commands from another thread.
    pub fn commands(&self) -> Commands {
        Commands(self.out.clone())
    }

    /// Waits for the next view, rebuilds it whole and acknowledges it. Views that arrive after
    /// a newer one are skipped.
    pub fn next_view(&mut self) -> io::Result<(Snapshot, CombatView)> {
        loop {
            let (body, size) = match self.views.recv_timeout(IDLE) {
                Ok(v) => v,
                Err(_) => {
                    let why = self.conn.close_reason().map_or("no views for ten seconds".to_string(), |r| r.to_string());
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, why));
                }
            };
            let view = match postcard::from_bytes::<ServerMsg>(&body) {
                Ok(ServerMsg::View(view)) => view,
                Ok(_) => continue,
                Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
            };
            if self.newest.is_some_and(|n| view.tick <= n) {
                continue;
            }
            let tick = view.tick;
            let decoded = match self.decoder.decode(view) {
                Ok(d) => d,
                // Built on a view that arrived after a newer one and was skipped: the next is
                // against what this client acknowledged.
                Err(WireError::UnknownBase(_)) => continue,
                Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
            };
            self.newest = Some(tick);
            self.last_size = size;
            let _ = self.out.send(ClientMsg::Ack(tick));
            return Ok(decoded);
        }
    }

    /// The wire size of the last view `next_view` returned.
    pub fn last_size(&self) -> usize {
        self.last_size
    }

    /// Bytes of views received since connecting.
    pub fn bytes_received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }

    /// The count behind `bytes_received`, for another thread to read.
    pub fn received(&self) -> Arc<AtomicU64> {
        self.received.clone()
    }

    /// A handle that closes this connection from another thread, which ends a `next_view`
    /// waiting on it.
    pub fn closer(&self) -> Closer {
        Closer(self.conn.clone())
    }
}

/// Closes a [`Client`]'s connection from elsewhere.
pub struct Closer(quinn::Connection);

impl Closer {
    pub fn close(&self) {
        self.0.close(0u32.into(), b"bye");
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.conn.close(0u32.into(), b"bye");
        // Let the close go out, unless this is dropped on the network threads themselves.
        if tokio::runtime::Handle::try_current().is_err() {
            let conn = self.conn.clone();
            let _ = self.rt.block_on(async { tokio::time::timeout(Duration::from_millis(100), conn.closed()).await });
        }
    }
}
