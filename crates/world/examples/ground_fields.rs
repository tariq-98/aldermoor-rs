//! Samples the valley for painting its ground: `cargo run -p world --release --example
//! ground_fields -- OUT_DIR [SIZE]`.
//!
//! Writes `fields.bin`: SIZE × SIZE texels covering ±BOUNDS (row 0 is the north edge, z = −280;
//! column 0 the west edge), as little-endian f32 planes in this order: height, the normal's x,
//! y and z, distance to the nearest road, and the signed distance south of the river's centre
//! line. And `places.json`: the trees and houses. The art build turns these into the terrain's
//! layer weights (art/blender/build_world.py).

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
    std::fs::write(
        out.join("places.json"),
        format!(
            "{{\"size\": {size}, \"bounds\": {}, \"trees\": [{}], \"houses\": [{}]}}\n",
            world::BOUNDS,
            list(&valley.trees),
            list(&valley.houses)
        ),
    )?;
    println!("wrote {size}x{size} fields and {} trees, {} houses to {}", valley.trees.len(), valley.houses.len(), out.display());
    Ok(())
}
