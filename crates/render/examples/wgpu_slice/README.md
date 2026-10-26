# wgpu comparison slice (#95)

An isolated Cargo package; the shipping client and renderer dependency graph are unchanged.
It reads the supplied catalogues and reuses the existing terrain topology, scenery placement,
LOD selection, model decoding and camera maths. The standalone Linux wrapper supplies only
Camera and the rigid Draw contract, because the shipping crate exports Camera through Metal.

The fixed river-review camera draws three 1024px shadow cascades, sky, instanced opaque scenery,
water/transparency, TAA at internal resolution and composite at output resolution. Materials,
lighting, exposure, fog and water values come from the current catalogues. Characters and HUD
are excluded from both paths. This is an offscreen experiment; it cannot prove presented FPS.

Build from the repository root:

```sh
CARGO_INCREMENTAL=0 cargo build --release --locked \
  --manifest-path crates/render/examples/wgpu_slice/Cargo.toml
```

The package has its own lockfile and target directory. Set CARGO_TARGET_DIR explicitly to share
build output or use a volume with space. On SMB, disable incremental compilation: rustc's
incremental lock protocol is not supported by the mounted NAS, even though ordinary file
locking works. Do not run two Cargo processes against that NAS target concurrently.

Every invocation that draws reports timings, so take the shared GPU lock first, including for
validation. On the Mac, use the repository's AGENTS.md lock loop, write the owner, and install
an EXIT trap that removes only the lock acquired by that invocation. Do not remove another
worker's lock. Then run the binary from the chosen target directory:

```sh
aldermoor-wgpu-slice --backend wgpu --assets /absolute/path/to/aldermoor-rs/assets \
  --width 2560 --height 1600 --scale 0.75 --frames 480 --warmup 120 > wgpu.json
aldermoor-wgpu-slice --backend metal --assets /absolute/path/to/aldermoor-rs/assets \
  --width 2560 --height 1600 --scale 0.75 --frames 480 --warmup 120 > metal.json
```

For the Mac capture worker, add `--capture /path/to/output.png` to each backend invocation.
Only an explicit capture argument writes an image. Capture invocations are for visual checks;
do not use their timing reports for adoption (the native capture includes encoding overhead).

Run with `--validate` before performance runs. Benchmark builds default to backend API validation
off, without disabling wgpu's safety checks. Both paths use one frame in flight and split scene
passes so sky, opaque and water can be measured individually. The report contains every measured
frame and identifies the adapter. CPU encode time includes preparation and submission; frame
work also includes waiting and timestamp readback. The GPU span includes pass boundaries.

On Linux the backend is Vulkan. With Mesa lavapipe selected, a small validation run proves the
portable path draws; it is not a GPU performance comparison. The dedicated CI workflow targets
both platforms, but account billing currently prevents it from starting. The local Linux build
and seven tests passed; the emulated lavapipe draw timed out and remains unverified.

## Evidence required before adoption

- Passing shader validation, clippy, Mac and lavapipe draws on the latest commit.
- Matched ABBA runs on the M1 Air at 2560x1600, 75% scale, one frame in flight; all pass timings
  within 5%, CPU no worse, with the 12 ms GPU budget and 6 ms client CPU budget reported.
- Mac-worker captures from both backends at the same frame and camera, posted on the PR using
  attach-visuals. Codex does not create or post image files. Inspect terrain normals, sky seam,
  foliage cut-outs, shadow edges, reflections, water blending and TAA over moving foliage.
- A draw-count stress or 200-character equivalent, launch and memory measurements for #93.

The initial M1 slice misses the GPU adoption gate; keep the shipped Metal renderer. See the
[ABBA report](../../../../docs/benchmarks/2026-10-25/wgpu-slice/README.md). Visual parity and the
remaining acceptance gates are still open. ASTC upload is deliberately rejected if compressed
catalogues arrive during the experiment: silently decoding only one backend would invalidate
comparison with Metal. Asset compression and any required packing changes belong to Claude.
