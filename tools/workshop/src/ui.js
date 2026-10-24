// The side panel: small DOM helpers and the panel layout. The panel reads and writes `state`
// through the callbacks main.js passes in; it never touches the scene itself.

const SLOT_LABELS = {
  head: 'Head',
  shoulders: 'Shoulders',
  torso_under: 'Shirt or jerkin',
  torso: 'Body armour',
  hands: 'Hands',
  legs_under: 'Trousers',
  legs: 'Leg armour',
  feet: 'Footwear',
  main_hand: 'Main hand',
  off_hand: 'Off hand',
  back: 'Back',
  ammo: 'Ammunition',
};
const SLOT_ORDER = ['head', 'shoulders', 'torso_under', 'torso', 'hands', 'legs_under', 'legs', 'feet'];
const TAG_ORDER = ['idle', 'locomotion', 'combat', 'work', 'social', 'undead'];
const TAG_LABELS = { idle: 'Idle', locomotion: 'Movement', combat: 'Combat', work: 'Work', social: 'Social', undead: 'Undead' };

export function slotLabel(slot) {
  return SLOT_LABELS[slot] || slot.replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase());
}

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === 'class') node.className = v;
    else if (k.startsWith('on')) node.addEventListener(k.slice(2), v);
    else if (v !== undefined && v !== null) node.setAttribute(k, v);
  }
  for (const c of children.flat()) if (c !== null && c !== undefined) node.append(c instanceof Node ? c : String(c));
  return node;
}

function section(title, open = true) {
  const body = el('div', { class: 'section-body' });
  const head = el('button', { class: 'section-head', type: 'button' }, title);
  const sec = el('section', { class: open ? 'section open' : 'section' }, head, body);
  head.addEventListener('click', () => sec.classList.toggle('open'));
  return { sec, body };
}

function row(label, control) {
  return el('div', { class: 'row' }, el('label', {}, label), control);
}

function select(options, value, onChange) {
  // options: [{ value, label }] or [{ group, options: [...] }]
  const s = el('select', { onchange: (e) => onChange(e.target.value || null) });
  const add = (parent, o) => {
    const opt = el('option', { value: o.value ?? '' }, o.label);
    if ((o.value ?? '') === (value ?? '')) opt.selected = true;
    parent.append(opt);
  };
  for (const o of options) {
    if (o.group) {
      const g = el('optgroup', { label: o.group });
      for (const oo of o.options) add(g, oo);
      s.append(g);
    } else add(s, o);
  }
  return s;
}

function segmented(options, value, onChange) {
  const wrap = el('div', { class: 'segmented' });
  for (const o of options) {
    const b = el('button', { type: 'button', class: o.value === value ? 'on' : '' }, o.label);
    b.addEventListener('click', () => onChange(o.value));
    wrap.append(b);
  }
  return wrap;
}

function linearToCss(rgb) {
  const enc = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);
  const [r, g, b] = rgb.map((c) => Math.round(Math.min(1, Math.max(0, enc(c))) * 255));
  return `rgb(${r}, ${g}, ${b})`;
}

function swatches(list, value, onChange, display = (rgb) => rgb) {
  const wrap = el('div', { class: 'swatches' });
  for (const item of list) {
    const b = el('button', {
      type: 'button',
      title: item.label || item.id,
      'aria-label': item.label || item.id,
      class: item.id === value ? 'swatch on' : 'swatch',
      style: `background: ${linearToCss(display(item.linear))}`,
    });
    b.addEventListener('click', () => onChange(item.id));
    wrap.append(b);
  }
  return wrap;
}

function toggle(label, value, onChange) {
  const input = el('input', { type: 'checkbox' });
  input.checked = !!value;
  input.addEventListener('change', () => onChange(input.checked));
  return el('label', { class: 'toggle' }, input, el('span', {}, label));
}

/**
 * Builds (or rebuilds) the panel. `app` provides: state, humanoid, gear, character, actions.
 */
