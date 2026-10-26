# Aldermoor art bible

The agreed look, as rules with numbers. Chippy chose the references on 25 October 2026 (#21):
elements of **RuneScape, Guild Wars, Breath of the Wild and League of Legends**, and from
**Hogwarts Legacy only its buildings and roads**. On our physically based assets that means
stylised realism: bold, readable shapes and colour, rich worked materials, painterly skies and
warm light.

- [ASSETS.md](ASSETS.md) is the asset contract: files, rigs, catalogues and the build. This file
  says what the art should look like.
- [PERFORMANCE.md](PERFORMANCE.md) holds the frame budgets. The per-asset art budgets (triangles,
  draws, bones, maps) move into ASSETS.md and `cargo test -p assets` with #107.
- The look board (#109) turns these rules into target renders for Chippy to sign off. Until then
  the numbers are starting targets, set against the art Chippy approved on 24 October and
  measured from it ("today" below means `main` on 25 October).
- Every art PR is checked against this file (see "Checking art"). To change a rule, change it here
  in a PR and log it in [wiki/Decisions-log.md](../wiki/Decisions-log.md).

## What each reference brings

| Reference | Take | Leave |
|---|---|---|
| League of Legends | Silhouettes that read at a glance, saturated colour on characters and effects, one friend-or-foe language, attacks you can see coming | Superhero proportions, effects that bury the fight |
| RuneScape | Gear tiers you can read by colour and shape, gathering and crafting you can see, a Wilderness that gets grimmer the deeper you go | Its low-detail models |
| Guild Wars | Dye channels, materials with painted accents, palettes per region and faction, painterly skies, fields you can read on the ground | Long trailing cloth that hides the silhouette |
| Breath of the Wild | The land's calm colour, wind in everything, haze that turns far hills blue, landmarks you can see and head for | Flat cel shading |
| Hogwarts Legacy | Its buildings and roads, and nothing else: crooked timber and stone villages, steep slate roofs and chimneys, cobbled streets, stone bridges, walls and towers | Its materials, light, colour, characters and effects |

## Shapes and proportions

**Bodies stay as approved.** The races keep their frames (ASSETS.md, "The humanoid family"). The
gear carries the stylisation; it stays practical (DESIGN.md), only larger and bolder than life:

- **Weapons:** blades, heads and guards 10–20% larger than life and up to 30% thicker. Two-handers,
  bows and staves grow no more than 10% in length, so they don't tower over whoever holds them.
  Grips stay hand-sized, so sockets, clips and the off-hand IK still fit. Today's weapons are life
  size (arming sword 0.92 m, greatsword 1.37 m, spear 2.28 m); the tier pass (#121) brings them up.
  At 30 m a 5 cm blade is 2 pixels wide on the reference view, a 6.5 cm one nearly 3.
- **Shoulders and helmets:** pauldrons 10–20% wider than the shoulder they sit on; helmets 10–20%
  bigger than the skull while still fitted to it (ASSETS.md, "Headwear"), with a crest, brim or
  plume that shows in silhouette.
- **Gloves and boots** may add 10% bulk. Hands and feet underneath don't change.

**One big shape first.** A design reads as one big shape (about 70% of what you see), one or two
medium ones (20%: a pauldron, a belt, a hood) and small detail (10%: rivets, stitching, trim).
Detail gathers where the eye goes (face, chest, hands, weapon), with quiet areas between.

**Armour weight reads from the silhouette** at 30 m, and the crowd stand-ins past about 37 m keep
exactly this, one per body and weight (#112):

| Weight | Big shape | Today |
|---|---|---|
| Cloth | A triangle, wide at the hem: robes and skirts to the shins, sleeves that flare | Mage's, gravecaller's and sunward robes |
| Leather | Slim and fitted: a V torso, hood or cowl, belts and straps | The Ranger outfit |
| Mail | A column: straight sides, a coif, a surcoat to the knees | Not built (#120) |
| Plate | Broad shoulders over a narrow waist: big pauldrons and a helm | The plate set |

**Weapons read by family:** each family has one feature that shows at 30 m, and no two share it:
the sword's cross guard, the greatsword's length, the spear's head and shaft, the axe's beard, the
mace's flanged head, the dagger's short blade (and pair), the bow's curve, the staff's head, the
wand's glow, the shield's outline.

**Races read by outline:** elves tall and slim with long ears, dwarves short and broad with
beards, foxfolk with muzzle and tail (helmets hide the ears), goblins small with big ears and
noses.

**Sizes on screen.** On the reference view (1,200 pixels tall, the default 52° field of view) a
1.8 m character stands:

| Distance | Height | What it means |
|---|---|---|
| 7 m | 300 px | Full detail above this (ASSETS.md, "Levels of detail") |
| 22 m | 100 px | LOD1 above, LOD2 below |
| 30 m | 75 px | Armour weight, weapon family and tier colour must read |
| 37 m | 60 px | Crowd stand-ins below (#112) |
| 50 m | 44 px | Team colours must still read (#141) |
| 100 m | 22 px | Band markers and landmarks read; characters are dots |

At the closest camera (2 m) the screen shows about 600 pixels per metre, at 4 m about 300.

## Tiers and named items

Items have fixed stats and come in material tiers (#71). A tier reads at a glance, the way
RuneScape's metals do: **by colour first, then by shape.** Each step also adds one medium shape (a
second plate, a fuller collar, a raised rim), so it still reads at LOD2, where the crowd palette
keeps base colours and drops painted detail. Tier names are provisional, like every name in
DESIGN.md.

| Levels | Metal | Base colour (linear) | Roughness | Finish |
|---|---|---|---|---|
| 1–9 | Bronze | (0.70, 0.42, 0.20) | 0.55–0.7 | Dull, green patina in the crevices |
| 10–19 | Iron | (0.24, 0.24, 0.25) | 0.6–0.75 | Dark and hammered, rust in the crevices |
| 20–29 | Steel | (0.36, 0.36, 0.38), today's hammered steel | 0.45–0.65 | Clean, bright worn edges |
| 30–39 | Silvered steel | (0.62, 0.66, 0.74) | 0.25–0.4 | Cool and bright, fine engraving |
| 40–50 | Blackened steel | (0.14, 0.14, 0.16) with gold (0.8, 0.62, 0.27) on every edge | 0.3–0.5 | Dark plates, gilt rims and studs |

The ladder alternates light and dark (mid, dark, mid, light, darkest with gold), so neighbouring
tiers never look alike, even in greyscale. Cloth and leather follow the same bands:

| Levels | Cloth | Leather |
|---|---|---|
| 1–9 | Undyed linen and wool, plain hems | Rawhide (today's tan leather), laced |
| 10–19 | One dye, a plain trim | Hardened dark leather (today's), stitched panels |
| 20–29 | Two dyes, an embroidered trim band | Studded with iron or brass, a fur collar |
| 30–39 | Fine wool and velvet (roughness 0.6–0.75), metal-thread trim | Tooled and dyed, brass fittings |
| 40–50 | Rune-woven trim with a faint glow (emissive strength up to 0.5) | Beast hide with fur, horn or bone trim |

- **"Fine" versions** (#71) keep the mesh and the tier colour and get a better finish: metal 0.1
  smoother with brighter edges, trim one step richer, a small maker's mark. Icons mark them (#124).
- **Named items** break the ladder on purpose: one oversized feature (a blade's shape, a head, a
  guard), a signature colour, and at most one restrained effect, such as a glow at emissive
  strength 1–2 or a slow trail (#121, #15).

## Materials

Physically based (ASSETS.md, "Materials"). The stylisation is in shape, colour and finish, not in
the shading model.

- **Shared surfaces first.** Armour and back items use the worked kits (hammered steel, brass,
  leather and wool, `humanoid/textures/kit_*`), buildings the tiles in `environment/textures/`.
  Paint a unique map only where a piece needs its own.
- **Painted accents on top:** brighter worn edges where hands and blades rub, a warmer or cooler
  shift in folds and hollows, and grime gathered in crevices through `COLOR_0`. Wear tells a story
  (a smith's apron is scorched, a guard's boots muddy); it doesn't cover everything.
- **No photographic textures, and no lighting painted into the base colour.** Shading comes from
  the normal, ORM and `COLOR_0` occlusion.

**Base colour (linear, non-metals).** Keep each material's mean in its class:

| Class | Linear | sRGB | Today (mean of the map) |
|---|---|---|---|
| Darkest: soot, charcoal, black hair and fur | 0.01–0.04 | 25–56 | Charcoal dye 0.025, black hair 0.012 |
| Dark: leather, dark wood and cloth, wet earth | 0.04–0.12 | 56–97 | Leather 0.08, apron 0.06, coat 0.08, dark planks 0.11 |
| Mid: grass, foliage, wood, bark, rock, most cloth | 0.07–0.25 | 75–137 | Grass 0.08, meadow 0.08, foliage 0.13, bark 0.13, earth 0.13, planks 0.19, limestone 0.21 |
| Light: dressed stone, thatch, straw, sand, bone | 0.25–0.5 | 137–188 | Stone 0.43, cobbles 0.43, thatch 0.38, straw hat 0.35 |
| White: whitewash, linen, snow | 0.5–0.8 | 188–231 | Plaster 0.73, the healer's robe 0.54 |

Nothing non-metal goes below 0.01 or above 0.8, apart from pupils, painted crevice lines and
emissive parts. Large surfaces (ground, walls, roofs) stay between 0.04 and 0.5, whitewash
excepted, so the sun can't blow them out and shadows keep their detail.

**Metals** reflect 55–100% in life (iron 0.56, silver 0.97). Ours run darker, 0.14 to 0.75, for a
worked, aged look that doesn't glare in the sun: today's steel is 0.36 and brass (0.68, 0.49, 0.23).
Metal is never dyed; its colour is the tier.

**Roughness:** metal 0.25–0.75 by tier (today's steel 0.52, brass 0.55); leather 0.55–0.7 (0.62);
cloth 0.85–0.95 (wool 0.92), velvet and silk 0.6–0.75; skin 0.6–0.85 (0.82); fur 0.85–0.9 (0.86);
ground and stone 0.8–0.95 (0.82–0.92); wet and glossy things (eyes, nose leather, lacquer, wet
stone) 0.05–0.35 (the fox's nose 0.32).

**Texel density.** Scenery that touches (a wall on the ground, a prop against a wall) stays within
4 times of its neighbour; characters are the exception, sharper than anything around them.

| Surface | Pixels per metre | Today |
|---|---|---|
| Faces, once they get their own map (#134) | 2,000 | About 1,000, a share of the body's 2K map |
| Bodies and outfits, unique maps | 1,000 | 2K maps per body or outfit |
| Gear, each item's own map | 1,000 | 1K maps, about a metre per item |
| Worked kits on armour and capes (tiling) | 2,000–3,400 | Steel 2,048, brass 2,560, leather 2,926, wool 3,413 |
| Buildings and props (tiling) | 500–850 | Plaster, stone and thatch 512, planks 640, roof tiles 853 |
| Ground layers | 170–290 | Limestone 171, meadow 205, grass 256, earth 293 |

So characters stay sharp at every zoom (600 pixels per metre at the closest camera), and the
ground leans on its normal maps up close.

## Colour

**Characters pop; the land stays calm.** Measured as the mean HSV saturation of a colour map in
sRGB:

- **The land:** grass and foliage at most 60%, everything else (earth, stone, wood, roofs, walls)
  at most 50%. Today grass is 57%, foliage 52%, roof tiles 49%, meadow 48%, planks 40%, thatch 38%,
  earth 33%, limestone 12%, plaster 11%.
- **Characters:** coloured cloth and dyes 45–85%. Today the traveller's cape is 70%, the Peasant
  and Ranger outfits 59–60%, the mage's robe 57%, the coat 46%. Neutrals (bone, slate, charcoal,
  undyed linen, the healer's white) sit low on purpose.
- **Above both:** effects and team colours.
- **Accents** (flowers, banners, awnings, painted shutters, lanterns) are small and saturated,
  like a painter's highlights: at most about a tenth of the land in view.
- **Value as well as hue:** in a greyscale capture each character's outline must still separate
  from the ground behind it.

**Dyes (Guild Wars).**
- Garments, capes and leather carry two dye channels: R the main cloth, G the trim (ASSETS.md,
  "Tints"). In team modes the team colour replaces the main cloth, and a darker shade of it the
  trim (#141).
- The palette is the catalogue's `dyes`: 16 today, from charcoal (0.025) to bone (0.7). New dyes
  stay between 0.02 and 0.7 linear and under 85% saturation, so effects and team colours stay
  ahead of them.
- Named items keep their identity: their dyes are fixed or limited (DESIGN.md).
- At LOD2 every outfit shares one crowd palette of base colours and dye channels (#112), so the big
  shapes and the main colours must carry it from 22 m out.

**Regions.**

| Region | Land | Accents | Light |
|---|---|---|---|
| Brambleford valley | Lush and dry greens, warm limestone, whitewash, golden thatch, terracotta and slate roofs, a teal river | Wildflowers, painted shutters, market awnings | Warm sun, blue sky, blue haze |
| The Barrow (#104, #131) | Cold grey-green stone, roots, moss, old timber | Warm torches and braziers, the undead's cold blue glow | Dark, key to fill 8:1 or more |
| The Wilderness (#76, #138) | Band by band, from dry ochre grass through scorched earth to ash and black rock, each band about a quarter less saturated | Rusted iron, embers, band markers in bone and red | Sky and fog turn grey-ochre, then red-brown in the deepest band |
| Team-mode maps (#141) | Neutral stone and short grass, little clutter | The two team colours own the banners and lights | Even and clear, key to fill about 3:1, no fog to hide in |

**Factions** (DESIGN.md; names provisional). Colours adapt to each race, and every race and
specialty wears every faction:

| Faction | Shapes | Colours (dyes) | Materials |
|---|---|---|---|
| Protecting nature | Curves, leaves, antlers, living wood | moss, forest, weld, walnut | Wood, leather, horn, bark; little metal |
| Building civilisation and trade | Squares and shields, bands and rivets, heraldry | woad, bone, crimson, with brass | Steel, brass, dressed stone, fine cloth |
| Recovering ancient magic | Triangles and circles, floating parts, rune bands | heather, plum, teal, with a cold blue glow | Carved stone, crystal, silvered metal |

NPC guilds wear their trade (aprons, tools) in muted colours. Player companies show heraldry built
from a fixed kit of fields, charges and borders in the dye palette (#142).

**Friend and foe (League of Legends).** On every screen, friends are blue and foes red-orange:
ground warnings (as the effects in #151 draw them), nameplates and health bars (#42), target
outlines, and in team modes the team dyes too, so your team is blue and theirs red-orange on your
screen. Team dyes start at blue (0.01, 0.10, 0.55) and red-orange (0.80, 0.22, 0.01), more
saturated than any dye: different in hue and nearly three times apart in brightness, so they hold
for the common kinds of colour blindness and in greyscale. Shape doubles colour wherever it
carries meaning: foe markers are angular, friend markers round.

**Effects.** Each school keeps one hue family everywhere (projectiles, fields, glows on gear,
icons), from the ramps in the effects catalogue (#151): physical warm white, fire orange to gold,
frost blue to white, arcane violet, nature green, holy gold, shadow purple, poison yellow-green,
lightning blue-white, water blue. Glow colours stay saturated, with one or two channels near zero,
because the ACES tone mapper turns bright pale colours white. #151 measured 1.3 ms GPU with every
effect on screen at once; a normal fight costs less.

## Buildings and roads (Hogwarts Legacy)

The one thing we take from Hogwarts Legacy (Chippy, 25 October 2026): its villages, streets and
stonework. Shapes and layout come from there; the materials are our shared kits and the colours
the region's.

- **Houses:** two or three storeys, a stone ground floor under timber-framed upper floors that
  jut out over the street, packed along winding lanes with gables to the street.
- **Roofs:** steep (45–60°), in slate, tile or thatch, with dormers and tall chimney stacks.
  Ridges sag and walls lean a degree or two: nothing is quite square, and no two windows match.
- **Bigger buildings:** stone with arches, buttresses and towers under conical roofs (the guard
  tower, the shrine, the ruins in #115).
- **Streets and roads (#154):** cobbles in the square and the main street, flagstones before the
  inn and the shrine, kerbs, gutters and steps where streets climb; country roads with two ruts
  and a grass crown, stony verges, dry-stone walls, timber fences, milestones and signposts.
- **Street furniture:** lamp posts, hanging signs, window boxes, wells and stone bridges with
  arches.
- **Inside (#126):** exposed beams, stone hearths, crooked stairs.

Roads are lighter and smoother than the land beside them (packed earth 0.13 against grass 0.08
today), so they lead the eye from landmark to landmark.

## Light and atmosphere

`catalog/look.json` holds the light; every time of day and weather is a keyframe of it (#117,
#146). Key to fill is the sun's direct light against the ambient fill as the renderer weighs them:
`sun_intensity × luminance(sun_colour) / π` against `luminance(ambient)`.

| Time | Sun (or moon) height | Sun colour (linear) | Key to fill | Brightness against late morning | Haze |
|---|---|---|---|---|---|
| Dawn | 5–12° | (1.0, 0.62, 0.40) | About 3:1 | 50–60% | Thicker (0.002), pink-gold toward the sun |
| Late morning, today | 38° | (1.0, 0.9, 0.78), intensity 3.2 | 4.5:1 | 100% | 0.0013, the horizon's blue |
| Midday | 55–65° | (1.0, 0.95, 0.88) | About 5:1 | 100–110% | Thinner (0.0011) |
| Golden hour | 8–15° | (1.0, 0.70, 0.45) | About 4:1 | 70–80% | 0.0016, gold toward the sun |
| Dusk | Just set | None: sky light only | Under 2:1 | 30–40% | 0.0018, blue-violet |
| Night | Moon at 30–50° | (0.55, 0.65, 1.0) | About 2.5:1 | 20–25%, exposure raised to 1.8–2.2 | 0.0015, deep blue |
| Overcast and rain (#146) | Hidden | (0.85, 0.88, 0.92) | 1.5:1 to 2:1 | 60–70% | 0.002–0.003, grey |

- **Shadows are coloured, never black.** The ambient is the sky's colour: cool by day, blue at
  night. Sun and fill sit on opposite sides of warm and cool, most of all at golden hour.
- **At night** lamps, windows and campfires are the brightest things in view (#117), and paths and
  characters stay readable.
- **Interiors and the Barrow** are pools of warm light in the dark: candles and hearths around
  (1.0, 0.55, 0.25) against a cool fill from windows, key to fill 6:1 to 10:1 indoors and 8:1 or
  more in the Barrow, with the undead's cold blue glow (0.25, 0.75, 1.0, as the golem's runes). They
  need local lights (#14) and fog shafts (#13); until those land, the look board shows the target.
- **Haze (Breath of the Wild):** the renderer fogs `1 − e^(−density × distance)` toward
  `fog_colour`. The fog colour matches the sky at the horizon in every keyframe, so far hills melt
  into the sky instead of greying out. Today's 0.0013 gives 12% haze at 100 m, 32% at 300 m (where
  the landmarks you steer by keep two thirds of their contrast) and 65% at the 800 m far plane.
- **Skies** are painted, not photographed: soft gradients and big cumulus lit from the sun's side,
  as in `sky/brambleford_day.jpg`.
- **Grade:** ACES filmic tone mapping, a saturation lift of 0–15% (8% today), and exposure 1.0 by
  day.

**Wind.** Everything that grows moves; nothing built does, apart from sails, flags, banners, signs
and hanging lamps.
- Every plant carries `_WIND` (ASSETS.md, "Wind"). At strength 1, grass and flower tips sway up to
  about 12 cm, bush and canopy edges 5–8 cm; trunks and rocks stay still.
- `wind_strength` in look.json: 1.0 on a fair day, about 0.3 in a dawn calm, 2–2.5 in a storm.
- Capes and tails swing on their springs; flags and banners get springs or `_WIND`.

## Reading the world

- **Landmarks:** every settlement and dungeon has one that shows above the trees from 300 m (the
  windmill, a tower, a barrow mound with standing stones), lit or against the sky.
- **Climbing:** what you can climb looks climbable, from across the valley (#249; the climbing
  plan in MOVEMENT.md):
  - Stone holds stand at least 5 cm proud of the wall, with a pale, worn top that catches the
    light: capstones on broken walls, a coping you can walk up, sills, and corbelled jamb stones
    and keystones whose shadowed undersides slope back to the wall.
  - Timber holds (the cottages' sills, beams and eaves) are dark, with light, rubbed tops.
  - Cliff holds are pale bands of harder stone running along the strata.
  - Masonry too tall or too smooth to climb ends in broken, mossy rubble, and has no pale edges:
    a pale edge is always a hold.
  - Every hold you can see can be grabbed and every hold you can grab can be seen: the art is
    built from `world`'s own list (ASSETS.md, "Masonry").
- **Gathering (RuneScape):** nodes show what they give: ore veins in the ore's colour, trees by
  species, herbs by their flowers. Spent nodes look spent (a stump, bare rock) and visibly regrow
  (#143).
- **The Wilderness** gets grimmer band by band, with markers you can read at 100 m (#138).

## Animation

- **Readable key poses:** a strong line of action, wide anticipation and follow-through (League of
  Legends), weight on every contact. Poses read from 20 m and from behind.
- **Wind-ups match COMBAT.md:** heavy blows wind up for 0.4 s, with the weapon drawn back past the
  shoulder in the first 0.15 s, so the 0.25 s parry window is fair. Quick skills have no wind-up
  pose. Monsters' big blows pair a ground warning with a body wind-up.
- **Rolls** in every direction keep today's `Roll` length, so the sim's dodge timing (0.4 s in
  which nothing lands) still holds (#105).
- **Swings** sweep each weapon family's cone (#64, #123).
- **Gathering and crafting loops** (RuneScape) show the work clearly at 10 m, with the sweet spot on
  a visible impact or bite (#144).
- Clips are 30 fps and in place, and carry their events (ASSETS.md, "Clips, curves and events").

## HUD and icons

- **Friend and foe** colours as above, everywhere in the HUD.
- **Icons** are rendered from the item meshes with one light (from the top left), one angle and one
  frame (#124). They read at 32 px and in greyscale.
- **Text** is at least 14 px on the reference view (bigger on the Steam Deck, #150).

## Tone and exclusions

- **Tone** (DESIGN.md): warm, adventurous and occasionally funny; eerie ruins, a dangerous
  Wilderness, darker necromancy.
- **No graphic gore:** no blood sprays, pools, open wounds or severed parts. Bleeding shows as a
  few small dark-red sparks. The dead fall and fade. Skeletons and bone piles are fine, and
  skinnable carcasses show no wounds (#143).
- **Hard exclusion** (DESIGN.md, ASSETS.md): no spiders, other arachnids, spider-like creatures or
  webs, anywhere: enemies, bosses, summons, pets, effects, decorations and icons. The Barrow has
  dust, roots and hanging moss, never cobwebs, and cracks and lightning never radiate like a web.
- **No real-world religious or political symbols.** Faction and company heraldry come from our
  own kit (#142).
- **Sources:** free packs (CC0 today) and our own Blender builds, no paid packs (ASSETS.md), and no
  photographic textures.

## Checking art

Every art PR says which of these it ran:

1. **Next to the look board** (#109), in the same light, once it exists.
2. **In greyscale:** silhouettes, friend and foe, warnings and paths still read.
3. **At distance:** at 30 m armour weight, weapon family and tier read; at 50 m team colours do.
4. **Against the numbers here:** base colour class, saturation and texel density, plus the
   budgets in `cargo test -p assets` (#107).
5. **In game on the Mac** for anything visual (`needs-mac`): captures of the latest commit posted
   on the PR with the `attach-visuals` skill, main and the branch side by side from the same
   camera when a look changes (AGENTS.md, "Visuals").
