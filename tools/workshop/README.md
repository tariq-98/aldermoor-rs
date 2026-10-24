# Character workshop

An interactive viewer for the humanoid kit: pick a body, skin tone, hair and beard, dress the
character from the catalogue, equip gear, and play any clip from the shared library, from every
angle. It loads exactly what the game will load. See [docs/ASSETS.md](../../docs/ASSETS.md):
- `assets/catalog/humanoid.json` and its glTF files;
- `assets/catalog/gear.json` and its files, once `art/blender/build_gear.py` has written them.

```sh
tools/workshop/serve.sh          # installs three.js on first run, then serves the repo root
# open the printed address: http://localhost:8765/tools/workshop/
```

## What it does

- **Assembly.**
  - Each piece's skinned meshes are rebound to the body's bones by name.
  - Bones the body lacks, such as cape chains, join the skeleton under their parent bone.
  - Clips from `anims.glb` play with their rotations as they are. Only the pelvis keeps its
    translation, scaled by leg length.
- **Hiding.** A shader patch reads each vertex's `_region`. Triangles in regions that
  higher-layer pieces cover are clipped, and their shadows go with them.
  - Hair disappears under pieces with `covers_hair`.
  - Beards, brows and eyes disappear when the face is covered.
- **Tints.** Materials named `skin.*` and `hair.*` take twice the chosen linear tone as their
  colour. Outfit pieces offer their colourways from `texture_variants`.
- **Gear.**
  - Rigid items ride on socket bones; the drawn or stowed toggle moves them between their held
    and stowed sockets.
  - Sheaths stay on the stowed socket.
  - Two-handers clear the off hand.
  - A drawn weapon's own clips are listed first.
- **Springs.** Items with `springs` get a VRM-style verlet chain solver. It runs at a fixed
  120 Hz with capsule colliders: the body type's `colliders` if the catalogue lists them,
  otherwise capsules derived from the torso, pelvis, thighs, shins and upper arms.

## Controls

- **Mouse:** drag to orbit, scroll to zoom, right-drag to pan.
- **Keys:**
  - `Space`: play or pause.
  - `1`–`6`: front, back, left, right, three-quarter and head cameras.
  - `T`: turntable.
- **Debug toggles:** region colours, wireframe, skeleton and socket axes.
- **Sharing:** the URL hash holds the whole look (body, tone, hair, pieces, colourways, gear,
  clip, time and camera), so a link reproduces it exactly. For example:
  `#body=female&hair=hair.long&haircol=copper&p.torso_under=ranger.top&p.head=ranger.hood&clip=Sword_Idle&paused=1&t=0.4&cam=back`

## Checks

```sh
cd tools/workshop && npm install && node check.mjs --springs --ui --nogear
```

The checks run in headless Chrome and save screenshots to `art/out/review/workshop/`.
- **Looks:** each look in `check.mjs` is loaded.
- **`--springs`:** hangs a test chain from the upper spine. It checks the chain stays attached,
  finite and clear of the colliders through idle, jogging at jittery 30–240 fps frame times, and
  idle again.
- **`--ui`:** drives the panel.
- **`--nogear`:** checks the page before any gear exists.

The script exits non-zero on any console error or warning.
