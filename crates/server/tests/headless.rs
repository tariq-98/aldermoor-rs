//! The real `server` binary starts with no window or GPU, and a client can join it and play.

use glam::Vec2;
use protocol::Command;
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Command as Process, Stdio};

/// The server, and its output kept open while it runs: closing it would fail the server's next
/// line ("failed printing to stdout").
struct Running(Child, #[allow(dead_code)] BufReader<ChildStdout>);

/// Starts the server binary on a free port with its certificate in a folder of its own, and
/// returns it with its address and certificate.
fn start(name: &str, args: &[&str]) -> (Running, String, server::net::CertificateDer<'static>) {
    let dir = std::env::temp_dir().join(format!("aldermoor-headless-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cert = dir.join("server.der");
    let mut child = Process::new(env!("CARGO_BIN_EXE_server"))
        .args(["--bind", "127.0.0.1:0", "--cert", cert.to_str().unwrap()])
        .args(args)
        .stdout(Stdio::piped())
        .spawn()
        .expect("the server starts");
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    let addr = line.trim().strip_prefix("listening on ").unwrap_or_else(|| panic!("unexpected first line {line:?}")).to_string();
    let certificate = server::net::CertificateDer::from(std::fs::read(&cert).expect("the server saved its certificate"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(cert.with_extension("key")).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "the key is readable by others: {mode:o}");
    }
    (Running(child, out), addr, certificate)
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_headless_server_takes_a_client() {
    let (_running, addr, certificate) = start("takes", &["--seed", "7"]);
    let mut client = server::net::Client::connect(addr.as_str(), &certificate, "Tester").unwrap();
    let (first, _) = client.next_view().unwrap();
    let p0 = first.unit(client.player).expect("the player is in its own view").position;
    client.send(Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, sprint: false, face_yaw: None }).unwrap();
    let mut last = first.clone();
    for _ in 0..20 {
        last = client.next_view().unwrap().0;
    }
    assert!(last.tick >= first.tick + 20 * server::SEND_EVERY, "a view every {} steps: {} to {}", server::SEND_EVERY, first.tick, last.tick);
    let moved = (last.unit(client.player).unwrap().position - p0).length();
    assert!(moved > 3.0, "moved {moved} m in a second");
}

#[test]
fn an_old_client_is_turned_away() {
    let (_running, addr, certificate) = start("old", &[]);
    match server::net::Client::hello(addr.as_str(), &certificate, "Old", 0) {
        Err(e) => assert!(e.to_string().contains("protocol"), "{e}"),
        Ok(_) => panic!("an old client was let in"),
    }
}

#[test]
fn a_client_refuses_a_server_it_doesnt_know() {
    let (_running, addr, _) = start("stranger", &["--seed", "2"]);
    let stranger = server::net::Identity::generate().unwrap().certificate;
    assert!(server::net::Client::connect(addr.as_str(), &stranger, "Wary").is_err());
}

#[test]
fn the_headless_server_runs_a_realm_and_its_players_cross_borders() {
    // Two regions split at x = 5; players start just east of it.
    let (_running, addr, certificate) = start("realm", &["--regions", "2", "--crowd", "40"]);
    let mut client = server::net::Client::connect(addr.as_str(), &certificate, "Crosser").unwrap();
    let (first, _) = client.next_view().unwrap();
    let x0 = first.unit(client.player).expect("the player is in its own view").position.x;
    assert!(x0 > 5.0, "started at x {x0}");
    client.send(Command::Move { direction: Vec2::new(-1.0, 0.0), walk: false, sprint: false, face_yaw: None }).unwrap();
    let mut last = first;
    for _ in 0..60 {
        last = client.next_view().unwrap().0;
    }
    let x = last.unit(client.player).expect("still in its own view after crossing").position.x;
    assert!(x < 0.0, "walked to x {x}, not across the border");
}

/// Polls `f` for up to `seconds`; whether it came true.
fn eventually(seconds: u64, f: impl Fn() -> bool) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < until {
        if f() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    f()
}

#[test]
fn a_character_is_still_there_after_the_server_restarts() {
    let dir = std::env::temp_dir().join(format!("aldermoor-accounts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("accounts.db");
    let db_arg = db.to_str().unwrap().to_string();

    // A new account and character, walked north for a second, then logged out.
    let (running, addr, certificate) = start("accounts-1", &["--seed", "7", "--accounts", &db_arg]);
    let refused = server::net::Client::connect(addr.as_str(), &certificate, "Tester");
    assert!(refused.is_err(), "a server with accounts let a plain hello in");
    let mut client = server::net::Client::login(addr.as_str(), &certificate, "Chippy", "correct horse", "Brambleguard", true).unwrap();
    client.next_view().unwrap();
    client.send(Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, sprint: false, face_yaw: None }).unwrap();
    for _ in 0..20 {
        client.next_view().unwrap();
    }
    client.send(Command::Move { direction: Vec2::ZERO, walk: false, sprint: false, face_yaw: None }).unwrap();
    let mut left_at = Vec2::ZERO;
    for _ in 0..10 {
        let (v, _) = client.next_view().unwrap();
        let p = v.unit(client.player).unwrap().position;
        left_at = Vec2::new(p.x, p.z);
    }
    drop(client);
    let store = server::accounts::Store::open(&db).unwrap();
    assert!(
        eventually(5, || store.load("Brambleguard").is_some_and(|r| (Vec2::from(r.at) - left_at).length() < 0.5)),
        "the character wasn't saved where it left: {:?} against {left_at}",
        store.load("Brambleguard").map(|r| r.at)
    );
    drop(running);

    // The server comes back: the wrong password is refused, and the right one finds the
    // character where it was.
    let (_running, addr, certificate) = start("accounts-2", &["--seed", "7", "--accounts", &db_arg]);
    assert!(server::net::Client::login(addr.as_str(), &certificate, "Chippy", "wrong password", "Brambleguard", false).is_err());
    let mut client = server::net::Client::login(addr.as_str(), &certificate, "Chippy", "correct horse", "Brambleguard", false).unwrap();
    let (v, _) = client.next_view().unwrap();
    let p = v.unit(client.player).unwrap().position;
    assert!((Vec2::new(p.x, p.z) - left_at).length() < 0.5, "back at {p}, left at {left_at}");
    let _ = std::fs::remove_dir_all(&dir);
}
