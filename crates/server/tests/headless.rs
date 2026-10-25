//! The real `server` binary starts with no window or GPU, and a client can join it and play.

use glam::Vec2;
use protocol::Command;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command as Process, Stdio};

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_headless_server_takes_a_client() {
    let mut child = Process::new(env!("CARGO_BIN_EXE_server"))
        .args(["--bind", "127.0.0.1:0", "--seed", "7"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("the server starts");
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let _running = Running(child);
    let addr = line.trim().strip_prefix("listening on ").unwrap_or_else(|| panic!("unexpected first line {line:?}"));

    let mut client = server::net::Client::connect(addr, "Tester").unwrap();
    let (first, _) = client.next_view().unwrap();
    let p0 = first.unit(client.player).expect("the player is in its own view").position;
    client.send(Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, sprint: false, face_yaw: None }).unwrap();
    let mut last = first.clone();
    for _ in 0..60 {
        last = client.next_view().unwrap().0;
    }
    assert!(last.tick >= first.tick + 60, "a view a step: {} to {}", first.tick, last.tick);
    let moved = (last.unit(client.player).unwrap().position - p0).length();
    assert!(moved > 3.0, "moved {moved} m in a second");
}

#[test]
fn an_old_client_is_turned_away() {
    use std::io::Write;
    let mut child = Process::new(env!("CARGO_BIN_EXE_server")).args(["--bind", "127.0.0.1:0"]).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let _running = Running(child);
    let mut stream = std::net::TcpStream::connect(line.trim().strip_prefix("listening on ").unwrap()).unwrap();
    stream.write_all(&server::net::encode(&server::net::ClientMsg::Hello { version: 0, name: "Old".into() })).unwrap();
    match server::net::read::<server::net::ServerMsg>(&mut stream).unwrap() {
        server::net::ServerMsg::Refused(why) => assert!(why.contains("protocol"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}
