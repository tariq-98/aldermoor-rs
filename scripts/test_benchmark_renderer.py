"""Exercise report validation using a fake executable; no GPU or art generation."""
import copy
import json
import pathlib
import subprocess
import tempfile
import unittest

HARNESS = pathlib.Path(__file__).with_name('benchmark_renderer.py')
NAMES = ['shadow_0', 'shadow_1', 'shadow_2', 'scene_sky_opaque_water_transparent', 'taa', 'composite', 'hud']


def fixture():
    return dict(counters_enabled=True, samples=32, counter_sampled_frames=32,
                gpu_ms=dict(median=10.0, p95=11.0, samples=32),
                passes={name: dict(missing_samples=0, elapsed_ms=dict(median=1., p95=2., samples=32)) for name in NAMES})


class ReportTests(unittest.TestCase):
    def run_report(self, details, *options):
        with tempfile.TemporaryDirectory() as directory:
            directory = pathlib.Path(directory)
            binary = directory / 'client'
            log = ('Metal: Test | 64x64 -> 64x64 | 1x samples | TAA | cascades=3x1024\n'
                   'GPU median=99ms p95=100ms samples=31\n'
                   'FRAME median=3ms p95=4ms samples=31 mode=offscreen-throughput\n'
                   'PERF ' + json.dumps(details))
            binary.write_text('#!/usr/bin/env python3\nprint(' + repr(log) + ')\n')
            binary.chmod(0o700)
            output = directory / 'report.json'
            result = subprocess.run(['python3', str(HARNESS), '--binary', str(binary),
                                     '--size', '64x64', '--frames', '36', '--warmup', '4',
                                     '--no-check', '--output', str(output), *options],
                                    capture_output=True, text=True)
            return result, json.loads(output.read_text()) if output.exists() else None

    def test_completed_ids_replace_stale_legacy_gpu_summary(self):
        result, report = self.run_report(fixture())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(report['gpu_samples'], 32)
        self.assertEqual(report['gpu_median_ms'], 10.)
        self.assertIsNone(report['presentation_pass'])
        self.assertEqual(len(report['binary_sha256']), 64)

    def test_missing_completion_or_timestamp_is_rejected(self):
        for change in ('completion', 'pass', 'coverage'):
            data = copy.deepcopy(fixture())
            if change == 'completion': data['samples'] -= 1
            elif change == 'coverage': data['counter_sampled_frames'] = 0
            else: data['passes']['taa']['elapsed_ms'] = None
            result, report = self.run_report(data)
            self.assertNotEqual(result.returncode, 0, change)
            self.assertIsNone(report)

    def test_cadence_skips_account_for_coverage_without_hiding_missing_counters(self):
        for skipped in (16, 32):
            data = fixture()
            far = data['passes']['shadow_2']
            far['skipped_samples'] = skipped
            if skipped == 32: far['elapsed_ms'] = None
            else: far['elapsed_ms']['samples'] = 32 - skipped
            result, _ = self.run_report(data)
            self.assertEqual(result.returncode, 0, result.stderr)
            far['missing_samples'] = 1
            result, _ = self.run_report(data)
            self.assertNotEqual(result.returncode, 0)
            far['missing_samples'] = 0
            far['skipped_samples'] -= 1
            result, _ = self.run_report(data)
            self.assertNotEqual(result.returncode, 0)

    def test_unavailable_counters_require_explicit_opt_out(self):
        data = fixture()
        data['counters_enabled'] = False
        result, _ = self.run_report(data)
        self.assertNotEqual(result.returncode, 0)
        result, report = self.run_report(data, '--no-gpu-counters')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(report['performance']['counters_enabled'])

    def test_abba_sampling_has_half_coverage(self):
        data = fixture()
        data['counter_sampled_frames'] = 16
        for value in data['passes'].values(): value['elapsed_ms']['samples'] = 16
        result, report = self.run_report(data, '--counter-overhead')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(report['counter_overhead'])


if __name__ == '__main__':
    unittest.main()
