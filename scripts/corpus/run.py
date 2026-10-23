"""Run only after the host has completed the required Defender scan."""
import collections, json, os, pathlib, subprocess, time
if os.environ.get('PDF_EDITOR_CONTAINER') != '1': raise SystemExit(2)
counts = collections.Counter()
out = pathlib.Path('/results')
failures=None
if os.environ.get('CORPUS_FAILURES_ONLY')=='1':
    failures={v['id'] for v in (json.loads(x) for x in (out/'results.jsonl').read_text().splitlines()) if v['type']!='ok'}
name='failures.jsonl' if failures is not None else 'results.jsonl'
with (out/name).open('w') as log:
    for path in sorted(pathlib.Path('/corpus').glob('*.pdf')):
        if len(path.stem)!=64 or any(c not in '0123456789abcdef' for c in path.stem): continue
        if failures is not None and path.stem not in failures: continue
        destination = out/(path.stem+'.pdf')
        start=time.monotonic()
        signature=0
        try:
            result=subprocess.run(['/opt/pdf-editor/corpus-runner',str(path),str(destination)],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=30)
            allowed={'ok','runtime','open','pages','page_limit','render','objects','find','edit','render_after','dimensions','coordinates','render_diff','save','reopen','reopen_pages','page_count','panic'}
            value=result.stdout.decode('ascii',errors='replace').strip()
            parts=value.split(':')
            signature=int(parts[1]) if len(parts)==2 and parts[1].isdigit() and len(parts[1])<=20 else 0
            value=parts[0] if len(parts)==2 else ''
            status=value if result.returncode==0 and value in allowed else 'crash'
        except subprocess.TimeoutExpired: status='timeout'
        finally: destination.unlink(missing_ok=True)
        counts[(status,signature)]+=1
        log.write(json.dumps({'id':path.stem,'type':status,'signature':signature,'seconds':round(time.monotonic()-start,3)})+'\n')
        log.flush()
print(json.dumps({f'{k[0]}:{k[1]}':v for k,v in counts.items()}))
