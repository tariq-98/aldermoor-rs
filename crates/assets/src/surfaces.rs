//! The ground, water and sky the renderer is given (`catalog/surfaces.json`) and the light it
//! draws them in (`catalog/look.json`): every file is there, the terrain's layers can stack
//! into texture arrays, and the numbers make sense. Both files are optional.

use std::path::Path;

use serde_json::Value;

use crate::Problem;

/// Width and height of a PNG or JPEG, read from its header.
pub fn image_size(path: &Path) -> Result<(u32, u32), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() >= 24 && bytes[..8] == *b"\x89PNG\r\n\x1a\n" {
        let at = |o: usize| u32::from_be_bytes(bytes[o..o + 4].try_into().unwrap());
        return Ok((at(16), at(20)));
    }
    if bytes.len() >= 4 && bytes[..2] == [0xFF, 0xD8] {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            // Start-of-frame markers (baseline, extended, progressive) carry the size.
            if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Ok((w, h));
            }
            i += 2 + length;
        }
    }
    Err(format!("{}: not a PNG or JPEG this check can read", path.display()))
}

fn numbers(v: &Value) -> Option<Vec<f64>> {
    v.as_array()?.iter().map(Value::as_f64).collect()
}

fn read(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// The size of the image `rel` names, or why it can't be had.
fn size(root: &Path, what: &str, rel: Option<&str>) -> Result<(u32, u32), String> {
    let rel = rel.ok_or_else(|| format!("surfaces.json: {what} names no file"))?;
    image_size(&root.join(rel)).map_err(|e| format!("surfaces.json: {what}: {e}"))
}

pub(crate) fn check(root: &Path) -> Vec<Problem> {
    let mut out: Vec<String> = Vec::new();
    let surfaces = root.join("catalog/surfaces.json");
    if surfaces.exists() {
        match read(&surfaces) {
            Err(e) => out.push(format!("surfaces.json: {e}")),
            Ok(v) => check_surfaces(root, &v, &mut out),
        }
    }
    let look = root.join("catalog/look.json");
    if look.exists() {
        match read(&look) {
            Err(e) => out.push(format!("look.json: {e}")),
            Ok(v) => check_look(&v, &mut out),
        }
    }
    out.into_iter().map(Problem).collect()
}

fn check_surfaces(root: &Path, v: &Value, out: &mut Vec<String>) {
    let terrain = &v["terrain"];
    let layers = terrain["layers"].as_array().cloned().unwrap_or_default();
    if !(1..=4).contains(&layers.len()) {
        out.push(format!("surfaces.json: the terrain needs 1 to 4 layers, not {}", layers.len()));
    }
    let mut colours = Vec::new();
    let mut normals = Vec::new();
    for (i, l) in layers.iter().enumerate() {
        for (kind, list) in [("colour", &mut colours), ("normal", &mut normals)] {
            match size(root, &format!("terrain layer {i} {kind}"), l[kind].as_str()) {
                Ok(s) => list.push(s),
                Err(e) => out.push(e),
            }
        }
        let metres = l["uv_metres"].as_f64().unwrap_or(0.0);
        let rough = l["roughness"].as_f64().unwrap_or(-1.0);
        if metres <= 0.0 || !(0.0..=1.0).contains(&rough) {
            out.push(format!("surfaces.json: terrain layer {i} needs uv_metres above 0 and roughness in 0..1"));
        }
    }
    // The renderer stacks the layers into texture arrays: one size for each kind.
    for (kind, sizes) in [("colour", &colours), ("normal", &normals)] {
        if sizes.windows(2).any(|w| w[0] != w[1]) {
            out.push(format!("surfaces.json: terrain {kind} maps differ in size: {sizes:?}"));
        }
    }
    match size(root, "terrain weights", terrain["weights"].as_str()) {
        Ok((w, h)) if w != h => out.push(format!("surfaces.json: the terrain weights should be square, not {w}x{h}")),
        Ok(_) => {}
        Err(e) => out.push(e),
    }
    match numbers(&terrain["bounds"]).as_deref() {
        Some([a, b, c, d]) if c > a && d > b => {}
        _ => out.push("surfaces.json: terrain bounds should be [min_x, min_z, max_x, max_z]".into()),
    }
    let water = &v["water"];
    if let Err(e) = size(root, "water normal", water["normal"].as_str()) {
        out.push(e);
    }
    let ior = water["ior"].as_f64().unwrap_or(0.0);
    let opacity = water["opacity"].as_f64().unwrap_or(-1.0);
    if ior <= 1.0 || !(0.0..=1.0).contains(&opacity) {
        out.push("surfaces.json: water needs an ior above 1 and an opacity in 0..1".into());
    }
    match size(root, "sky", v["sky"]["texture"].as_str()) {
        Ok((w, h)) if w != 2 * h => out.push(format!("surfaces.json: the sky is equirectangular, twice as wide as tall, not {w}x{h}")),
        Ok(_) => {}
        Err(e) => out.push(e),
    }
}

fn check_look(v: &Value, out: &mut Vec<String>) {
    let sun = numbers(&v["sun_direction"]).unwrap_or_default();
    if sun.len() != 3 || sun.iter().map(|x| x * x).sum::<f64>() < 1e-6 || sun[1] <= 0.0 {
        out.push("look.json: the sun should be a direction above the horizon".into());
    }
    for key in ["sun_intensity", "fog_density", "exposure", "wind_strength"] {
        if !v[key].as_f64().is_some_and(|x| x.is_finite() && x >= 0.0) {
            out.push(format!("look.json: {key} should be a number of at least 0"));
        }
    }
    if !matches!(v["tone_map"].as_str(), Some("linear" | "aces")) {
        out.push("look.json: tone_map is linear or aces".into());
    }
}
