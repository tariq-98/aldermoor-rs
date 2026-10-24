//! Apply supplied Look dyes and colourways exactly as the character workshop does.
use crate::model::Texture;
use anyhow::{Context, Result};
use std::{collections::HashMap, sync::Arc};
#[derive(Clone)]
pub struct Dye {
    pub mask: Arc<Texture>,
    pub colours: [[f32; 4]; 2],
    pub references: [[f32; 4]; 2],
}
#[derive(Clone, Default)]
pub struct Appearance {
    pub dye: Option<Arc<Dye>>,
    pub variants: HashMap<String, Arc<Texture>>,
}
impl Appearance {
    pub fn load(
        catalog: &assets::Catalog,
        cache: &mut crate::model::ModelCache,
        look: &protocol::Look,
        id: &str,
        def: &serde_json::Value,
        maps: impl Iterator<Item = Arc<Texture>>,
    ) -> Result<Self> {
        let mut style = Self::default();
        if let Some((_, main, trim)) = look.dyes.iter().find(|(target, ..)| target == id)
            && let Some(file) = def["dye"].as_str()
        {
            let mut colours = [[0f32; 4]; 2];
            let mut references = [[1f32; 4]; 2];
            for (channel, choice) in [main, trim].into_iter().enumerate() {
                let tone = choice
                    .as_ref()
                    .and_then(|id| catalog.humanoid.dyes.iter().find(|d| &d.id == id));
                if let Some(tone) = tone {
                    colours[channel] = [tone.linear[0], tone.linear[1], tone.linear[2], 1.];
                    let key = if channel == 0 { "r" } else { "g" };
                    let reference = def["dye_channels"]
                        .as_array()
                        .and_then(|a| a.iter().find(|v| v["channel"] == key))
                        .and_then(|v| v["reference"].as_array())
                        .context("dye channel lacks its authored reference")?;
                    for i in 0..3 {
                        references[channel][i] =
                            reference[i].as_f64().context("invalid dye reference")? as f32;
                    }
                }
            }
            if colours.iter().any(|c| c[3] > 0.) {
                style.dye = Some(Arc::new(Dye {
                    mask: cache.texture(&catalog.path(file))?,
                    colours,
                    references,
                }));
            }
        }
        // Workshop switches only materials whose base map offers the selected colourway.
        if let Some((_, want)) = look.variants.iter().find(|(target, _)| target == id) {
            for original in maps {
                let stem = std::path::Path::new(&original.key)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if catalog
                    .humanoid
                    .texture_variants
                    .get(stem)
                    .is_some_and(|v| v.contains(want))
                {
                    let root = catalog.path("humanoid/textures");
                    let file = ["jpg", "png"]
                        .into_iter()
                        .map(|ext| root.join(format!("{want}.{ext}")))
                        .find(|p| p.exists())
                        .context("catalogue colourway image is missing")?;
                    style
                        .variants
                        .insert(original.key.clone(), cache.texture(&file)?);
                }
            }
        }
        Ok(style)
    }
}
