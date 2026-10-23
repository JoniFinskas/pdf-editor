"""Container-only supervisor; no native diagnostics or document strings escape."""
import collections, json, os, pathlib, subprocess, time
if os.environ.get('PDF_EDITOR_CONTAINER') != '1': raise SystemExit(2)
counts=collections.Counter()
out=pathlib.Path('/results')
failures=None
if os.environ.get('CORPUS_FAILURES_ONLY')=='1':
    failures={v['id'] for v in (json.loads(x) for x in (out/'results.jsonl').read_text().splitlines()) if v['type']!='ok'}
name='failures.jsonl' if failures is not None else 'results.jsonl'
allowed={'runtime','open','pages','page_limit','render','objects','find','edit','render_after','dimensions','coordinates','render_diff','save','reopen','reopen_pages','page_count','panic'}
with (out/name).open('w') as log:
    for path in sorted(pathlib.Path('/corpus').glob('*.pdf')):
        if len(path.stem)!=64 or any(c not in '0123456789abcdef' for c in path.stem): continue
        if failures is not None and path.stem not in failures: continue
        destination=out/(path.stem+'.pdf')
        start=time.monotonic()
        errors=[]
        try:
            result=subprocess.run(['/opt/pdf-editor/corpus-runner',str(path),str(destination)],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=30)
            value=result.stdout.decode('ascii',errors='replace').strip()
            if result.returncode!=0: errors=[('crash',0)]
            elif value!='ok:0':
                for line in value.splitlines():
                    parts=line.split(':')
                    if len(parts)!=2 or parts[0] not in allowed or not parts[1].isdigit() or len(parts[1])>20:
                        errors=[('crash',0)];break
                    errors.append((parts[0],int(parts[1])))
                if not errors: errors=[('crash',0)]
        except subprocess.TimeoutExpired: errors=[('timeout',0)]
        finally: destination.unlink(missing_ok=True)
        status='failed' if errors else 'ok'
        counts[status]+=1
        for stage,signature in set(errors): counts[f'{stage}:{signature}']+=1
        log.write(json.dumps({'id':path.stem,'type':status,'errors':errors,'seconds':round(time.monotonic()-start,3)})+'\n')
        log.flush()
print(json.dumps(dict(counts)))
