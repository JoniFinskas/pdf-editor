"""Download allowed Govdocs archives inside the isolated download container only."""
import hashlib, json, os, pathlib, urllib.request, zipfile

root = pathlib.Path('/corpus')
if os.environ.get('PDF_EDITOR_CONTAINER') != '1':
    raise SystemExit(2)
root.mkdir(exist_ok=True)
disk_used = sum(p.stat().st_size for p in root.glob('*.pdf'))
count = len(list(root.glob('*.pdf')))
with (root / 'SOURCES.md').open('a', encoding='utf-8') as log:
    for thread in range(100):
        if count >= 3000:
            break
        url = f'https://downloads.digitalcorpora.org/corpora/files/govdocs1/zipfiles/{thread:03}.zip'
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
                        disk_used += len(data)
            print(json.dumps({'archives':thread+1,'pdf_count':count}), flush=True)
        except Exception as error:
            log.write(f'- {url} skipped: {type(error).__name__}\n')
            log.flush()
        finally:
            archive.unlink(missing_ok=True)
    log.write('- Additional optional PDFium, pdf.js and veraPDF sources not fetched; Govdocs minimum is the download target.\n')
if count < 3000: raise SystemExit(1)

