//! Players over the network. For now a plain TCP stream on which every message is a 4-byte
//! little-endian length and then the message in postcard; QUIC (#86) replaces the transport and
//! keeps the messages.
//!
//! A client opens with [`ClientMsg::Hello`]; the server answers [`ServerMsg::Welcome`] (or
//! `Refused`) and then sends a [`ServerMsg::View`] every [`SEND_EVERY`](crate::SEND_EVERY)
//! steps, each a delta against the last one the client acknowledged with [`ClientMsg::Ack`]
//! (`protocol::wire`, #34). Each connection has a reader thread that queues its commands and a
//! writer thread that sends its views.

use crate::{Server, Viewer};
use protocol::wire::{Decoder, WireView};
use protocol::{CombatView, Command, Snapshot, UnitId};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::thread::JoinHandle;
use std::time::Duration;

/// Views a connection may fall behind by (two seconds) before the server drops it.
pub const BACKLOG: usize = 120;

/// The largest message either side accepts.
pub const MAX_MESSAGE: usize = 16 << 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// The first message: who's playing, and which `protocol::VERSION` they speak.
    Hello { version: u32, name: String },
    Command(Command),
    /// The client has decoded view `tick`, so later views may be deltas against it.
    Ack(u64),
}

// Views are nearly every message, so boxing them would only add an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome { version: u32, player: UnitId },
    Refused(String),
    /// What the player sees now, with everything that happened since the last view.
    View(WireView),
}

/// A message as it goes on the wire: length, then postcard.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut out = vec![0; 4];
    postcard::to_io(msg, &mut out).expect("messages always serialise");
    let len = (out.len() - 4) as u32;
    out[..4].copy_from_slice(&len.to_le_bytes());
    out
}

/// Reads one message.
pub fn read<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> io::Result<T> {
    read_sized(r).map(|(msg, _)| msg)
}

