//! Just enough glTF reading to check the catalogue: node names, skins, attributes and clips.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

use crate::Error;

#[derive(Debug, Clone)]
pub struct Gltf {
    pub json: Value,
}

impl Gltf {
    /// Reads `.gltf` (JSON) or `.glb` (binary container) and keeps the JSON part.
    pub fn open(path: &Path) -> Result<Gltf, Error> {
        let bytes = std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let json = if bytes.starts_with(b"glTF") {
            if bytes.len() < 20 {
                return Err(Error(format!("{}: truncated glb", path.display())));
            }
            let len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
            let kind = &bytes[16..20];
            if kind != b"JSON" || bytes.len() < 20 + len {
                return Err(Error(format!("{}: glb without a JSON chunk", path.display())));
            }
            serde_json::from_slice(&bytes[20..20 + len])
        } else {
            serde_json::from_slice(&bytes)
        }
        .map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let g = Gltf { json };
        // External buffers and images must exist too.
        let dir = path.parent().unwrap_or(Path::new("."));
        for key in ["buffers", "images"] {
            for item in g.array(key) {
                if let Some(uri) = item.get("uri").and_then(Value::as_str) {
                    if !uri.starts_with("data:") && !dir.join(uri).exists() {
                        return Err(Error(format!("{}: missing {key} file {uri}", path.display())));
                    }
                }
            }
        }
        Ok(g)
    }

    fn array(&self, key: &str) -> Vec<&Value> {
        self.json.get(key).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
    }

    pub fn node_names(&self) -> BTreeSet<String> {
        self.array("nodes").iter().filter_map(|n| n.get("name").and_then(Value::as_str)).map(str::to_string).collect()
    }

    pub fn joint_names(&self) -> BTreeSet<String> {
        let nodes = self.array("nodes");
        let mut out = BTreeSet::new();
        for skin in self.array("skins") {
            for j in skin.get("joints").and_then(Value::as_array).into_iter().flatten() {
                if let Some(name) = j.as_u64().and_then(|i| nodes.get(i as usize)).and_then(|n| n.get("name")).and_then(Value::as_str) {
                    out.insert(name.to_string());
                }
            }
        }
        out
    }

    pub fn animation_names(&self) -> BTreeSet<String> {
        self.array("animations").iter().filter_map(|a| a.get("name").and_then(Value::as_str)).map(str::to_string).collect()
    }

    /// Morph target names (Blender writes them to each mesh's `extras.targetNames`).
    pub fn morph_target_names(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for mesh in self.array("meshes") {
            for n in mesh.get("extras").and_then(|e| e.get("targetNames")).and_then(Value::as_array).into_iter().flatten() {
                if let Some(n) = n.as_str() {
                    out.insert(n.to_string());
                }
            }
        }
        out
    }

    /// How many draws the file costs the renderer: a primitive of every mesh a node uses.
    pub fn draws(&self) -> usize {
        let meshes = self.array("meshes");
        self.array("nodes")
            .iter()
            .filter_map(|n| n.get("mesh").and_then(Value::as_u64))
            .filter_map(|i| meshes.get(i as usize))
            .map(|m| m.get("primitives").and_then(Value::as_array).map_or(0, Vec::len))
            .sum()
    }

    /// How many different materials its primitives use.
    pub fn materials(&self) -> usize {
        let mut seen = BTreeSet::new();
        for mesh in self.array("meshes") {
            for p in mesh.get("primitives").and_then(Value::as_array).into_iter().flatten() {
                seen.insert(p.get("material").and_then(Value::as_u64));
            }
        }
        seen.len()
    }

    /// True when every primitive of every mesh carries the attribute.
    pub fn all_meshes_have(&self, attribute: &str) -> bool {
        self.array("meshes").iter().all(|m| {
            m.get("primitives")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .all(|p| p.get("attributes").and_then(|a| a.get(attribute)).is_some())
        })
    }

    /// True when every primitive of every skinned mesh carries the attribute.
    pub fn all_skinned_have(&self, attribute: &str) -> bool {
        let meshes = self.array("meshes");
        for node in self.array("nodes") {
            if node.get("skin").is_none() {
                continue;
            }
            let Some(mesh) = node.get("mesh").and_then(Value::as_u64).and_then(|i| meshes.get(i as usize)) else { continue };
            for prim in mesh.get("primitives").and_then(Value::as_array).into_iter().flatten() {
                if prim.get("attributes").and_then(|a| a.get(attribute)).is_none() {
                    return false;
                }
            }
        }
        true
    }
}
