import unittest
import os
import signal
import sys
import tempfile
from pathlib import Path

from benchmark import bounded_process, distance, lcs_length, metrics, normalize


class MetricsTests(unittest.TestCase):
    def test_distance_counts_insertions_deletions_and_substitutions(self):
        self.assertEqual(distance('abc', 'axc!'), 2)
        self.assertEqual(distance([], ['extra']), 1)
        self.assertEqual(distance(['same'] * 100000, ['same'] * 100000), 0)

    def test_lcs_preserves_repeats_and_order(self):
        self.assertEqual(lcs_length('abac', 'aabc'), 3)
        self.assertEqual(lcs_length('abc', 'cba'), 1)

    def test_normalization_preserves_punctuation_but_joins_line_hyphenation(self):
        self.assertEqual(normalize(' “Cafe\u0301”  won’t\n dis-\njoin ', True), '"café" won\'t disjoin')

    def test_failure_has_no_accuracy_and_anchor_reference_has_no_wer(self):
        case = {'expected': {'metadata': {'width': 640}, 'text_path': 'expected/map.txt', 'comparison': 'unordered-anchors'}}
        failed = metrics(case, {'outcome': 'extraction_failed'})
        self.assertIsNone(failed['metadata_accuracy'])
        self.assertIsNone(failed['wer'])
        partial = metrics(case, {'outcome': 'success', 'text': 'Bokale', 'metadata': {'width': 640}})
        self.assertEqual(partial['metadata_accuracy'], 1)
        self.assertEqual(partial['null_reasons']['wer'], 'partial_reference')
        self.assertGreater(partial['coverage'], 0)
        self.assertLess(partial['coverage'], 1)


@unittest.skipUnless(sys.platform == 'linux', 'Linux process-group collector')
class IsolationTests(unittest.TestCase):
    def test_deadline_kills_group_and_captures_child_memory(self):
        with tempfile.TemporaryDirectory() as directory, open(os.devnull, 'wb') as output:
            script = "import subprocess,time; subprocess.Popen(['python3','-c','import time; x=bytearray(30*1024*1024); time.sleep(30)']); time.sleep(30)"
            status, timeout, elapsed, peak = bounded_process(
                ['python3', '-c', script], output, output, directory, 0.5)
            self.assertTrue(timeout)
            self.assertEqual(status, -signal.SIGKILL)
            self.assertLess(elapsed, 2000)
            self.assertGreater(peak, 30 * 1024 * 1024)

    def test_output_file_cap_is_enforced(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'output'
            with target.open('wb') as output:
                status, timeout, _, _ = bounded_process(
                    ['python3', '-c', "import sys; sys.stdout.buffer.write(b'x' * (20*1024*1024))"],
                    output, output, directory, 5)
            self.assertFalse(timeout)
            self.assertNotEqual(status, 0)
            self.assertLessEqual(target.stat().st_size, 16 * 1024 * 1024)


if __name__ == '__main__':
    unittest.main()
