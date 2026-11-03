"""Run the production worker against the extraction corpus inside bounded Linux."""
import hashlib
import json
import os
import re
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import sys

sys.path.insert(0, "/corpus")
from benchmark import metrics

ROOT = Path('/corpus')
LIMITS = dict(max_input_bytes=20971520, max_archive_bytes=67108864,
              max_member_bytes=33554432, max_archive_ratio=100,
              max_archive_entries=4096, max_xml_depth=128, max_pages=100,
              max_slides=100, max_sheets=256, max_cells=1000000,
              max_image_pixels=25000000, max_text_bytes=8388608)
def mixed_pdf():
    source = (ROOT / 'fixtures/scan.pdf').read_bytes()
    match = re.search(rb'/Subtype /Image.*?/Length (\d+) >>\s*stream\r?\n', source)
    assert match
    jpeg = source[match.end():match.end()+int(match[1])]
    def stream(data, header=b''):
        return b'<< '+header+b' /Length '+str(len(data)).encode()+b' >>\nstream\n'+data+b'\nendstream'
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>',
               b'<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>',
               b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 369.6 477.6] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>',
               b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 369.6 477.6] /Resources << /XObject << /Im1 8 0 R >> >> /Contents 7 0 R >>',
               b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
               stream(b'BT /F1 12 Tf 20 200 Td (Native first page marker) Tj ET'),
               stream(b'q 369.6 0 0 477.6 0 0 cm /Im1 Do Q'),
               stream(jpeg, b'/Type /XObject /Subtype /Image /Width 770 /Height 995 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode')]
    pdf = b'%PDF-1.4\n'
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(pdf))
        pdf += str(index).encode()+b' 0 obj\n'+obj+b'\nendobj\n'
    xref = len(pdf)
    pdf += b'xref\n0 9\n0000000000 65535 f \n'
    for offset in offsets[1:]:
        pdf += f'{offset:010} 00000 n \n'.encode()
    pdf += f'trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()
    return pdf

rows = []
for case in json.loads((ROOT / 'manifest.json').read_text())['cases']:
    path = ROOT / case['path']
    assert hashlib.sha256(path.read_bytes()).hexdigest() == case['sha256']
    with tempfile.TemporaryDirectory() as temp:
        measurement = Path(temp) / 'time'
        started = time.monotonic()
        child = subprocess.Popen(
            ['/usr/bin/time', '-f', '%M', '-o', str(measurement),
             'prlimit', '--cpu=60:60', '--as=1073741824:1073741824',
             '--fsize=16777216:16777216', '--core=0:0', '--',
             '/usr/local/bin/whorl-extract', str(path)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            start_new_session=True, env={**os.environ, 'TMPDIR': temp})
        try:
            stdout, stderr = child.communicate(json.dumps(LIMITS).encode(), timeout=60)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate()
            raise AssertionError(f"{case['id']}: timeout")
        assert child.returncode == 0, (case['id'], stderr)
        result = json.loads(stdout)
        expected = case['expected']['outcome']
        if expected == 'success':
            assert 'Ok' in result, (case['id'], result)
            assert result['Ok']['text'].strip(), case['id']
        elif expected == 'detect_png':
            assert 'Ok' in result and result['Ok']['ocr'], (case['id'], result)
        else:
            error = {'empty_file': 'EmptyFile', 'no_extractable_text': 'NoExtractableText',
                     'malformed_file': 'MalformedFile', 'limit_probe': 'LimitExceeded',
                     'file_too_large': 'FileTooLarge'}[expected]
            assert result == {'Err': error}, (case['id'], result, error)
        quality = None
        if 'Ok' in result:
            text = result['Ok']['text']
            metadata = dict(result['Ok']['metadata'])
            for key, value in case['expected'].get('metadata', {}).items():
                if key in metadata and isinstance(value, int):
                    metadata[key] = int(metadata[key])
                if key in metadata and isinstance(value, list):
                    metadata[key] = metadata[key].split('\n')
            quality = metrics(case, dict(text=text, metadata=metadata, outcome='success'))
            if case['expected'].get('comparison') == 'ordered-tokens':
                assert quality['coverage'] == 1, (case['id'], quality)
                if 'notes' not in case['id']:
                    assert quality['wer'] == 0, (case['id'], quality)
            if case['expected'].get('metadata'):
                assert quality['metadata_accuracy'] == 1, (case['id'], quality, metadata)
            reference = case['expected'].get('text_path')
            if reference and case['expected'].get('comparison') == 'exact':
                assert ' '.join(text.split()) == ' '.join((ROOT / reference).read_text().split()), case['id']
            text_hash = hashlib.sha256(text.encode()).hexdigest()
        else:
            text_hash = None
        rows.append(dict(case=case['id'], outcome='success' if 'Ok' in result else result['Err'],
                         wall_ms=round((time.monotonic()-started)*1000, 2),
                         peak_rss_kib=int(measurement.read_text().strip()), text_sha256=text_hash, quality=quality))
with tempfile.TemporaryDirectory() as temp:
    path = Path(temp) / 'mixed.pdf'
    path.write_bytes(mixed_pdf())
    result = subprocess.run(['prlimit', '--cpu=60:60', '--as=1073741824:1073741824',
                             '/usr/local/bin/whorl-extract', str(path)],
                            input=json.dumps(LIMITS).encode(), capture_output=True, timeout=60)
    document = json.loads(result.stdout)['Ok']
    assert document['ocr'] and document['metadata']['pages'] == '2'
    assert 'Native first page marker' in document['text']
    assert 'Watson' in document['text'], document['text']
    rows.append(dict(case='mixed-native-scanned-pdf', outcome='success',
                     text_sha256=hashlib.sha256(document['text'].encode()).hexdigest()))
print(json.dumps(dict(machine=os.uname().machine,
                      cpu_quota=Path('/sys/fs/cgroup/cpu.max').read_text().strip(),
                      memory_limit_bytes=int(Path('/sys/fs/cgroup/memory.max').read_text()),
                      swap_limit_bytes=int(Path('/sys/fs/cgroup/memory.swap.max').read_text()),
                      limits=LIMITS, cases=rows), indent=2))
