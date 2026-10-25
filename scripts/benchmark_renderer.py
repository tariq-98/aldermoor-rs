#!/usr/bin/env python3
"""Real client benchmark: fail when measured GPU/frame budgets exceed the requested limits."""
import argparse, hashlib, json, pathlib, re, subprocess, sys
from datetime import datetime, timezone
p = argparse.ArgumentParser()
p.add_argument('--assets', type=pathlib.Path, help='Existing asset catalogue root, shared with a baseline checkout')
p.add_argument('--binary', type=pathlib.Path, help='Client executable (allows matched pre-change comparisons)')
p.add_argument('--size', default='5120x1440')
p.add_argument('--scale', type=float, default=0.8)
p.add_argument('--live', action='store_true')
p.add_argument('--legacy-cpu', action='store_true')
p.add_argument('--look-churn', action='store_true')
p.add_argument('--reset-on-look', action='store_true')
p.add_argument('--output-blit', action='store_true')
p.add_argument('--look-settings')
p.add_argument('--surface-assets')
p.add_argument('--shared-constants', action='store_true')
p.add_argument('--fps', type=float, default=60)
p.add_argument('--in-flight', type=int, default=2)
p.add_argument('--aa', default='taa')
p.add_argument('--frames', type=int, default=720)
p.add_argument('--warmup', type=int, default=120)
p.add_argument('--gpu-budget', type=float, default=12.0)
p.add_argument('--frame-budget', type=float, default=1000/60)
p.add_argument('--minimum-fps', type=float, default=59.5)
p.add_argument('--presentation-budget', type=float, default=17.2)
p.add_argument('--fullscreen', action='store_true')
p.add_argument('--windowed', action='store_true')
p.add_argument('--output', type=pathlib.Path)
p.add_argument('--capture')
p.add_argument('--gpu-profile-scene', action='store_true', help='Split scene encoders for diagnostic timings; changes tile load/store costs')
p.add_argument('--counter-overhead', action='store_true', help='Alternate timestamps in balanced ABBA/BAAB frame groups without changing queue depth')
p.add_argument('--no-gpu-counters', action='store_true', help='Disable timestamp sampling for overhead comparisons')
p.add_argument('--perf-overlay', action='store_true', help='Start the F3 overlay visible (off for cost comparisons)')
p.add_argument('--no-check', action='store_true')
a = p.parse_args()
if a.fullscreen: a.windowed = True
if a.no_gpu_counters and (a.gpu_profile_scene or a.counter_overhead): p.error('Profiling requires counters')
if a.counter_overhead and (a.perf_overlay or a.gpu_profile_scene): p.error('Counter overhead needs overlay off and the normal scene layout')
if a.gpu_profile_scene and a.aa == 'msaa': p.error('Split scene profiling requires single-sample rendering')
if a.frames < a.warmup + 30: p.error('Need at least 30 measured frames after warmup')
root = pathlib.Path(__file__).resolve().parents[1]
binary = (a.binary or root/'target/release/client').resolve()
started_at = datetime.now(timezone.utc).isoformat()
binary_digest = hashlib.sha256(binary.read_bytes()).hexdigest()
cmd = [str(binary), '--showcase', '--benchmark', '--fps', str(a.fps), '--in-flight', str(a.in_flight), '--aa', a.aa, '--frames', str(a.frames), '--warmup', str(a.warmup), '--size', a.size, '--scale', str(a.scale)]
if a.assets: cmd += ['--assets', str(a.assets.resolve())]
if a.capture: cmd += ['--capture', a.capture]
if a.no_gpu_counters: cmd.append('--no-gpu-counters')
if a.gpu_profile_scene: cmd.append('--gpu-profile-scene')
if a.counter_overhead: cmd.append('--counter-overhead')
if a.perf_overlay: cmd.append('--perf-overlay')
if a.live: cmd.remove('--showcase')
if not a.windowed: cmd.append('--offscreen')
if a.shared_constants: cmd.append('--shared-constants')
if a.output_blit: cmd.append('--output-blit')
if a.legacy_cpu: cmd.append('--legacy-cpu')
if a.look_churn: cmd.append('--look-churn')
if a.reset_on_look: cmd.append('--reset-on-look')
if a.look_settings: cmd += ['--look-settings', a.look_settings]
if a.surface_assets: cmd += ['--surface-assets', a.surface_assets]
if a.fullscreen: cmd.append('--fullscreen')
r = subprocess.run(cmd, cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
print(r.stdout, end='')
if r.returncode: sys.exit(r.returncode)
expected = 4 if a.aa == 'msaa' else 1
if f'| {expected}x samples' not in r.stdout and a.aa not in ('taa', 'metalfx'): sys.exit('Requested antialiasing mode was not used; refusing to compare timings')
if a.aa == 'taa' and '| TAA |' not in r.stdout: sys.exit('Temporal AA was requested but not used')
if a.aa == 'metalfx' and 'TEMPORAL MetalFX' not in r.stdout: sys.exit('MetalFX unavailable; fallback run cannot be labelled MetalFX')
gpu = re.search(r'GPU median=([\d.]+)ms p95=([\d.]+)ms samples=(\d+)', r.stdout)
frame = re.search(r'FRAME median=([\d.]+)ms p95=([\d.]+)ms samples=(\d+) mode=([\w-]+)', r.stdout)
if not gpu or not frame: sys.exit('Missing GPU or full frame measurements')
report = dict(capture=a.capture, output=a.size, scale=a.scale, mode=frame[4], aa=a.aa, in_flight=a.in_flight, paced_fps=a.fps, direct_drawable=not a.output_blit, legacy_cpu=a.legacy_cpu, live=a.live, look_churn=a.look_churn, reset_on_look=a.reset_on_look, frame_measure='completed-work' if a.in_flight==1 else 'cpu-encode-and-slot-wait', shared_constants=a.shared_constants, gpu_median_ms=float(gpu[1]), gpu_p95_ms=float(gpu[2]), frame_median_ms=float(frame[1]), frame_p95_ms=float(frame[2]), samples=int(frame[3]), warmup=a.warmup)
# PERF uses completed frame IDs, including the final in-flight drain, to exclude warmup
# accurately. Keep the legacy stdout measurements above for older binaries.
perf = re.search(r'^PERF (.+)$', r.stdout, re.MULTILINE)
if perf:
    details = json.loads(perf[1])
    report['performance'] = details
    if not a.no_gpu_counters and not details['counters_enabled']:
        sys.exit('GPU counters unavailable; use --no-gpu-counters for an explicit uninstrumented run')
    expected_samples = a.frames - a.warmup
    if details['samples'] != expected_samples:
        sys.exit(f'Incomplete completed-frame measurements: {details["samples"]} != {expected_samples}')
    report.update(gpu_median_ms=details['gpu_ms']['median'], gpu_p95_ms=details['gpu_ms']['p95'], gpu_samples=details['samples'])
    if details['counters_enabled']:
        sampled = sum(1 for frame_id in range(a.warmup, a.frames) if not a.counter_overhead or ((frame_id % 4 in (1, 2)) != ((frame_id // 4).bit_count() % 2 == 1)))
        if details['counter_sampled_frames'] != sampled:
            sys.exit('Unexpected timestamp frame coverage')
        expected_passes = ['shadow_0', 'shadow_1', 'shadow_2', 'composite', 'hud']
        expected_passes += ['sky', 'opaque', 'water_transparent'] if a.gpu_profile_scene else ['scene_sky_opaque_water_transparent']
        if a.aa == 'taa': expected_passes.append('taa')
        for name in expected_passes:
            timing = details['passes'][name]['elapsed_ms']
            if details['passes'][name]['missing_samples'] or not timing or timing['samples'] != sampled:
                sys.exit(f'Missing timestamp samples for {name}; refusing incomplete pass timings')
if not perf and (a.counter_overhead or a.gpu_profile_scene or a.perf_overlay or a.no_gpu_counters):
    sys.exit('Missing PERF report for requested diagnostics')
report.update(binary=str(binary), binary_sha256=binary_digest, command=cmd, started_at=started_at, frames=a.frames, counter_overhead=a.counter_overhead, split_scene=a.gpu_profile_scene, overlay_visible=a.perf_overlay)
for field, marker in [('look_settings', 'LOOK'), ('surface_assets', 'SURFACES')]:
    resolved = re.search(r'^' + marker + r' (.+)$', r.stdout, re.MULTILINE)
    report[field] = str(pathlib.Path(resolved[1]).resolve()) if resolved else None
resets = re.search(r'HISTORY resets=(\d+)', r.stdout)
if resets: report['history_resets'] = int(resets[1])
interval=re.search(r'INTERVAL mean_fps=([\d.]+) p95=([\d.]+)ms',r.stdout)
if interval: report.update(mean_fps=float(interval[1]),interval_p95_ms=float(interval[2]))
dimensions=re.search(r'\| (\d+)x(\d+) -> (\d+)x(\d+) \|',r.stdout)
if not dimensions: sys.exit('Missing actual renderer dimensions')
report['internal'] = f'{dimensions[1]}x{dimensions[2]}'
report['output'] = f'{dimensions[3]}x{dimensions[4]}'
if report['output'] != a.size: sys.exit('Window size differs from requested benchmark dimensions')
present=re.search(r'PRESENT mean_fps=([\d.]+) p95=([\d.]+)ms samples=(\d+)',r.stdout)
if present: report.update(presented_mean_fps=float(present[1]),presented_p95_ms=float(present[2]),presented_samples=int(present[3]))
report['work_budget_pass'] = report['gpu_p95_ms'] <= a.gpu_budget and report['frame_p95_ms'] <= a.frame_budget
# Offscreen throughput cannot establish display pacing. A windowed pass requires actual
# drawable presentation times, not just the rate at which the CPU submits work.
report['presentation_pass'] = (report.get('presented_samples', 0) >= 0.95 * (a.frames - a.warmup - 1)
                               and report.get('presented_mean_fps', 0) >= a.minimum_fps
                               and report.get('presented_p95_ms', float('inf')) <= a.presentation_budget) if a.windowed else None
report['pass'] = report['work_budget_pass'] and (report['presentation_pass'] if a.windowed else True)
if a.output:
    a.output.parent.mkdir(parents=True, exist_ok=True)
    a.output.write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report, indent=2))
if not report['pass'] and not a.no_check: sys.exit(2)
