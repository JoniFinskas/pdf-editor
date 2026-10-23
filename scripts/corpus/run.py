"""Run only after the host has completed the required Defender scan."""
import collections, json, os, pathlib, subprocess, time
if os.environ.get('PDF_EDITOR_CONTAINER') != '1': raise SystemExit(2)
counts = collections.Counter()
out = pathlib.Path('/results')
with (out/'results.jsonl').open('w') as log:
    for path in sorted(pathlib.Path('/corpus').glob('*.pdf')):
        if len(path.stem)!=64 or any(c not in '0123456789abcdef' for c in path.stem): continue
        destination = out/(path.stem+'.pdf')
        start=time.monotonic()
        try:
            result=subprocess.run(['/opt/pdf-editor/corpus-runner',str(path),str(destination)],capture_output=True,timeout=30)
            allowed={'ok','runtime','open','pages','page_limit','render','objects','find','edit','render_after','dimensions','coordinates','render_diff','save','reopen','reopen_pages','page_count','panic'}
            value=result.stdout.decode('ascii',errors='replace').strip()
            status=value if result.returncode==0 and value in allowed else 'crash'
        except subprocess.TimeoutExpired: status='timeout'
        finally: destination.unlink(missing_ok=True)
        counts[status]+=1
        log.write(json.dumps({'id':path.stem,'type':status,'seconds':round(time.monotonic()-start,3)})+'\n')
        log.flush()
print(json.dumps(dict(counts)))
