"""Download allowed Govdocs archives inside the isolated download container only."""
import hashlib, json, os, pathlib, urllib.request, zipfile

root = pathlib.Path('/corpus')
if os.environ.get('PDF_EDITOR_CONTAINER') != '1':
    raise SystemExit(2)
root.mkdir(exist_ok=True)
disk_used = sum(p.stat().st_size for p in root.glob('*.pdf'))
count = len(list(root.glob('*.pdf')))
gov_count = int((root/'govdocs-count').read_text()) if (root/'govdocs-count').exists() else 0
previous = (root/'SOURCES.md').read_text() if (root/'SOURCES.md').exists() else ''
with (root / 'SOURCES.md').open('a', encoding='utf-8') as log:
    for thread in range(100):
        if gov_count >= 3000:
            break
        url = f'https://downloads.digitalcorpora.org/corpora/files/govdocs1/zipfiles/{thread:03}.zip'
        if f'- {url} ' in previous: continue
        archive = root / 'download.zip'
        try:
            # Redirects may not expand the allowlisted download domain.
            class AllowedRedirect(urllib.request.HTTPRedirectHandler):
                def redirect_request(self, req, fp, code, msg, headers, newurl):
                    from urllib.parse import urlparse
                    parsed = urlparse(newurl)
                    if parsed.scheme != 'https' or parsed.hostname != 'downloads.digitalcorpora.org':
                        raise ValueError('redirect')
                    return super().redirect_request(req, fp, code, msg, headers, newurl)
            with urllib.request.build_opener(AllowedRedirect()).open(url, timeout=60) as response, archive.open('wb') as output:
                digest = hashlib.sha256()
                total = 0
                while chunk := response.read(1024*1024):
                    total += len(chunk)
                    if total > 2*1024**3: raise ValueError('archive_limit')
                    digest.update(chunk)
                    output.write(chunk)
            log.write(f'- {url} SHA256 `{digest.hexdigest()}`\n')
            log.flush()
            with zipfile.ZipFile(archive) as z:
                entries = z.infolist()
                if len(entries)>100000 or sum(e.file_size for e in entries)>8*1024**3: raise ValueError('expansion_limit')
                for entry in entries:
                    if not entry.filename.lower().endswith('.pdf') or entry.file_size > 64*1024**2: continue
                    if disk_used + entry.file_size > 8*1024**3: raise ValueError('disk_limit')
                    if entry.compress_size and entry.file_size/entry.compress_size > 1000: continue
                    # Only raw bytes written to digest-derived paths; archive paths/modes ignored.
                    data = z.read(entry)
                    file_id = hashlib.sha256(data).hexdigest()
                    destination = root / (file_id + '.pdf')
                    if not destination.exists():
                        destination.write_bytes(data)
                        count += 1
                        gov_count += 1
                        (root/'govdocs-count').write_text(str(gov_count))
                        disk_used += len(data)
            print(json.dumps({'archives':thread+1,'pdf_count':count}), flush=True)
        except Exception as error:
            log.write(f'- {url} skipped: {type(error).__name__}\n')
            log.flush()
        finally:
            archive.unlink(missing_ok=True)
    import tarfile
    sources = [
        ('https://pdfium.googlesource.com/pdfium/+archive/refs/heads/main/testing/resources.tar.gz', 'tar'),
        ('https://github.com/mozilla/pdf.js/archive/refs/heads/master.zip', 'zip'),
        ('https://github.com/veraPDF/veraPDF-corpus/archive/refs/heads/master.zip', 'zip'),
    ]
    for url, kind in sources:
        from urllib.parse import urlparse
        host = urlparse(url).hostname
        class ExactHostRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, newurl):
                parsed=urlparse(newurl)
                if parsed.scheme!='https' or parsed.hostname!=host:
                    raise ValueError('source_boundary')
                return super().redirect_request(req,fp,code,msg,headers,newurl)
        archive = root/'download.archive'
        try:
            digest=hashlib.sha256()
            total=0
            with urllib.request.build_opener(ExactHostRedirect()).open(url,timeout=60) as response, archive.open('wb') as output:
                while chunk:=response.read(1024*1024):
                    total+=len(chunk)
                    if total>2*1024**3: raise ValueError('archive_limit')
                    digest.update(chunk)
                    output.write(chunk)
            log.write(f'- {url} SHA256 `{digest.hexdigest()}`\n')
            def store(data):
                global count,disk_used
                if disk_used+len(data)>8*1024**3: raise ValueError('disk_limit')
                dest=root/(hashlib.sha256(data).hexdigest()+'.pdf')
                if not dest.exists():
                    dest.write_bytes(data);count+=1;disk_used+=len(data)
            if kind=='zip':
                with zipfile.ZipFile(archive) as z:
                    entries=z.infolist()
                    if len(entries)>100000 or sum(e.file_size for e in entries)>8*1024**3: raise ValueError('expansion_limit')
                    for e in entries:
                        if not e.filename.lower().endswith('.pdf') or e.file_size>64*1024**2: continue
                        if 'mozilla' in url and '/test/pdfs/' not in e.filename: continue
                        store(z.read(e))
            else:
                with tarfile.open(archive,'r:gz') as t:
                    total=0
                    for index,e in enumerate(t):
                        total+=e.size
                        if index>100000 or total>8*1024**3: raise ValueError('expansion_limit')
                        if e.isfile() and e.name.lower().endswith('.pdf') and e.size<=64*1024**2:
                            with t.extractfile(e) as f: store(f.read())
        except Exception as error:
            log.write(f'- {url} skipped: {type(error).__name__} (redirects beyond the exact source host are refused)\n')
        finally:
            archive.unlink(missing_ok=True);log.flush()
        print(json.dumps({'source':kind,'pdf_count':count}),flush=True)
if gov_count < 3000: raise SystemExit(1)

