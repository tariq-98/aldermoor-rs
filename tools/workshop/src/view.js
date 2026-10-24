// The stage: renderer, studio lighting, ground, camera presets and orbit controls.

import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';

// Character faces +Z. Presets are camera directions from the target, plus a framing height.
const PRESETS = {
  front: { dir: [0, 0.08, 1], fit: 'body' },
  back: { dir: [0, 0.08, -1], fit: 'body' },
  left: { dir: [1, 0.08, 0], fit: 'body' }, // The character's left side is +X.
  right: { dir: [-1, 0.08, 0], fit: 'body' },
  three_quarter: { dir: [0.55, 0.12, 0.85], fit: 'body' },
  head: { dir: [0.3, 0.05, 1], fit: 'head' },
};

export class View {
  constructor(container) {
    this.container = container;
    const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true, alpha: false });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.AgXToneMapping ?? THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.05;
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;
    container.appendChild(renderer.domElement);
    this.renderer = renderer;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color().setRGB(0.36, 0.38, 0.42, THREE.SRGBColorSpace);
    scene.fog = new THREE.Fog(scene.background.clone(), 9, 22);
    this.scene = scene;

    // A soft room for reflections on metal and leather.
    const pmrem = new THREE.PMREMGenerator(renderer);
    scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
    scene.environmentIntensity = 0.35;

    this.camera = new THREE.PerspectiveCamera(30, 1, 0.05, 60);
    this.controls = new OrbitControls(this.camera, renderer.domElement);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.08;
    this.controls.minDistance = 0.4;
    this.controls.maxDistance = 12;
    this.controls.autoRotateSpeed = 1.6;
    this.controls.target.set(0, 1.0, 0);

    this.addLights();
    this.addGround();
    this.fitHeight = 1.8;
    this.resize();
    window.addEventListener('resize', () => this.resize());
  }

  addLights() {
    const s = this.scene;
    const hemi = new THREE.HemisphereLight(0xdfe8ff, 0x5a4b3a, 0.55);
    s.add(hemi);
    // Warm key from the front-left, casting the soft shadow.
    const key = new THREE.DirectionalLight(0xfff0dd, 2.4);
    key.position.set(-2.6, 3.6, 3.2);
    key.castShadow = true;
    key.shadow.mapSize.set(2048, 2048);
    key.shadow.camera.near = 0.5;
    key.shadow.camera.far = 12;
    key.shadow.camera.left = -1.6;
    key.shadow.camera.right = 1.6;
    key.shadow.camera.top = 2.4;
    key.shadow.camera.bottom = -0.6;
    key.shadow.bias = -0.0004;
    key.shadow.normalBias = 0.02;
    key.shadow.radius = 4;
    s.add(key);
    s.add(key.target);
    // Cool fill from the right.
    const fill = new THREE.DirectionalLight(0xd8e4ff, 0.8);
    fill.position.set(3.2, 1.6, 1.6);
    s.add(fill);
    // Rim from behind, to separate the silhouette from the background.
    const rim = new THREE.DirectionalLight(0xffffff, 1.6);
    rim.position.set(0.6, 2.6, -3.4);
    s.add(rim);
    this.lights = { hemi, key, fill, rim };
  }

  addGround() {
    const geo = new THREE.CircleGeometry(4, 96);
    geo.rotateX(-Math.PI / 2);
    // A gentle falloff to the background colour at the rim.
    const colors = [];
    const pos = geo.attributes.position;
    const inner = new THREE.Color().setRGB(0.47, 0.46, 0.44, THREE.SRGBColorSpace);
    const outer = this.scene.background.clone();
    for (let i = 0; i < pos.count; i++) {
      const r = Math.hypot(pos.getX(i), pos.getZ(i)) / 4;
      const c = inner.clone().lerp(outer, THREE.MathUtils.smoothstep(r, 0.35, 1));
      colors.push(c.r, c.g, c.b);
    }
    geo.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
    const mat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.95, metalness: 0 });
    const ground = new THREE.Mesh(geo, mat);
    ground.receiveShadow = true;
    this.scene.add(ground);
    this.ground = ground;
  }

  resize() {
    const w = this.container.clientWidth || window.innerWidth;
    const h = this.container.clientHeight || window.innerHeight;
    this.renderer.setSize(w, h, false);
    this.renderer.domElement.style.width = `${w}px`;
    this.renderer.domElement.style.height = `${h}px`;
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
  }

  /**
   * Scales the stage to the subject: a cottage or a windmill needs the camera, fog, shadows and
   * floor several times further out than a character does.
   */
  fitStage(height) {
    const k = Math.max(1, height / 2.2);
    if (this.stageScale === k) return;
    this.stageScale = k;
    this.controls.maxDistance = 12 * k;
    this.scene.fog.near = 9 * k;
    this.scene.fog.far = 22 * k;
    this.camera.far = 60 * k;
    this.camera.updateProjectionMatrix();
    const { key, fill, rim } = this.lights;
    key.position.set(-2.6, 3.6, 3.2).multiplyScalar(k);
    fill.position.set(3.2, 1.6, 1.6).multiplyScalar(k);
    rim.position.set(0.6, 2.6, -3.4).multiplyScalar(k);
    const cam = key.shadow.camera;
    cam.left = -1.6 * k;
    cam.right = 1.6 * k;
    cam.top = 2.4 * k;
    cam.bottom = -0.6 * k;
    cam.far = 12 * k;
    cam.updateProjectionMatrix();
    this.ground.scale.setScalar(k);
  }

  /** Moves the camera to a named preset, framing a character of the given height. */
  preset(name, height = this.fitHeight, headY = null) {
    const p = PRESETS[name] || PRESETS.three_quarter;
    this.fitHeight = height;
    this.fitStage(height);
    const dir = new THREE.Vector3(...p.dir).normalize();
    let target;
    let span;
    if (p.fit === 'head') {
      target = new THREE.Vector3(0, headY ?? height * 0.93, 0);
      span = 0.42;
    } else {
      target = new THREE.Vector3(0, height * 0.5, 0);
      span = height * 1.26;
    }
    const vfov = THREE.MathUtils.degToRad(this.camera.fov);
    const aspect = this.camera.aspect;
    // Fit vertically, and horizontally on narrow screens.
    const distV = span / 2 / Math.tan(vfov / 2);
    const hfov = 2 * Math.atan(Math.tan(vfov / 2) * aspect);
    const distH = (span * 0.55) / 2 / Math.tan(hfov / 2);
    const dist = Math.max(distV, distH);
    this.controls.target.copy(target);
    this.camera.position.copy(target).addScaledVector(dir, dist);
    this.camera.lookAt(target);
    this.controls.update();
    this.presetName = name;
  }

  set turntable(on) {
    this.controls.autoRotate = on;
  }

  get turntable() {
    return this.controls.autoRotate;
  }

  render() {
    this.controls.update();
    this.renderer.render(this.scene, this.camera);
  }

  screenshot(filename = 'aldermoor-workshop.png') {
    this.render();
    const url = this.renderer.domElement.toDataURL('image/png');
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
  }
}

export const PRESET_NAMES = Object.keys(PRESETS);
