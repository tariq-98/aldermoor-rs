//! Bots: players that join over the network like a real client and wander, chase and fight, so
//! load tests (#74) measure the server the way an MMO loads it.

use crate::net::Client;
use glam::{Vec2, Vec3, Vec3Swizzles};
use protocol::{Command, UnitId};
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// What one bot saw.
#[derive(Clone, Copy, Debug, Default)]
pub struct BotReport {
    pub views: u64,
    pub bytes: u64,
    /// Views in which it was fighting something.
    pub fighting: u64,
}

/// Runs a bot until `stop` is set or the server goes. `seed` makes its choices its own.
pub fn run(addr: SocketAddr, name: &str, seed: u64, stop: Arc<AtomicBool>) -> io::Result<BotReport> {
    let mut client = Client::connect(addr, name)?;
    let me = client.player;
    let mut rng = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut report = BotReport::default();
    let mut heading = Vec2::X;
    while !stop.load(Ordering::Relaxed) {
        let (snapshot, _) = match client.next_view() {
            Ok(v) => v,
            Err(e) if report.views > 0 && e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        };
        report.views += 1;
        report.bytes += client.last_size() as u64;
        let Some(mine) = snapshot.unit(me) else { continue };
        let at = mine.position.xz();
        let target = nearest_foe(&snapshot.units, me, at, 20.0);
        if target.is_some() {
            report.fighting += 1;
        }
        // A real player acts a few times a second, not every view.
        if !report.views.is_multiple_of(5) {
            continue;
        }
        if snapshot.player.as_ref().is_some_and(|p| p.can_respawn) {
            client.send(Command::Respawn)?;
            continue;
        }
        let r = next(&mut rng);
        let direction = match target {
            Some((id, there)) => {
                client.send(Command::SetTarget(Some(id)))?;
                client.send(Command::SetAutoAttack(true))?;
                if r.is_multiple_of(4) {
                    client.send(Command::UseSlot { slot: (r >> 8) as u8 % 5, ground: Some(there) })?;
                }
                let to = there.xz() - at;
                if to.length() > 2.0 { to.normalize() } else { Vec2::ZERO }
            }
            None => {
                // Wander, turning now and then.
                if r.is_multiple_of(12) {
                    let a = (r >> 16) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
                    heading = Vec2::new(a.cos(), a.sin());
                }
                heading
            }
        };
        client.send(Command::Move { direction, walk: false, sprint: false, face_yaw: None })?;
    }
    Ok(report)
}

/// The closest living enemy within `range`.
fn nearest_foe(units: &[protocol::UnitState], me: UnitId, at: Vec2, range: f32) -> Option<(UnitId, Vec3)> {
    units
        .iter()
        .filter(|u| u.id != me && u.hostile && u.health > 0.0 && u.position.xz().distance(at) < range)
        .min_by(|a, b| a.position.xz().distance_squared(at).total_cmp(&b.position.xz().distance_squared(at)))
        .map(|u| (u.id, u.position))
}

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