export function renderPanel(root, app) {
  const { state, humanoid, gear, creatures, character, actions } = app;
  root.replaceChildren();
  root.append(el('header', { class: 'title' }, el('h1', {}, 'Aldermoor'), el('p', {}, 'Character workshop')));

  // Beasts on their own rigs replace the character entirely.
  if (creatures?.creatures?.length) {
    const beasts = section('Beasts');
    const options = [{ value: '', label: 'None (humanoids)' }, ...creatures.creatures.map((c) => ({ value: c.id, label: c.label }))];
    beasts.body.append(row('Beast', select(options, state.beast || '', (v) => actions.setBeast(v))));
    root.append(beasts.sec);
  }
  if (state.beast && character?.clipNames) {
    const anim = section('Animation');
    anim.body.append(row('Clip', select(character.clipNames().map((n) => ({ value: n, label: n.replace(/_Loop$/, '').replace(/_/g, ' ') })), state.clip, actions.setClip)));
    root.append(anim.sec);
    appendView(root, state, actions);
    root.append(el('footer', { class: 'status', id: 'status' }, ''));
    return { setTime() {}, setPlaying() {} };
  }

  // Character.
  const who = section('Character');
  const races = humanoid.races || null;
  const current = humanoid.body_types[state.body] || {};
  const raceId = current.race || 'human';
  const frameId = current.frame || state.body;
  if (races) {
    const raceOptions = Object.entries(races).map(([id, r]) => ({ value: id, label: r.npc_only ? `${r.label} (NPC)` : r.label }));
    const frameOptions = Object.keys(races[raceId]?.frames || {}).map((f) => ({ value: f, label: f === 'male' ? 'Broad frame' : f === 'female' ? 'Slender frame' : f }));
    who.body.append(
      row('Race', segmented(raceOptions, raceId, (r) => actions.setBody(races[r].frames[frameId] || Object.values(races[r].frames)[0]))),
      row('Frame', segmented(frameOptions, frameId, (f) => actions.setBody(races[raceId].frames[f]))),
    );
  } else {
    who.body.append(row('Body', segmented(Object.entries(humanoid.body_types).map(([id, b]) => ({ value: id, label: b.label })), state.body, actions.setBody)));
  }
  // A race with tones of its own (goblin greens, foxfolk fur) offers only those.
  const own = humanoid.skin_tones.filter((t) => t.races?.includes(raceId));
  const tones = own.length ? own : humanoid.skin_tones.filter((t) => !t.races);
  who.body.append(row('Skin', swatches(tones, state.tone, actions.setTone)));
  const hairOf = (kind) => [{ value: '', label: 'None' }, ...humanoid.hair.filter((h) => h.kind === kind).map((h) => ({ value: h.id, label: h.label }))];
  who.body.append(
    row('Hair', select(hairOf('hair'), state.hair, (v) => actions.setHair('hair', v))),
    row('Hair colour', swatches(humanoid.hair_colours, state.haircol, actions.setHairColour)),
    row('Brows', select(hairOf('brows'), state.brows, (v) => actions.setHair('brows', v))),
    row('Beard', select(hairOf('beard'), state.beard, (v) => actions.setHair('beard', v))),
  );
  root.append(who.sec);

  // Clothing and armour.
  const wear = section('Clothing and armour');
  const slots = [...new Set(humanoid.pieces.map((p) => p.slot))].sort((a, b) => {
    const ia = SLOT_ORDER.indexOf(a);
    const ib = SLOT_ORDER.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
  });
  for (const slot of slots) {
    const options = [{ value: '', label: 'None' }, ...humanoid.pieces.filter((p) => p.slot === slot && p.files?.[state.body]).map((p) => ({ value: p.id, label: `${p.label} (${p.id.split('.')[0]})` }))];
    wear.body.append(row(slotLabel(slot), select(options, state.pieces[slot] || '', (v) => actions.setPiece(slot, v))));
    const id = state.pieces[slot];
    const variants = id ? character?.variantsOf(id) : [];
    for (const [base, ...alts] of variants || []) {
      const opts = [{ value: '', label: 'Original' }, ...alts.map((a, i) => ({ value: a, label: `Colourway ${i + 2}` }))];
      wear.body.append(row('  Colourway', select(opts, state.variants[id] || '', (v) => actions.setVariant(id, v))));
    }
    // Dyes: one row per channel, starting with the colour it was painted.
    const piece = id ? humanoid.pieces.find((p) => p.id === id) : null;
    if (piece?.dye && humanoid.dyes?.length) {
      (piece.dye_channels || []).forEach((ch, k) => {
        const list = [{ id: '', label: 'As painted', linear: ch.reference }, ...humanoid.dyes];
        const name = ch.name.replace(/^\w/, (c) => c.toUpperCase());
        wear.body.append(row(`  ${name} dye`, swatches(list, state.dyes[id]?.[k] || '', (v) => actions.setDye(id, k, v || null))));
      });
    }
  }
  root.append(wear.sec);

  // Gear.
  const kit = section('Gear');
  const items = gear?.items || [];
  if (!items.length) {
    kit.body.append(el('p', { class: 'note' }, 'No gear built yet. art/blender/build_gear.py writes assets/catalog/gear.json.'));
  } else {
    const gearSlots = [
      ['main_hand', (g) => g.slot === 'main_hand' || g.slot === 'two_hand'],
      ['off_hand', (g) => g.slot === 'off_hand'],
      ['back', (g) => g.slot === 'back'],
      ['ammo', (g) => g.slot === 'ammo'],
    ];
    for (const [slot, test] of gearSlots) {
      const list = items.filter(test).filter((g) => g.file || g.files?.[state.body]);
      if (!list.length) continue;
      const families = [...new Set(list.map((g) => g.family || 'other'))];
      const options = [{ value: '', label: 'None' }];
      for (const f of families) {
        options.push({ group: f.replace(/^\w/, (c) => c.toUpperCase()), options: list.filter((g) => (g.family || 'other') === f).map((g) => ({ value: g.id, label: g.label + (g.slot === 'two_hand' ? ' (two-handed)' : '') })) });
      }
      const disabled = slot === 'off_hand' && items.find((g) => g.id === state.gear.main_hand)?.slot === 'two_hand';
      const s = select(options, state.gear[slot] || '', (v) => actions.setGear(slot, v));
      if (disabled) s.disabled = true;
      kit.body.append(row(slotLabel(slot), s));
    }
    kit.body.append(el('div', { class: 'row' }, toggle('Weapons drawn', state.drawn, actions.setDrawn)));
    if (character?.attacks?.().length) {
      const attack = el('button', { type: 'button', class: 'wide' }, 'Attack');
      attack.addEventListener('click', actions.attack);
      kit.body.append(el('div', { class: 'row buttons' }, attack));
    }
  }
  root.append(kit.sec);

  // Animation.
  const anim = section('Animation');
  const clips = humanoid.animations.clips.filter((c) => !c.name.startsWith('A_TPose'));
  const groups = [];
  const weaponClips = character ? character.gearClips().filter((n) => clips.some((c) => c.name === n)) : [];
  if (weaponClips.length) groups.push({ group: 'Weapon', options: weaponClips.map((n) => ({ value: n, label: pretty(n) })) });
  for (const tag of TAG_ORDER) {
    const list = clips.filter((c) => (c.tags?.[0] || 'idle') === tag);
    if (list.length) groups.push({ group: TAG_LABELS[tag], options: list.map((c) => ({ value: c.name, label: pretty(c.name) })) });
  }
  anim.body.append(row('Clip', select(groups, state.clip, actions.setClip)));
  const play = el('button', { type: 'button', class: 'wide' }, state.paused ? 'Play' : 'Pause');
  play.addEventListener('click', actions.togglePause);
  const speed = el('input', { type: 'range', min: '0.1', max: '2', step: '0.05', value: String(state.speed) });
  const speedOut = el('span', { class: 'value' }, `${Number(state.speed).toFixed(2)}×`);
  speed.addEventListener('input', () => {
    speedOut.textContent = `${Number(speed.value).toFixed(2)}×`;
    actions.setSpeed(Number(speed.value));
  });
  const scrub = el('input', { type: 'range', min: '0', max: '1', step: '0.001', value: '0', class: 'scrub' });
  scrub.addEventListener('input', () => actions.scrub(Number(scrub.value)));
  const timeOut = el('span', { class: 'value' }, '0.00 s');
  anim.body.append(
    el('div', { class: 'row buttons' }, play),
    row('Speed', el('div', { class: 'slider' }, speed, speedOut)),
    row('Time', el('div', { class: 'slider' }, scrub, timeOut)),
  );
  root.append(anim.sec);

  appendView(root, state, actions);

  root.append(el('footer', { class: 'status', id: 'status' }, ''));
  return {
    setTime(t, duration) {
      if (document.activeElement !== scrub) scrub.value = duration ? String(t / duration) : '0';
      timeOut.textContent = `${t.toFixed(2)} / ${duration.toFixed(2)} s`;
    },
    setPlaying(paused) {
      play.textContent = paused ? 'Play' : 'Pause';
    },
  };
}


