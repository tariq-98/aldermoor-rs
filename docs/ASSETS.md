# Aldermoor assets

Claude builds the art as well as the game (Tariq, 24 October 2026). The requirements come from the
design interview with Codex, summarised in [DESIGN.md](DESIGN.md). The headline: **everything is
equipped correctly.**
- Capes stay on the shoulders.
- Weapons sit in the hand and in their sheaths.
- Nothing pokes through anything else.
- Every outfit fits every body.

## Art direction

- **Breath of the Wild in spirit:** stylised, expressive proportions, strong silhouettes,
  restrained texture detail and cohesive lighting.
- **Practical gear:** most equipment looks practical and proportionate. Exceptional pieces get
  bolder silhouettes and restrained effects.
- **Hard exclusion:** no spiders, other arachnids, spider-like creatures or web scenery, anywhere.

## Foundation

| Source | What we use | Licence |
|---|---|---|
| [Quaternius Universal Base Characters](https://quaternius.itch.io/universal-base-characters) (free tier) | Two bodies, six hairstyles and beards, two eyebrow sets | CC0 |
| [Quaternius Modular Outfits, Fantasy](https://quaternius.itch.io/modular-character-outfits-fantasy) (free tier) | Peasant and Ranger outfits, 20 parts, two colourways each | CC0 |
| [Quaternius Universal Animation Library 1 and 2](https://quaternius.itch.io/universal-animation-library) (free tiers) | 84 clips: locomotion, roll, sword combos, block, shield, casting, hits, deaths, farming, chopping, sitting, talking, zombies | CC0 |
| Our own (built in `art/blender/`) | Weapons, shields, bows, staves, capes, backpacks, armour, garments, race parts, creatures, 29 more clips | Ours |

All of these share one 65-bone humanoid rig with Unreal-mannequin bone names. Downloads are listed,
hashed and licensed in `art/vendor.json`; `art/fetch_vendor.sh` re-fetches them. The archives
stay out of git. Quaternius published a separate Quaternius Asset Licence on 28 August 2026, but
these packs' pages and their `License.txt` files say CC0. Either licence allows shipping them in
the game.

**No paid packs** (Tariq, 24 October 2026). Everything beyond these free CC0 sources is built
in `art/blender/`: more hairstyles, outfits, bodies, creatures and clips.

## The humanoid family

**One skeleton, many proportions.** Every humanoid skeleton uses the Universal Animation Library's
rest orientations for every bone; only joint positions differ between bodies, races and builds.
- A clip's local rotations then play unchanged on any of them.
- The renderer takes bone translations from the skeleton's own rest pose, except the pelvis. Its
  clip translation is scaled by `body.pelvis.y / animations.pelvis.y`.
- The build normalises every imported rig this way (`art/blender/aldermoor_art/rig.py`).

**Body types.** "Broad frame" and "Slender frame", built from the Quaternius male and female
bodies. Every outfit and hairstyle is offered on both; nothing is gendered in the game.

**Races** are the human frames rescaled bone by bone, so every piece of clothing follows. Elves,
dwarves and goblins add pointed ears, a bigger nose or a dwarven beard. **Foxfolk** have a
muzzle (the same shaping moves beards, brows and hoods), fox ears on top of the head, a tail on
six bones (`tail_01` to `tail_06`, rooted on the pelvis) and fur instead of skin:
- **Fur:** the body's material is one tintable `skin.*` material whose detail map is fur. Its
  `markings` mask (R the light fur: muzzle, throat, bib, inner ears, tail tip; G the dark: nose,
  ear backs, socks and gloves) shares the body's UVs. Fur tones have `light` and `dark` colours
  as well as `linear`. With `c` = 2 × linear, `l` = 2 × light and `d` = 2 × dark, the base colour
  is `detail × mix(mix(c, l, R), d, G)`.
- **The tail** swings on springs: the body type's `springs` (chains, stiffness, drag, gravity,
  radius) work like a cape's, and the body's `colliders` include capsules on the tail bones, so
  capes drape over it. A chain never collides with capsules on its own bones.
- **Headwear** hides the fox ears (every head piece hides `ears` on foxfolk).

**Body regions.** Every skinned piece carries a per-vertex float attribute `_REGION` (glTF
`_REGION`; three.js `_region`), assigned from the skeleton:
- the head: `scalp`, `face`, `ears`, `neck`;
- the torso: `chest`, `belly`, `hips`;
- the arms: `upperarm_hi`, `upperarm_lo`, `forearm_hi`, `forearm_lo`, `hand`;
- the legs: `thigh_hi`, `thigh_lo`, `shin_hi`, `shin_lo`, `foot`;
- `tail`.

The shin splits where trousers end, so trousers plus shoes replace a lower leg exactly.

**Layers and hiding.** Each piece has a `layer`:
- body 0;
- trousers 1.0, shirts 1.1, footwear 1.2;
- armour 2.x;
- headwear 2.5;
- shoulders 3;
- the back slot 4.

A piece hides regions only on pieces with a lower layer. Its catalogue entry lists, per body type,
the regions it hides on the body and on each lower piece it overlaps (`hides`). The renderer drops
a piece's triangles whose `_REGION` bit is set in the union of the equipped pieces' hides for it:
- **the body:** a bitmask of `hides[*].body`;
- **another piece:** a bitmask of `hides[*][that piece]`.

Hair is hidden by any piece with `covers_hair`. Beards and brows are hidden when the face is.

**Fitting.** The build checks fit three ways:
- **Coverage:** ray tests measure what each piece really covers. Quaternius parts replace whole
  body sections, so they also declare their hides, and the build prints a warning if a
  declaration disagrees with the geometry.
- **Clearance:** cloth is pushed 3 mm clear of every body region it leaves visible.
- **Hair:** hair is pushed clear of the scalp after moving onto each head.

**Sockets.** Bones whose names start with `socket_` carry equipment. Only body files carry them;
clothing binds to the body's skeleton by bone name.
- **The authoring rule:** an item is authored in Blender with its attachment point at the origin,
  "up" along +Z and "front" along −Y. A socket's local +Y is where the item's up points, and its
  local +Z is where the item's front points.
- **In glTF terms:** an item file's +Y and +Z axes map onto the socket node's +Y and +Z.
- **Weapons:** up is the blade or shaft tip, and front is the cutting edge. The origin is the
  centre of the grip: near the guard for swords, 60% of the way up a staff, the middle of a
  spear (so poles stand on the ground at the hand's height and ride the back centred).
- **In the hand:** a hand socket's +Y runs from the little finger to the index finger (out of the
  thumb side of a fist) and its +Z along the back of the hand, towards the knuckles.

| Socket | Parent | Holds |
|---|---|---|
| `socket_hand_r`, `socket_hand_l` | hands | Drawn weapons, wands, torches |
| `socket_shield_l` | left forearm | Strapped shields |
| `socket_back` | upper spine | Backpacks; the reference point for cape collars |
| `socket_back_1`, `socket_back_2` | upper spine | Blades, hilt up behind the right (1) or left (2) shoulder, where that hand reaches |
| `socket_back_pole` | upper spine | Staves, spears and long tools, head up over the right shoulder |
| `socket_back_bow` | upper spine | Bows, the upper limb over the left shoulder (the mirror of `socket_back_pole`) |
| `socket_back_shield` | upper spine | Shields, upright in the middle of the back, over anything slung beneath |
| `socket_quiver` | upper spine | Quivers, arrows over the right shoulder |
| `socket_quiver_hip` | pelvis | The quiver when a backpack takes the back |
| `socket_pack` | upper spine | A shield on the travel pack's outer face |
| `socket_pack_side`, `socket_pack_side_r` | upper spine | Long items down the pack's left side (bows) and right side (poles) |
| `socket_pack_blade` | upper spine | Blades on the pack's right side, hilt up |
| `socket_hip_l`, `socket_hip_r` | pelvis | Sheathed one-handers, hilt forward |
| `socket_head` | head | Rigid hats and helmets |

**Tints.**
- **Skin and hair:** these materials are named `skin.*` and `hair.*`. Their colour map is a
  tone-neutral detail map with a mean of 0.5. The renderer sets the base colour factor to twice
  the chosen linear tone (`skin_tones` and `hair_colours` in the catalogue).
- **Outfits:** they keep their painted textures. The catalogue's `texture_variants` lists
  alternative colourways.
- **Dyes:** a dyeable piece has `dye` (a mask texture: R is the main cloth, G the trim) and
  `dye_channels`, the linear colour each channel was painted as. To dye channel *k* with linear
  colour *d* (mask *m*, reference *ref*):
  `out = mix(out, base * d / max(ref, 0.01), m)`.
  This keeps the weave, stitching and shading, and gives exactly *d* where the cloth was
  painted *ref*. Apply it after the base texture is decoded to linear and before lighting.
  The palette is the catalogue's `dyes` (`id`, `label`, `linear`); the workshop offers it per
  channel of every dyeable piece.

## Catalogue: `assets/catalog/humanoid.json`

The build writes `assets/catalog/humanoid.json`; consumers read it and nothing else. Paths are
relative to `assets/`. Positions are glTF (metres, +Y up, +Z forward).

| Key | What it holds |
|---|---|
| `regions` | Region names in bit order |
| `skin_tones`, `hair_colours` | `id` plus `linear` RGB |
| `dyes` | The dye palette: `id`, `label` and `linear` RGB |
| `body_types.<id>` | `label`, `file` (the body with the skeleton and sockets), `pelvis`, `height`, `skin_reference`, `sockets`, `colliders` (cloth capsules: `bone`, `radius`, and ends `a` and `b` in the bone's node frame) |
| `defaults.<body type>` | A starting look: pieces, hair, brows, beard, tone and hair colour |
| `animations` | `file` (`anims.glb`: the reference skeleton plus every clip), `pelvis`, `fps`, and `clips` (each with `name`, `seconds`, `loop`, `group` and `tags`; our own clips add `source: "authored"`, and may add `upper_body`, `curves` and `events`: see "Clips, curves and events") |
| `hair[]` | `id`, `kind` (`hair`, `beard` or `brows`), `label`, and `files.<body type>` |
| `pieces[]` | `id`, `slot`, `layer`, `label`, `files.<body type>`, `hides.<body type>`, `covers_hair`, `coverage` (diagnostic), and for dyeable pieces `dye` and `dye_channels`. `source` names the build that owns the piece; `idle` names the clip to stand in while it's worn |
| `texture_variants`, `texture_variant_labels` | Base texture name mapped to its alternatives, and display names for each colourway |

Each entry in `hides.<body type>` maps either `body` or a lower piece's id to the list of regions
to drop.

Race and creature body types add `race`, `frame`, `gear_scale` (how much to scale held and
stowed rigid items), and `creature` / `dress` flags; foxfolk add `markings` and `springs`. `races.<id>.frames` maps the frame
choices to body types. The beast catalogue, `assets/catalog/creatures.json`, lists beasts on
their own rigs: `file` (one glb with mesh, skeleton and clips), `clips` by role (idle, walk,
run, attack, hit, death, jump, special), `speeds`, and `sim_body` (the simulation's body name).

The gear catalogue, `assets/catalog/gear.json`, follows the same rules. Each item has:
- `id`, `label`, `family` and `slot` (`main_hand`, `off_hand`, `two_hand`, `back` or `ammo`);
- `file`: a rigid glTF in the socket convention; skinned items have `files.<body type>` instead;
- `held`: the socket while in use. Ammunition has no `held`, only where it rides;
- `stowed`: the socket while put away, chosen by the equipped back item's `back_kind`
  (`default`, `cape` or `backpack`);
- `sheath`: an optional file that stays on the stowed socket;
- `clips`: which library clips the item uses, by role: `idle` (while drawn), `attacks` (in
  turn), `ready` and `stow` (taking it out and putting it away), and family extras (`block`,
  `dash`, `bash`, `work`; staves' `channel` and `slam`; bows' `draw`, `aim` and `release`);
- `grips.off_hand`: for two-handed items, where the other hand holds them (`at`, `up`, `front`
  in the item's glTF space, and `range`, how far along the item that hand may slide);
- `grab_range`: for back items, how far along the item's up axis (metres from its grip) a hand
  may take it when drawing;
- `string`: for bows, the string's `rest` point and its `pull` at full draw, in the item's space;
- for back items, `back_kind`, plus `layer` and `hides` as for pieces.

Capes also have `springs`: `chains` of `cape_<column>_<link>` bones rooted on `spine_03`, plus
`stiffness`, `drag`, `gravity` and particle `radius`. A cape's collar copies the body's own skin
weights, so it can't separate from the shoulders. Only the chains swing, against the body type's
`colliders`.

## Clips, curves and events

The library is the Universal Animation Library's 84 clips plus 29 of our own, built by
`art/blender/build_clips.py`. Ours are keyed in code on the broad frame's skeleton: each pose
places the item and the hands follow their sockets, so on that body every grip is exact.

| Family | Clips |
|---|---|
| Bow | `Bow_Idle_Loop`, `Bow_Draw` (arrow from the quiver to full draw), `Bow_Aim_Loop`, `Bow_Release`, `Bow_Shoot` (draw, loose and recover in 1.2 s) |
| Staff | `Staff_Idle_Loop` (planted), `Staff_Cast`, `Staff_Channel_Loop`, `Staff_Slam` |
| Spear | `Spear_Idle_Loop`, `Spear_Thrust`, `Spear_Sweep` |
| Greatsword | `GreatSword_Idle_Loop`, `GreatSword_Slash`, `GreatSword_Swing`, `GreatSword_Overhead` |
| Drawing | `Draw_Hip` / `Sheathe_Hip` (right hand, left hip), `Draw_Hip_L` / `Sheathe_Hip_L` (the mirror), `Draw_Side` / `Sheathe_Side` (right hand, right hip), `Draw_Back` / `Sheathe_Back` (blades over the right shoulder), `Draw_Pole` / `Sheathe_Pole` (poles over the right shoulder), `Draw_Bow` / `Sheathe_Bow` (the left hand, over the left shoulder) |
| Robes | `Idle_Robe_Loop`: the idle with the arms held a little out, clear of a skirt |

**Upper-body clips** (`upper_body: true`, the draws) play on the spine, arms, neck and head over
whatever the legs are doing. The same goes for a drawn idle while the unit moves: legs from the
locomotion clip, the rest from the item's `idle`.

**Curves** are lists of `[seconds, value]` keys. Between keys the value is linear; before the
first and after the last it holds; two keys at the same time make a step, and at that instant the
later key wins. `assets::curve` implements this, and the workshop's `perform.js` does too.

| Curve | While it's above 0 |
|---|---|
| `off_hand` | The off hand is pulled onto the main item by two-bone IK, weighted by the value |
| `reach` | The hand taking or stowing an item is pulled onto it where it rides |
| `string` | The drawing hand holds the bowstring |
| `arrow` | A nocked arrow shows |

**Events** mark moments: `grab` (the item moves to the hand), `stow` (it moves back to where it
rides), `release` (an arrow leaves: spawn the projectile), `hit` (a blow lands) and `cast` (a
spell leaves).

**Off-hand IK.** Clips play on bodies with other proportions, so the off hand drifts off
two-handed items. While `off_hand` > 0, find where the clip puts that hand's socket, measure
along the item's up axis to the nearest point (in the item's space), keep it within
`grips.off_hand.range`, and pull the hand there with two-bone IK (keeping the clip's elbow
plane), turned like `grips.off_hand` (`up` and `front` give the hand socket's +Y and +Z there).
That lets a spear run through the front hand as it's thrust.

**Drawing and stowing.** An item's `ready` clip takes it from where it rides; its `stow` clip puts
it back. With `s` = the distance along the stowed item's up axis to the point nearest the hand's
socket, kept within `grab_range` (in item metres; multiply by `gear_scale`):
- while `reach` > 0, pull the hand's socket onto the stowed item at `s`, turned like the item;
- **ready:** before `grab` the item stays where it rides. At `grab` it moves to the hand, offset
  `−s` along its up axis (so the point the hand took is in the hand), and the offset eases to 0
  with smoothstep between `grab` and the end of the clip;
- **stow:** before `stow` the item is in the hand with an offset of `−s × smoothstep(t / stow)`,
  measuring `s` each frame; at `stow` it moves back to where it rides;
- other gear without its own clip (a shield, the off-hand dagger) moves at the same event.

**Bows.** The bow's mesh has two morph targets. `arrow` shows a nocked arrow (it is folded inside
the grip otherwise); set it from the `arrow` curve. `draw` pulls the string's middle back by
`string.pull` and bends the limbs; with `h` = the drawing hand's socket in the bow's space,
set it to `clamp(dot(h − rest, pull) / dot(pull, pull), 0, 1) × string curve`, so the string
always meets the hand, whatever the archer's size. The drawing hand is the one not holding the bow.

## Materials and file details

- **Materials:** standard glTF PBR (metallic-roughness). Colour textures are sRGB; normal, ORM
  and roughness maps are linear. Normal maps follow the glTF convention.
- **Gear surfaces:** each rigid item has one material for most of it, baked by
  `aldermoor_art/surface.py` from procedural wood grain, leather, brushed metal, edge wear,
  grime and chipped paint: a colour texture and an ORM texture (occlusion R, roughness G,
  metalness B; one JPEG serves both glTF slots), in `gear/textures/`. Glowing and see-through
  parts (crystals, flames, glass) keep separate materials.
- **Vertex colours:** where present, `COLOR_0` multiplies the base colour, as the glTF spec says:
  the skeleton's aged bone and the beasts' painted coats.
- **Glowing parts:** crystals, flames, runes and undead eyes use emissive colour with
  `KHR_materials_emissive_strength`.
- **Transparency:** the lantern's glass is the only blended material (`alphaMode: BLEND`).
- **Skinning:** every skinned file carries the 65 rig bones as nodes; bodies add the sockets.
  Bind a piece's joints to the body's bones by name and keep the piece's own inverse bind
  matrices. A piece may add bones the body lacks (`cape_<column>_<link>` chains): parent them
  under the bone their glTF parent is named after.
- **Animation clips:** 30 fps and in place (no root motion). Every bone has rotation tracks; only
  the pelvis has translation.
- **Beasts:** one `.glb` each, on the same axes (+Y up, facing +Z, metres), with in-place clips.
- **The reference implementation:** the workshop does all of this in about 2,100 lines of
  three.js (`tools/workshop/src/`):
  - `character.js`: assembly, rebinding by bone name, sockets and the drawn/stowed rules;
  - `regions.js`: the hiding shader;
  - `anim.js`: clip retargeting and the pelvis remap;
  - `perform.js`: clip curves and events (off-hand and reach IK, drawing and stowing, bows);
  - `springs.js`: the cape solver;
  - `beast.js`: beasts.

  Port the behaviour rather than inventing new rules, and the game will match what Tariq
  approved in the workshop.
- **Levels of detail:** every body, piece, hairstyle, cape, pack and rigid item has
  `<name>.lod1.gltf` (half the triangles) and `<name>.lod2.gltf` (a fifth) beside it, on the same
  skeleton, materials and texture coordinates. Region borders and open edges don't move, so
  hiding and the `hides` lists apply unchanged. Bows (morph targets) have only the full mesh.
  Switch a whole character at once, by its height on screen: the full meshes above about 300
  pixels, LOD 1 above about 100, LOD 2 below (about 10 m and 30 m at 1440p). `lod_path` in the
  `assets` crate names the files, and the workshop's View section previews each level.
- **Triangle counts today:** a farmhand is about 30k. A full plate knight with shield and cape is
  about 84k at full detail, 46k at LOD 1 and 28k at LOD 2 (region borders and open edges stay,
  so the reduction is less than the meshes' own halves and fifths).

## Pipeline

```sh
art/fetch_vendor.sh      # once: third-party packs into art/vendor/
art/build_all.sh         # everything, in dependency order (add --review for review renders)
cargo test -p assets     # checks every promise the catalogues make
tools/workshop/serve.sh  # the interactive workshop at http://localhost:8765/tools/workshop/
```

`art/blender.sh` finds Blender 4.5 (`$BLENDER`, then `/Applications`, then the copy in
`aldermoor-wild/.tools`). The build order:

| Script | Builds | Writes |
|---|---|---|
| `build_humanoid.py` | The shared rig, both frames' bodies, hair, the Quaternius outfits | `humanoid/`, `catalog/humanoid.json` |
| `build_clips.py` | The clip library: the 84 vendor clips plus our 29 | `humanoid/anims.glb`, the catalogue's `animations` |
| `build_races.py` | Elves, dwarves, foxfolk and goblins on both frames: every outfit and hairstyle refitted by bone-segment scaling; foxfolk's muzzle, ears, tail and fur (`aldermoor_art/fox.py`) | `humanoid/<race>_<frame>/` |
| `build_creatures.py` | Skeleton warrior, the Barrow King, stone golem: creature body types on the humanoid rig | `humanoid/skeleton/`, `barrow_king/`, `golem/` |
| `build_gear.py` | Rigid weapons, shields, bows, quivers, casters and tools | `gear/`, `catalog/gear.json` |
| `build_back.py` | Capes on spring chains and the travel pack, fitted per body type | `gear/<body type>/` |
| `build_armour.py` | The plate set, fitted per body type | `humanoid/<body type>/armour/` |
| `build_garments.py` | Robes (mage, necromancer, healer), belted tunic, traveller's coat, smith's apron, wizard's and straw hats, with dye masks | `humanoid/<body type>/clothes/` |
| `build_beasts.py` | Wolves on their own four-legged rig, with generated gaits | `creatures/`, `catalog/creatures.json` |

- **Output format:** `.gltf` + `.bin` with a shared `textures/` folder, so the parts of an outfit
  share their texture files. Colour maps are JPEG and data maps PNG, 2048 px at most. Beasts are
  single `.glb` files.
- **Review:** `review_humanoid.py` and the `--review` flags assemble looks the way the game will,
  from the exported files, and render them into `art/out/` (ignored by git).
- **Catalogue checks:** `cargo test -p assets` (the `assets` crate) loads every catalogue and
  checks that each named thing exists:
  - every file, bone, socket, collider, region, hide target and clip;
  - clip curves and events (known names, inside the clip, values 0 to 1), and that what a
    clip asks of an item (an off-hand grip, a bowstring, a `grab` or `stow` event) the item has;
  - both levels of detail beside every file, on the same skeleton;
  - `_REGION` on every skinned piece;
  - that every dressable body type has a fit for every piece;
  - that no id or label mentions spiders, arachnids or webs.

## What exists (24 October 2026)

| Area | Contents |
|---|---|
| Body types | Human, elf, dwarf, foxfolk and goblin, each on a broad and a slender frame; skeleton warrior, the Barrow King (boss) and stone golem |
| Appearance | 8 skin tones, 4 goblin greens and 5 fox furs (red fox, ember, sand, silver, arctic), 11 hair colours, 5 hairstyles, 2 brow styles, a full beard and a dwarven beard; 16 dyes |
| Clothing | Peasant shirt, trousers and shoes; Ranger jerkin, leggings, boots, hood and pauldron (two colourways each); mage's, gravecaller's and sunward robes; belted tunic; traveller's coat; smith's apron; wizard's and straw hats. Each garment has a `dye` mask: R for the main cloth, G for the trim |
| Plate | Cuirass with faulds, pauldrons, gauntlets, leg plates, nasal helm, kettle hat |
| Weapons | Arming sword and greatsword; dagger, bearded axe, flanged mace and spear; round and kite shields; hunting bow and longbow (drawable strings, nocked arrows) and quiver; all with baked surface detail |
| Caster gear | Oak, gravecaller's and sunward staves; wand, spellbook and focus orb |
| Tools | Smith's hammer, hoe, watering can, lantern |
| Back | Traveller's cape and ranger's cloak on spring chains; travel pack with body-conforming straps |
| Beasts | Grey wolf and dire wolf: idle, walk, run, attack, hit, death, jump, howl |
| Clips | 84 library clips (locomotion, roll, sword combos, block, shield, spells, hits, deaths, farming, chopping, fixing, sitting, talking, zombie) plus our 29: archery, staves, spears, greatswords, drawing and sheathing from hip, side and back, and a robe idle |

**Gaps, next in line:**
- weapon-ready locomotion (until then, layer the drawn idle's upper body over the run);
- more beasts and hostile humanoid outfits;
- the same surface pass for plate armour;
- levels of detail for beasts.

## Milestones

1. **The character workshop** (`tools/workshop/`), an interactive viewer for one complete kit:
   - **Choices:** body type, tone, hair, brows and beard.
   - **The kit:** the Peasant and Ranger outfits, light and heavy armour, a cape, a backpack, a
     sword, a shield, a bow and a staff.
   - **What to watch:** every clip, with weapons drawn and stowed, from every angle.
   - Tariq judges attachment, fit and clipping there before the catalogue grows.
2. **Races:**
   - elves: taller and slimmer, with their own ears;
   - dwarves: shorter and stockier;
   - foxfolk: their own head with a muzzle, ears, a tail and fur patterns (done).
   Each race is built by scaling bone segments in the rest pose, so every piece of gear follows
   automatically, and then the result is baked into per-race files.
3. **NPC wardrobe:** outfits by occupation, region and faction, and visible work tools.
4. **Creatures:**
   - wild beasts;
   - hostile humanoids, on the humanoid rig;
   - undead, using the library's zombie clips;
   - ancient constructs.
   Each family gets several distinct members and regional variants.
5. **Bosses:** one fully designed major boss for the first dungeon, plus minibosses built from
   ordinary enemies.

## Budgets

| Item | Budget |
|---|---|
| One fully dressed character | About 40k triangles |
| Character textures | 2K |
| Gear textures | 1K |
| A crowd NPC | Under 15k triangles at LOD 2 (a farmhand is about 11k) |
