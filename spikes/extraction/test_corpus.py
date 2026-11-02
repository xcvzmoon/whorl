"""Check that corpus verification catches damaged inputs and missing coverage."""
import contextlib
import io
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import corpus


class CorpusVerificationTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for name in ['generated', 'licenses']:
            (self.root / name).symlink_to(corpus.ROOT / name, target_is_directory=True)
        (self.root / 'corpus.py').symlink_to(corpus.ROOT / 'corpus.py')
        (self.root / 'fixtures').mkdir()
        for source in (corpus.ROOT / 'fixtures').iterdir():
            (self.root / 'fixtures' / source.name).symlink_to(source)
        shutil.copytree(corpus.ROOT / 'expected', self.root / 'expected')
        self.manifest = json.loads((corpus.ROOT / 'manifest.json').read_text())
        self.save_manifest()
        root_patch = patch.object(corpus, 'ROOT', self.root)
        self.addCleanup(root_patch.stop)
        root_patch.start()

    def save_manifest(self):
        (self.root / 'manifest.json').write_text(json.dumps(self.manifest))

    def test_complete_corpus_passes(self):
        with contextlib.redirect_stdout(io.StringIO()):
            corpus.verify()

    def test_changed_fixture_is_rejected(self):
        fixture = self.root / 'fixtures/stop.jpg'
        original = fixture.read_bytes()
        fixture.unlink()
        fixture.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
        with self.assertRaisesRegex(ValueError, 'fixture differs'):
            corpus.verify()

    def test_changed_reference_is_rejected(self):
        (self.root / 'expected/photo.txt').write_text('BROKEN\n')
        with self.assertRaisesRegex(ValueError, 'reference differs'):
            corpus.verify()

    def test_missing_boundary_format_is_rejected(self):
        self.manifest['cases'] = [case for case in self.manifest['cases'] if case['id'] != 'boundary-jpeg']
        self.save_manifest()
        with self.assertRaisesRegex(ValueError, 'missing formats'):
            corpus.verify()

    def test_missing_license_is_rejected(self):
        self.manifest['cases'][0]['provenance']['license'] = ''
        self.save_manifest()
        with self.assertRaisesRegex(ValueError, 'missing license evidence'):
            corpus.verify()


if __name__ == '__main__':
    unittest.main()
