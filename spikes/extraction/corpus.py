"""Materialize and verify the extraction corpus without running extractors."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import zipfile
import zlib

ROOT = Path(__file__).resolve().parent
LIMIT = 20 * 1024 * 1024
FORMATS = {'pdf', 'docx', 'pptx', 'xlsx', 'png', 'jpeg'}


def digest(path):
    with path.open('rb') as stream:
        result = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
        return result.hexdigest()


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def package(entries):
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w') as archive:
        for name, data, compression in entries:
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = compression
            info.external_attr = 0o600 << 16
            archive.writestr(info, data)
    return output.getvalue()


def generate():
    out = ROOT / 'generated'
    out.mkdir(exist_ok=True)
    for fmt, source in [('pdf', 'digital.pdf'), ('docx', 'table.docx'), ('pptx', 'table.pptx'), ('xlsx', 'workbook.xlsx'), ('png', 'map.png'), ('jpeg', 'stop.jpg')]:
        data = (ROOT / 'fixtures' / source).read_bytes()
        if fmt in {'docx', 'pptx', 'xlsx'}:
            with zipfile.ZipFile(io.BytesIO(data)) as archive:
                entries = [(n, archive.read(n), zipfile.ZIP_STORED) for n in sorted(archive.namelist())]
            if fmt == 'xlsx':
                rows = ''.join(f'<row r="{i}"><c r="A{i}" t="inlineStr"><is><t>row {i:06d}</t></is></c></row>' for i in range(1, 100001))
                sheet = ('<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>' + rows + '</sheetData></worksheet>').encode()
                entries = [(n, sheet if n == 'xl/worksheets/sheet1.xml' else d, c) for n, d, c in entries]
            entries.append(('customXml/boundary.xml', b'<padding></padding>', zipfile.ZIP_STORED))
            missing = LIMIT - len(package(entries))
            entries[-1] = ('customXml/boundary.xml', b'<padding>' + b' ' * missing + b'</padding>', zipfile.ZIP_STORED)
            data = package(entries)
        elif fmt == 'pdf':
            position = data.rindex(b'startxref')
            data = data[:position] + b'%' + b' ' * (LIMIT - len(data) - 2) + b'\n' + data[position:]
        elif fmt == 'png':
            position = data.rindex(b'\x00\x00\x00\x00IEND')
            data = data[:position] + chunk(b'whOr', b' ' * (LIMIT - len(data) - 12)) + data[position:]
        else:
            remaining = LIMIT - len(data)
            segments = []
            while remaining:
                size = min(65537, remaining)
                if 0 < remaining - size < 4:
                    size -= 4
                segments.append(b'\xff\xef' + struct.pack('>H', size - 2) + b' ' * (size - 4))
                remaining -= size
            data = data[:2] + b''.join(segments) + data[2:]
        if len(data) != LIMIT:
            raise ValueError(f'{fmt}: incorrect boundary size')
        (out / f'boundary.{fmt}').write_bytes(data)
        (out / f'over-limit.{fmt}').write_bytes(data + b'\n')
        (out / f'malformed.{fmt}').write_bytes(data[:32])
    for fmt, source, member in [('docx', 'table.docx', 'word/bomb.xml'), ('pptx', 'table.pptx', 'ppt/bomb.xml'), ('xlsx', 'workbook.xlsx', 'xl/bomb.xml')]:
        with zipfile.ZipFile(ROOT / 'fixtures' / source) as archive:
            entries = [(n, archive.read(n), zipfile.ZIP_DEFLATED) for n in sorted(archive.namelist())]
        entries.append((member, b'<bomb>' + b' ' * (64 * 1024 * 1024) + b'</bomb>', zipfile.ZIP_DEFLATED))
        (out / f'expansion.{fmt}').write_bytes(package(entries))
    compressor = zlib.compressobj(9)
    compressed = bytearray()
    for _ in range(10000):
        compressed.extend(compressor.compress(b'\0' * 10001))
    compressed.extend(compressor.flush())
    image = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 10000, 10000, 8, 0, 0, 0, 0)) + chunk(b'IDAT', bytes(compressed)) + chunk(b'IEND', b'')
    (out / 'huge.png').write_bytes(image)
    (out / 'empty.pdf').write_bytes(b'')
    (out / 'spoofed.docx').write_bytes((ROOT / 'fixtures' / 'map.png').read_bytes())


def verify():
    manifest = json.loads((ROOT / 'manifest.json').read_text())
    ids = set()
    for case in manifest['cases']:
        if case['id'] in ids:
            raise ValueError('duplicate case id')
        ids.add(case['id'])
        path = ROOT / case['path']
        if path.stat().st_size != case['bytes'] or digest(path) != case['sha256']:
            raise ValueError(f"fixture differs: {case['id']}")
        expected = case['expected']
        if expected.get('text_path'):
            if digest(ROOT / expected['text_path']) != expected['text_sha256']:
                raise ValueError(f"reference differs: {case['id']}")
        if not case['provenance'].get('license') or not case['provenance'].get('license_evidence'):
            raise ValueError('missing license evidence')
        if case['category'] == 'boundary' and case['bytes'] != LIMIT:
            raise ValueError('boundary size differs')
        if case['category'] == 'over-limit' and case['bytes'] != LIMIT + 1:
            raise ValueError('over-limit size differs')
        if case['category'] == 'expansion':
            with zipfile.ZipFile(path) as archive:
                largest = max(archive.infolist(), key=lambda entry: entry.file_size)
                if largest.file_size < 64 * 1024 * 1024 or largest.file_size / largest.compress_size < 100:
                    raise ValueError('expansion case lost its expansion')
        if case['category'] == 'huge-image':
            if struct.unpack('>II', path.read_bytes()[16:24]) != (10000, 10000):
                raise ValueError('huge image dimensions differ')
    for category in ['boundary', 'over-limit', 'malformed']:
        covered = {c['format'] for c in manifest['cases'] if c['category'] == category}
        if covered != FORMATS:
            raise ValueError(f'{category}: missing formats')
    expansions = {c['format'] for c in manifest['cases'] if c['category'] == 'expansion'}
    if expansions != {'docx', 'pptx', 'xlsx'}:
        raise ValueError('missing archive expansion formats')
    for category in ['digital', 'scan', 'photo', 'table', 'images', 'notes', 'spreadsheet', 'huge-image']:
        if not any(c['category'] == category for c in manifest['cases']):
            raise ValueError(f'missing category {category}')
    for attachment in manifest['evidence']:
        if digest(ROOT / attachment['path']) != attachment['sha256']:
            raise ValueError(f"evidence differs: {attachment['path']}")
    print(f"Verified {len(ids)} cases, fixture/reference/evidence checksums, and required coverage.")


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['generate', 'verify'])
    args = parser.parse_args()
    if args.action == 'generate':
        generate()
    else:
        verify()
