"""Check every registry package in Cargo.lock against a 14-day release-age floor."""
import concurrent.futures
import datetime
import json
from pathlib import Path
import sys
import tomllib
import urllib.request

root = Path(__file__).resolve().parent.parent
as_of = datetime.date.fromisoformat(sys.argv[1]) if len(sys.argv) > 1 else datetime.datetime.now(datetime.timezone.utc).date()
cutoff = datetime.datetime.combine(as_of - datetime.timedelta(days=14), datetime.time.min, tzinfo=datetime.timezone.utc)
packages = [p for p in tomllib.loads((root / "Cargo.lock").read_text())['package'] if 'source' in p]

def check(package):
    name, version = package['name'], package['version']
    request = urllib.request.Request(f'https://crates.io/api/v1/crates/{name}/{version}', headers={'User-Agent': 'pdf-editor-dependency-age-check'})
    with urllib.request.urlopen(request, timeout=30) as response:
        data = json.load(response)['version']
    released = datetime.datetime.fromisoformat(data['created_at'].replace('Z', '+00:00'))
    return {'name': name, 'version': version, 'released': data['created_at'], 'eligible': released <= cutoff and not data['yanked']}

with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    results = sorted(pool.map(check, packages), key=lambda row: (row['name'], row['version']))
report = {'as_of': str(as_of), 'cutoff': cutoff.isoformat(), 'packages': results}
(root / 'target/crate-age-report.json').write_text(json.dumps(report, indent=2) + '\n')
failed = [row for row in results if not row['eligible']]
for row in failed:
    print(f"INELIGIBLE {row['name']} {row['version']} {row['released']}")
print(f"Checked {len(results)} locked registry packages; {len(failed)} ineligible.")
sys.exit(bool(failed))
