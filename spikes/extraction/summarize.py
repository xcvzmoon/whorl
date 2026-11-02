import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import statistics
import tarfile
import unicodedata

from benchmark import ROOT, normalize


def similarity_text(text):
    return ' '.join(''.join(c for c in normalize(text, True) if c.isalnum() or c.isspace()).split())


def shingles(text, unit, k):
    normalized = similarity_text(text)
    values = normalized.split() if unit == 'word' else list(normalized)
    if len(values) < k:
        return {('short', normalized)} if normalized else set()
    return {tuple(values[index:index + k]) for index in range(len(values) - k + 1)}


def jaccard(left, right):
    return len(left & right) / len(left | right) if left or right else None


def summarize(measurements):
    groups = defaultdict(list)
    for trial in measurements['trials']:
        if not trial['warmup']:
            groups[(trial['case_id'], trial['candidate'])].append(trial)
    rows = []
    for (case, candidate), trials in groups.items():
        row = {'case_id': case, 'candidate': candidate, 'repetitions': len(trials),
               'outcomes': sorted({trial['outcome'] for trial in trials}),
               'median_wall_ms': statistics.median(t['wall_ms'] for t in trials),
               'min_wall_ms': min(t['wall_ms'] for t in trials), 'max_wall_ms': max(t['wall_ms'] for t in trials),
               'max_sampled_tree_rss_bytes': max(t['peak_process_tree_rss_bytes'] for t in trials),
               'distinct_text_hashes': len({t['artifacts']['text.txt']['sha256'] for t in trials})}
        for metric in ['wer', 'cer', 'coverage', 'metadata_accuracy']:
            values = [t[metric] for t in trials if t[metric] is not None]
            row[metric] = statistics.median(values) if values else None
        rows.append(row)
    return rows


def shingle_trials(measurements, raw):
    manifest = json.loads((ROOT / 'manifest.json').read_text())
    cases = {case['id']: case for case in manifest['cases']}
    rows = []
    variants = [('word', 1), ('word', 2), ('word', 3), ('character', 3), ('character', 5)]
    references = {case_id: (ROOT / cases[case_id]['expected']['text_path']).read_text()
                  for case_id in ['scan-pdf', 'photo-jpeg', 'table-docx', 'table-pptx', 'workbook-xlsx']}
    for trial in measurements['trials']:
        if trial['repetition'] != 1 or trial['case_id'] not in ['scan-pdf', 'photo-jpeg']:
            continue
        if 'ocrs' not in trial['candidate'] and 'tesseract' not in trial['candidate']:
            continue
        artifact = trial['artifacts']['text.txt']
        actual = raw(artifact['path']).decode()
        for unit, k in variants:
            left = shingles(actual, unit, k)
            target = shingles(references[trial['case_id']], unit, k)
            rows.append({'case_id': trial['case_id'], 'candidate': trial['candidate'], 'unit': unit, 'k': k,
                         'ocr_vs_reference_jaccard': jaccard(left, target),
                         'exceeds_075': (jaccard(left, target) or 0) > 0.75,
                         'max_unrelated_reference_jaccard': max(jaccard(left, shingles(text, unit, k)) or 0
                            for key, text in references.items() if key != trial['case_id'])})
    return {'normalization': 'corpus normalization then lowercase, retain Unicode alphanumerics/whitespace, collapse whitespace; character shingles include spaces',
            'short_policy': 'fewer than k units: normalized full-text equality token; empty gives empty set',
            'symmetry': 'same normalization and shingle function for reference/ingest and OCR/query, with no source-dependent configuration',
            'scope': 'Real scan page and STOP photo only; map anchor list has no valid reading order/full transcription. Unrelated controls: other reference plus complete DOCX/PPTX/XLSX references. Descriptive evidence, not recall calibration.',
            'rows': rows}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('directory', type=Path)
    parser.add_argument('--measurements', type=Path)
    args = parser.parse_args()
    data = json.loads((args.measurements or args.directory / 'measurements.json').read_text())
    archive_path = args.directory / 'raw-results.tar.gz'
    if 'raw_archive' in data and hashlib.sha256(archive_path.read_bytes()).hexdigest() != data['raw_archive']['sha256']:
        raise ValueError('Raw archive checksum mismatch')
    with tarfile.open(archive_path) as archive:
        def raw(name):
            member = archive.extractfile(name)
            if member is None:
                raise ValueError(name)
            return member.read()
        for trial in data['trials']:
            for artifact in trial['artifacts'].values():
                if hashlib.sha256(raw(artifact['path'])).hexdigest() != artifact['sha256']:
                    raise ValueError(f"Artifact checksum mismatch: {artifact['path']}")
        evaluation = shingle_trials(data, raw)
    summary = summarize(data)
    (args.directory / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    (args.directory / 'shingles.json').write_text(json.dumps(evaluation, indent=2) + '\n')
    lines = ['# Extraction measurements', '', 'Five measured repetitions per row; warmup excluded. RSS is the peak process-tree estimate, including OCR children and the GNU time individual high-water counter. All metrics are fractions. See [the report](../../../docs/extraction-spike.md) for limitations.', '',
             '| Case | Candidate | Outcomes | Median ms | Max sampled RSS MiB | WER | CER | Coverage | Metadata |',
             '| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |']
    def metric(value):
        return f'{value:.4f}' if value is not None else '—'
    for row in summary:
        lines.append(f"| {row['case_id']} | {row['candidate']} | {', '.join(row['outcomes'])} | {row['median_wall_ms']:.2f} | {row['max_sampled_tree_rss_bytes']/1048576:.2f} | {metric(row['wer'])} | {metric(row['cer'])} | {metric(row['coverage'])} | {metric(row['metadata_accuracy'])} |")
    (args.directory / 'summary.md').write_text('\n'.join(lines) + '\n')
    print(f"Verified {len(data['trials'])} trials and all raw artifact hashes; wrote {len(summary)} summaries.")


if __name__ == '__main__':
    main()