/** The camera, debug and screenshot section, shared by characters and beasts. */
function appendView(root, state, actions) {
  // View.
  const view = section('View');
  const cams = el('div', { class: 'grid3' });
  for (const [name, label] of [['front', 'Front'], ['back', 'Back'], ['left', 'Left'], ['right', 'Right'], ['three_quarter', '¾'], ['head', 'Head']]) {
    const b = el('button', { type: 'button', class: state.cam === name ? 'on' : '' }, label);
    b.addEventListener('click', () => actions.setCamera(name));
    cams.append(b);
  }
  view.body.append(
    cams,
    el('div', { class: 'row' }, toggle('Turntable', state.turn, actions.setTurntable)),
    el('div', { class: 'row' }, toggle('Region colours', state.debug.includes('regions'), (on) => actions.setDebug('regions', on))),
    el('div', { class: 'row' }, toggle('Wireframe', state.debug.includes('wire'), (on) => actions.setDebug('wire', on))),
    el('div', { class: 'row' }, toggle('Skeleton and sockets', state.debug.includes('skel'), (on) => actions.setDebug('skel', on))),
  );
  const shot = el('button', { type: 'button', class: 'wide' }, 'Save screenshot');
  shot.addEventListener('click', actions.screenshot);
  view.body.append(el('div', { class: 'row buttons' }, shot));
  root.append(view.sec);

}

function pretty(name) {
  return name
    .replace(/_Loop$/, '')
    .replace(/_/g, ' ')
    .replace(/\bFwd\b/, 'forward')
    .replace(/\bRec\b/, 'recover');
}
