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
| [Quaternius Universal Animation Library 1 and 2](https://quaternius.itch.io/universal-animation-library) (free tiers) | 85 clips: locomotion, roll, sword combos, block, shield, casting, hits, deaths, farming, chopping, sitting, talking, zombies | CC0 |
| Our own (built in `art/blender/`) | Weapons, shields, bows, staves, capes, backpacks, armour, race parts, extra animations | Ours |

All of these share one 65-bone humanoid rig with Unreal-mannequin bone names. Downloads are listed,
hashed and licensed in `art/vendor.json`; `art/fetch_vendor.sh` re-fetches them. The archives
stay out of git. Quaternius published a separate Quaternius Asset Licence on 28 August 2026, but
these packs' pages and their `License.txt` files say CC0. Either licence allows shipping them in
the game.

**Paid upgrades:** proposed to Tariq with their prices, but nothing bought. Each Quaternius pack
has a Source tier:
- base characters, $19.99: six bodies, 20 hairstyles and skin shaders;
- outfits, $20: twelve outfits and 62 parts;
- the two animation libraries, $14.99 each: about 230 clips together.

## The humanoid family

**One skeleton, many proportions.** Every humanoid skeleton uses the Universal Animation Library's
rest orientations for every bone; only joint positions differ between bodies, races and builds.
- A clip's local rotations then play unchanged on any of them.
- The renderer takes bone translations from the skeleton's own rest pose, except the pelvis. Its
  clip translation is scaled by `body.pelvis.y / animations.pelvis.y`.
- The build normalises every imported rig this way (`art/blender/aldermoor_art/rig.py`).

**Body types.** "Broad frame" and "Slender frame", built from the Quaternius male and female
bodies. Every outfit and hairstyle is offered on both; nothing is gendered in the game.

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

**Sockets.** Bones whose names start with `socket_` carry equipment.
- **The authoring rule:** an item is authored in Blender with its attachment point at the origin,
  "up" along +Z and "front" along −Y. A socket's local +Y is where the item's up points, and its
  local +Z is where the item's front points.
- **In glTF terms:** an item file's +Y and +Z axes map onto the socket node's +Y and +Z.
- **Weapons:** up is the blade or shaft tip, and front is the cutting edge. The origin is the
  centre of the grip.

| Socket | Parent | Holds |
|---|---|---|
| `socket_hand_r`, `socket_hand_l` | hands | Drawn weapons, wands, torches |
| `socket_shield_l` | left forearm | Strapped shields |
| `socket_back` | upper spine | Backpacks; the reference point for cape collars |
| `socket_back_1`, `socket_back_2` | upper spine | Stowed two-handers, staves, bows and shields, crossing the back |
| `socket_quiver` | upper spine | Quivers, arrows over the right shoulder |
| `socket_quiver_hip` | pelvis | The quiver when a backpack takes the back |
| `socket_pack`, `socket_pack_side` | upper spine | A shield on the travel pack's outer face; a bow or staff down its left side |
| `socket_hip_l`, `socket_hip_r` | pelvis | Sheathed one-handers, hilt forward |
| `socket_head` | head | Rigid hats and helmets |

**Tints.**
- **Skin and hair:** these materials are named `skin.*` and `hair.*`. Their colour map is a
  tone-neutral detail map with a mean of 0.5. The renderer sets the base colour factor to twice
  the chosen linear tone (`skin_tones` and `hair_colours` in the catalogue).
- **Outfits:** they keep their painted textures. The catalogue's `texture_variants` lists
  alternative colourways.
- **Dyes:** per-item dye masks come next.

## Catalogue: `assets/catalog/humanoid.json`

The build writes `assets/catalog/humanoid.json`; consumers read it and nothing else. Paths are
relative to `assets/`. Positions are glTF (metres, +Y up, +Z forward).

| Key | What it holds |
|---|---|
| `regions` | Region names in bit order |
| `skin_tones`, `hair_colours` | `id` plus `linear` RGB |
| `body_types.<id>` | `label`, `file` (the body with the skeleton and sockets), `pelvis`, `height`, `skin_reference`, `sockets`, `colliders` (cloth capsules: `bone`, `radius`, and ends `a` and `b` in the bone's node frame) |
| `defaults.<body type>` | A starting look: pieces, hair, brows, beard, tone and hair colour |
| `animations` | `file` (`anims.glb`: the reference skeleton plus every clip), `pelvis`, `fps`, and `clips` (each with `name`, `seconds`, `loop`, `group` and `tags`) |
| `hair[]` | `id`, `kind` (`hair`, `beard` or `brows`), `label`, and `files.<body type>` |
| `pieces[]` | `id`, `slot`, `layer`, `label`, `files.<body type>`, `hides.<body type>`, `covers_hair`, and `coverage` (diagnostic) |
| `texture_variants`, `texture_variant_labels` | Base texture name mapped to its alternatives, and display names for each colourway |

Each entry in `hides.<body type>` maps either `body` or a lower piece's id to the list of regions
to drop.

The gear catalogue, `assets/catalog/gear.json`, follows the same rules. Each item has:
- `id`, `label`, `family` and `slot` (`main_hand`, `off_hand`, `two_hand`, `back` or `ammo`);
- `file`: a rigid glTF in the socket convention; skinned items have `files.<body type>` instead;
- `held`: the socket while in use. Ammunition has no `held`, only where it rides;
- `stowed`: the socket while put away, chosen by the equipped back item's `back_kind`
  (`default`, `cape` or `backpack`);
- `sheath`: an optional file that stays on the stowed socket;
- `clips`: which library clips the item uses;
- for back items, `back_kind`, plus `layer` and `hides` as for pieces.

Capes also have `springs`: `chains` of `cape_<column>_<link>` bones rooted on `spine_03`, plus
`stiffness`, `drag`, `gravity` and particle `radius`. A cape's collar copies the body's own skin
weights, so it can't separate from the shoulders. Only the chains swing, against the body type's
`colliders`.

## Pipeline

```sh
art/fetch_vendor.sh                                   # once: third-party packs into art/vendor/
art/blender.sh -P art/blender/build_humanoid.py       # bodies, hair, outfits, clips, catalogue
art/blender.sh -P art/blender/build_gear.py           # weapons, shields, bows, quiver, staff, wand
art/blender.sh -P art/blender/build_back.py           # capes and the travel pack, per body type
art/blender.sh -P art/blender/review_humanoid.py      # renders looks from the exported files
tools/workshop/serve.sh                               # the interactive workshop
```

Add `-- --review` to the gear and back builds for review renders.

- `art/blender.sh` finds Blender 4.5: `$BLENDER`, then `/Applications`, then the copy in
  `aldermoor-wild/.tools`.
- **Output:** the build writes `.gltf`, `.bin` and a shared `textures/` folder, so the parts of
  an outfit share their textures. Colour maps are JPEG and data maps PNG, at 2048 px at most.
- **Review:** `review_humanoid.py` assembles looks the way the game will, then renders them into
  `art/out/` (ignored by git):
  - it loads the exported files;
  - it drops hidden regions;
  - it applies tints;
  - it poses the skeleton with library clips.

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
   - foxfolk: their own head with a muzzle, ears, a tail and fur patterns.
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
| A crowd NPC | Under 15k triangles, once LODs exist |
