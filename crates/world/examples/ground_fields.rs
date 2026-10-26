//! Samples the valley for painting its ground: `cargo run -p world --release --example
//! ground_fields -- OUT_DIR [SIZE]`.
//!
//! Writes `fields.bin`: SIZE × SIZE texels covering ±BOUNDS (row 0 is the north edge, z = −280;
//! column 0 the west edge), as little-endian f32 planes in this order: height, the normal's x,
//! y and z, distance to the nearest road, and the signed distance south of the river's centre
//! line. And `places.json`: the trees and houses, which the art build turns into the terrain's
//! layer weights (art/blender/build_world.py); the masonry, which it builds props to fill
//! (art/blender/build_environment.py): each piece's blocks, grips and coping line in its own
//! frame, and the drawn ground under it every half metre, measured from its origin; and the
//! cottages' measurements, blocks and grips, which it builds the cottages to match.

use glam::Vec2;
use std::io::Write;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("usage: ground_fields OUT_DIR [SIZE]"));
    let size: usize = args.next().map_or(2048, |s| s.parse().expect("SIZE is a number"));
    std::fs::create_dir_all(&out)?;
    let step = world::BOUNDS * 2.0 / size as f32;
    let at = |i: usize| -world::BOUNDS + (i as f32 + 0.5) * step;

    let mut planes = vec![vec![0f32; size * size]; 6];
    for j in 0..size {
        let z = at(j);
        for i in 0..size {
            let x = at(i);
            let k = j * size + i;
            let n = world::normal(x, z);
            planes[0][k] = world::height(x, z);
            planes[1][k] = n.x;
            planes[2][k] = n.y;
            planes[3][k] = n.z;
            planes[4][k] = world::path_distance(x, z);
            planes[5][k] = z - world::river_z(x);
        }
    }
    let mut file = std::io::BufWriter::new(std::fs::File::create(out.join("fields.bin"))?);
    for plane in &planes {
        for v in plane {
            file.write_all(&v.to_le_bytes())?;
        }
    }
    file.flush()?;

    let valley = world::Valley::new();
    let list = |places: &[world::Placement]| {
        places.iter().map(|p| format!("[{}, {}, {}, {}]", p.position.x, p.position.y, p.yaw, p.scale)).collect::<Vec<_>>().join(", ")
    };
    let masonry: Vec<String> = valley.masonry.iter().map(|m| piece(&valley, m)).collect();
    std::fs::write(
        out.join("places.json"),
        format!(
            "{{\"size\": {size}, \"bounds\": {}, \"trees\": [{}], \"houses\": [{}], \"masonry\": [\n{}\n], \"cottages\": {}}}\n",
            world::BOUNDS,
            list(&valley.trees),
            list(&valley.houses),
            masonry.join(",\n"),
            cottages()
        ),
    )?;
    println!(
        "wrote {size}x{size} fields and {} trees, {} houses, {} pieces of masonry to {}",
        valley.trees.len(),
        valley.houses.len(),
        valley.masonry.len(),
        out.display()
    );
    Ok(())
}

fn f(v: f32) -> String {
    format!("{:.4}", v)
}

/// Blocks as JSON, in their piece's frame.
fn blocks(blocks: &[world::Block]) -> String {
    let list: Vec<String> = blocks
        .iter()
        .map(|b| {
            format!(
                "{{\"part\": \"{}\", \"center\": [{}, {}], \"half\": [{}, {}], \"yaw\": {}, \"top\": {}, \"rise\": {}}}",
                b.part,
                f(b.center.x),
                f(b.center.y),
                f(b.half.x),
                f(b.half.y),
                f(b.yaw),
                b.top.map_or("null".into(), f),
                f(b.rise)
            )
        })
        .collect();
    list.join(", ")
}

/// Grips as JSON, in their piece's frame.
fn grips(grips: &[world::Grip]) -> String {
    let list: Vec<String> = grips
        .iter()
        .map(|g| {
            format!(
                "{{\"part\": \"{}\", \"a\": [{}, {}, {}], \"b\": [{}, {}, {}], \"out\": [{}, {}]}}",
                g.part,
                f(g.a.x),
                f(g.a.y),
                f(g.a.z),
                f(g.b.x),
                f(g.b.y),
                f(g.b.z),
                f(g.out.x),
                f(g.out.y)
            )
        })
        .collect();
    list.join(", ")
}

/// The cottages as JSON (`world::cottage`): the measurements they share, and each one's own, with
/// its blocks and grips in its own frame.
fn cottages() -> String {
    use world::cottage::*;
    let each: Vec<String> = COTTAGES
        .iter()
        .map(|c| {
            format!(
                "{{\"prop\": \"{}\", \"rise\": {}, \"thick\": {}, \"ridge\": [{}, {}], \"chimney\": {}, \"blocks\": [{}], \"grips\": [{}]}}",
                c.prop,
                f(c.rise),
                f(c.thick),
                f(c.ridge.0),
                f(c.ridge.1),
                f(c.chimney),
                blocks(&c.blocks()),
                grips(&c.grips())
            )
        })
        .collect();
    format!(
        "{{\"width\": {}, \"depth\": {}, \"wall0\": {}, \"wall1\": {}, \"eaves\": {}, \"verge\": {}, \"kinds\": [\n{}\n]}}",
        f(WIDTH),
        f(DEPTH),
        f(WALL0),
        f(WALL1),
        f(EAVES),
        f(VERGE),
        each.join(",\n")
    )
}

/// A piece of masonry as JSON: its blocks and line, and the ground round it (a grid every half
/// metre over its blocks and 2 m beyond, rows of increasing z, in its own frame).
fn piece(valley: &world::Valley, m: &world::Masonry) -> String {
    let line: Vec<String> = m.line.iter().map(|p| format!("[{}, {}, {}]", f(p.x), f(p.y), f(p.z))).collect();
    let reach = m.blocks.iter().map(|b| b.center.abs() + Vec2::splat(b.half.length())).fold(Vec2::ZERO, Vec2::max) + Vec2::splat(2.0);
    let (nx, nz) = ((reach.x * 2.0).ceil() as i32, (reach.y * 2.0).ceil() as i32);
    let (s, c) = m.yaw.sin_cos();
    let mut ground = Vec::new();
    for j in -nz..=nz {
        for i in -nx..=nx {
            let (x, z) = (i as f32 * 0.5, j as f32 * 0.5);
            ground.push(f(valley.surface(m.origin.x + c * x + s * z, m.origin.z - s * x + c * z) - m.origin.y));
        }
    }
    format!(
        "{{\"prop\": \"{}\", \"origin\": [{}, {}, {}], \"yaw\": {}, \"blocks\": [{}], \"grips\": [{}], \"line\": [{}], \"ground\": {{\"from\": [{}, {}], \"step\": 0.5, \"size\": [{}, {}], \"heights\": [{}]}}}}",
        m.prop,
        f(m.origin.x),
        f(m.origin.y),
        f(m.origin.z),
        f(m.yaw),
        blocks(&m.blocks),
        grips(&m.grips),
        line.join(", "),
        f(-nx as f32 * 0.5),
        f(-nz as f32 * 0.5),
        2 * nx + 1,
        2 * nz + 1,
        ground.join(", ")
    )
}
