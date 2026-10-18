import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("performance", Path(__file__).with_name("check-performance.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PerformanceTests(unittest.TestCase):
    def fixture(self):
        return ([dict(working_set_mib=100, private_mib=80, cpu_one_core_percent=1) for _ in range(25)],
                dict(exit_code=0, helpers_ended=True, cleanup_observations=[
                    dict(is_root=False, has_exited=True, name="private fixture", process_id=123)]),
                dict(schema=1, workload="muted_local_3s", natural_transitions=6, skips=3,
                     request_to_progress_ms=[100, 200, 100, 300, 100, 100], source="private fixture"))

    def test_startup_peak_preserved_and_private_metadata_excluded(self):
        samples, cleanup, playback = self.fixture()
        samples[0]["working_set_mib"] = 400
        report = module.summarize(samples, cleanup, playback)
        self.assertEqual(report["metrics"]["working_set_mib"]["peak"], 400)
        self.assertEqual(report["metrics"]["working_set_mib"]["settled_median"], 100)
        self.assertNotIn("private fixture", json.dumps(report))

    def test_invalid_or_incomplete_evidence_fails(self):
        for value in (-1, float("nan"), float("inf"), True, "100"):
            samples, cleanup, playback = self.fixture()
            samples[0]["private_mib"] = value
            with self.assertRaises(ValueError):
                module.summarize(samples, cleanup, playback)
        for field, value in [("exit_code", 7), ("helpers_ended", False), ("cleanup_observations", [])]:
            samples, cleanup, playback = self.fixture()
            cleanup[field] = value
            with self.assertRaises(ValueError):
                module.summarize(samples, cleanup, playback)
        for field, value in [("skips", 2), ("natural_transitions", 0),
                             ("request_to_progress_ms", []), ("request_to_progress_ms", [6000] * 6)]:
            samples, cleanup, playback = self.fixture()
            playback[field] = value
            with self.assertRaises(ValueError):
                module.summarize(samples, cleanup, playback)


if __name__ == "__main__":
    unittest.main()