/// Reads one message, and how many bytes it took on the wire.
pub fn read_sized<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> io::Result<(T, usize)> {
    let mut len = [0; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a {len}-byte message is too big")));
    }
    let mut buf = vec![0; len];
    r.read_exact(&mut buf)?;
    let msg = postcard::from_bytes(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok((msg, 4 + len))
}

/// Accepting connections into a world.
pub struct Listener {
    addr: SocketAddr,
    _thread: JoinHandle<()>,
}

impl Listener {
    /// Where it listens (with the real port when bound to port 0).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

/// Starts accepting players into `server` on `addr`, on a thread of its own. Once the server
/// stops, connections are turned away.
pub fn listen(server: &Server, addr: impl ToSocketAddrs) -> io::Result<Listener> {
    let socket = TcpListener::bind(addr)?;
    let addr = socket.local_addr()?;
    let control = server.control.clone();
    let commands = server.commands.clone();
    let thread = std::thread::Builder::new().name("Aldermoor listener".into()).spawn(move || {
        for stream in socket.incoming().flatten() {
            let (control, commands) = (control.clone(), commands.clone());
            let _ = std::thread::Builder::new().name("Aldermoor connection".into()).spawn(move || {
                if let Err(e) = serve(stream, control, commands) {
                    eprintln!("connection closed: {e}");
                }
            });
        }
    })?;
    Ok(Listener { addr, _thread: thread })
}

fn serve(stream: TcpStream, control: crossbeam_channel::Sender<crate::Control>, commands: crossbeam_channel::Sender<(UnitId, Command)>) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let peer = stream.peer_addr()?;
    let mut reader = BufReader::new(stream.try_clone()?);
    // A connection that doesn't say hello promptly isn't a client.
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let name = match read::<ClientMsg>(&mut reader)? {
        ClientMsg::Hello { version, name } if version == protocol::VERSION => name,
        ClientMsg::Hello { version, .. } => {
            let why = format!("this server speaks protocol {}, not {version}", protocol::VERSION);
            (&stream).write_all(&encode(&ServerMsg::Refused(why.clone())))?;
            return Err(io::Error::other(why));
        }
        ClientMsg::Command(_) | ClientMsg::Ack(_) => return Err(io::Error::new(io::ErrorKind::InvalidData, "a command before hello")),
    };
    stream.set_read_timeout(None)?;
    let (frames, outbox) = crossbeam_channel::bounded::<Vec<u8>>(BACKLOG);
    let (reply, answer) = crossbeam_channel::bounded(1);
    let acked = Arc::new(AtomicU64::new(0));
    let remote = crate::Remote { frames, acked: acked.clone(), encoder: protocol::wire::Encoder::new(), phase: 0 };
    control
        .send(crate::Control::Join { name: name.clone(), viewer: Viewer::Remote(remote), reply })
        .map_err(|_| io::Error::other("the server has stopped"))?;
    let player = answer.recv().map_err(|_| io::Error::other("the server has stopped"))?;
    eprintln!("{name} joined from {peer} as unit {player}");
    let mut writer = BufWriter::new(stream.try_clone()?);
    writer.write_all(&encode(&ServerMsg::Welcome { version: protocol::VERSION, player }))?;
    writer.flush()?;
    // The writer sends views until the socket or the server goes; the server drops a connection
    // that falls BACKLOG views behind by dropping its channel.
    let sending = stream.try_clone()?;
    std::thread::Builder::new().name("Aldermoor sender".into()).spawn(move || {
        while let Ok(frame) = outbox.recv() {
            // Flush once caught up, so a burst of views goes out in one write.
            if writer.write_all(&frame).and_then(|_| if outbox.is_empty() { writer.flush() } else { Ok(()) }).is_err() {
                break;
            }
        }
        let _ = sending.shutdown(Shutdown::Both);
    })?;
    let result = loop {
        match read::<ClientMsg>(&mut reader) {
            Ok(ClientMsg::Command(c)) => {
                if commands.send((player, c)).is_err() {
                    break Ok(());
                }
            }
            Ok(ClientMsg::Ack(tick)) => {
                acked.fetch_max(tick.wrapping_add(1), Ordering::AcqRel);
            }
            Ok(ClientMsg::Hello { .. }) => break Err(io::Error::new(io::ErrorKind::InvalidData, "a second hello")),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break Ok(()),
            Err(e) => break Err(e),
        }
    };
    // Closing the socket ends the writer, which drops the channel, and the server lets go.
    let _ = stream.shutdown(Shutdown::Both);
    eprintln!("{name} left");
    result
}

/// A player connected to a server: what bots, load tests and (later) the client use.
pub struct Client {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    decoder: Decoder,
    last_size: usize,
    pub player: UnitId,
}

impl Client {
    /// Connects and says hello. Fails if the server refuses (another protocol version).
    pub fn connect(addr: impl ToSocketAddrs, name: &str) -> io::Result<Client> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        (&stream).write_all(&encode(&ClientMsg::Hello { version: protocol::VERSION, name: name.to_string() }))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        match read::<ServerMsg>(&mut reader)? {
            ServerMsg::Welcome { player, .. } => Ok(Client { stream, reader, decoder: Decoder::new(), last_size: 0, player }),
            ServerMsg::Refused(why) => Err(io::Error::new(io::ErrorKind::ConnectionRefused, why)),
            ServerMsg::View { .. } => Err(io::Error::new(io::ErrorKind::InvalidData, "a view before welcome")),
        }
    }

    pub fn send(&mut self, c: Command) -> io::Result<()> {
        (&self.stream).write_all(&encode(&ClientMsg::Command(c)))
    }

    /// Waits for the next view, rebuilds it whole and acknowledges it.
    pub fn next_view(&mut self) -> io::Result<(Snapshot, CombatView)> {
        loop {
            let (msg, size) = read_sized::<ServerMsg>(&mut self.reader)?;
            self.last_size = size;
            if let ServerMsg::View(view) = msg {
                let tick = view.tick;
                let decoded = self.decoder.decode(view).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                (&self.stream).write_all(&encode(&ClientMsg::Ack(tick)))?;
                return Ok(decoded);
            }
        }
    }

    /// The wire size of the last message `next_view` read.
    pub fn last_size(&self) -> usize {
        self.last_size
    }
}
