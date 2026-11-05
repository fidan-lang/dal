#!/usr/bin/env python3
"""Root-only, forced-command DAL deployment receiver.

Read exactly one JSON object from SSH stdin. The private GHCR token lives only in
this process memory and a root-only Docker config on /run (tmpfs).
Never call from an unprotected or pull-request-triggered GitHub workflow.
"""
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

CONF = Path('/opt/dal/compose.env')
BACKUP = Path('/opt/dal/compose.env.previous')
RUNTIME = Path('/run/dal')
LOG_ID = 'dal-deploy'
PREFIX = 'ghcr.io/fidan-lang/dal/'
DIGEST = re.compile(r'^sha256:[0-9a-f]{64}$')
SHA = re.compile(r'^[0-9a-f]{40}$')
USER = re.compile(r'^[a-zA-Z0-9_-]{1,80}$')


def run(*args, input_text=None, timeout=240):
    p = subprocess.run(args, input=input_text, text=True, capture_output=True,
                       timeout=timeout, check=False)
    if p.returncode:
        # Never print subprocess stderr/stdout since an external program might
        # inadvertently echo authentication input.
        raise RuntimeError(f'Command failed ({args[0]} {args[1] if len(args)>1 else ""}), exit={p.returncode}')
    return p.stdout.strip()


def replace_atomic(dst, data):
    with tempfile.NamedTemporaryFile(dir=dst.parent, prefix='.'+dst.name+'.', delete=False) as f:
        tmp = Path(f.name)
        try:
            os.fchmod(f.fileno(), 0o600)
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        except BaseException:
            tmp.unlink(missing_ok=True)
            raise
    os.replace(tmp, dst)


def update_image_lines(data, api, worker):
    text = data.decode('utf-8')
    replacements = {'DAL_SERVER_IMAGE': api, 'DAL_WORKER_IMAGE': worker}
    for key, value in replacements.items():
        pat = re.compile(r'(?m)^'+key+r'=.*$')
        if len(pat.findall(text)) != 1:
            raise RuntimeError(f'Expected exactly one {key} setting')
        text = pat.sub(key+'='+value, text)
    return text.encode()


def health(container, expected_image, timeout):
    image_id = run('docker','image','inspect','--format','{{.Id}}',expected_image)
    limit = time.monotonic()+timeout
    while time.monotonic() < limit:
        try:
            state = run('docker','inspect','--format','{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{end}} {{.Image}}',container)
            if state == f'running healthy {image_id}':
                return
        except RuntimeError:
            pass
        time.sleep(2)
    raise RuntimeError(f'{container} did not become healthy using expected image')


def restart_stack(api, worker):
    run('systemctl','stop','dal-worker.service', timeout=230)
    run('systemctl','stop','dal-api.service', timeout=100)
    run('systemctl','start','dal-api.service', timeout=190)
    health('dal-server-1', api, 140)
    run('systemctl','start','dal-worker.service', timeout=190)
    health('dal-worker-1', worker, 140)
    raw = run('curl','--fail','--silent','--show-error','--max-time','15',
              'http://127.0.0.1:8080/readyz')
    status = json.loads(raw)
    if status.get('status') != 'ready' or status.get('db') != 'ok':
        raise RuntimeError('DAL readiness response failed')


def parse_payload():
    raw = sys.stdin.buffer.read(8193)
    if len(raw) > 8192:
        raise ValueError('Input too large')
    obj = json.loads(raw)
    if not isinstance(obj, dict) or set(obj) != {'server_digest','worker_digest','ghcr_user','ghcr_token','release_sha','release_run'}:
        raise ValueError('Unexpected deployment request fields')
    for key in ('server_digest','worker_digest'):
        if not isinstance(obj[key], str) or not DIGEST.fullmatch(obj[key]):
            raise ValueError('Digest must be immutable sha256')
    if not isinstance(obj['release_sha'],str) or not SHA.fullmatch(obj['release_sha']):
        raise ValueError('Invalid release SHA')
    if not isinstance(obj['release_run'],int) or obj['release_run'] < 1:
        raise ValueError('Invalid release run identifier')
    if not isinstance(obj['ghcr_user'],str) or not USER.fullmatch(obj['ghcr_user']):
        raise ValueError('Invalid GHCR user')
    if (not isinstance(obj['ghcr_token'],str) or not 10 <= len(obj['ghcr_token']) <= 1000
            or '\n' in obj['ghcr_token'] or '\r' in obj['ghcr_token']):
        raise ValueError('Invalid GHCR credentials')
    return obj


def main():
    if os.geteuid() != 0:
        raise RuntimeError('Run as root through restricted sudo only')
    if os.stat(RUNTIME).st_uid != 0 or os.stat(RUNTIME).st_mode & 0o077:
        raise RuntimeError('Runtime directory must be private and root-owned')
    payload = parse_payload()
    with open('/run/lock/dal-deploy.lock','w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if not CONF.is_file() or not os.stat(CONF).st_uid == 0:
            raise RuntimeError('Missing root-owned compose environment')
        existing = CONF.read_bytes()
        previous = {}
        for line in existing.decode().splitlines():
            if line.startswith('DAL_SERVER_IMAGE='):
                previous['server'] = line.partition('=')[2]
            if line.startswith('DAL_WORKER_IMAGE='):
                previous['worker'] = line.partition('=')[2]
        if set(previous) != {'server','worker'}:
            raise RuntimeError('Existing image references missing')
        new_api = PREFIX + 'server@' + payload['server_digest']
        new_worker = PREFIX + 'worker@' + payload['worker_digest']
        revised = update_image_lines(existing, new_api, new_worker)
        print('Validated release: '+payload['release_sha'][:12]+' run '+str(payload['release_run']),flush=True)
        if revised == existing:
            print('Images already deployed; health verification only',flush=True)
            health('dal-server-1',new_api,20)
            health('dal-worker-1',new_worker,20)
            return
        if run('stat','-f','-c','%T',str(RUNTIME)) != 'tmpfs':
            raise RuntimeError('Refusing non-tmpfs temporary credentials')
        with tempfile.TemporaryDirectory(dir=RUNTIME, prefix='docker-auth-') as auth:
            os.chmod(auth,0o700)
            run('docker','--config',auth,'login','ghcr.io','-u',payload['ghcr_user'],'--password-stdin',
                input_text=payload['ghcr_token']+'\n',timeout=30)
            for image in (new_api,new_worker):
                run('docker','--config',auth,'pull',image,timeout=480)
        print('Both approved image digests pulled; transient GHCR auth deleted',flush=True)
        # A persistent backup permits explicit recovery even if the host crashes mid-deploy.
        replace_atomic(BACKUP,existing)
        run('systemctl','stop','dal-watchdog.timer',timeout=30)
        try:
            replace_atomic(CONF,revised)
            try:
                restart_stack(new_api,new_worker)
            except BaseException:
                print('Deployment unhealthy; restoring prior image references',flush=True)
                replace_atomic(CONF,existing)
                try:
                    restart_stack(previous['server'],previous['worker'])
                    print('Previous API and worker restored',flush=True)
                except BaseException:
                    print('URGENT: automatic rollback failed; inspect systemd and Docker',flush=True)
                raise
        finally:
            run('systemctl','start','dal-watchdog.timer',timeout=30)
        print('Deployment completed: both containers healthy and database ready',flush=True)


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, RuntimeError, OSError, subprocess.TimeoutExpired,
            json.JSONDecodeError, UnicodeError) as e:
        print('DAL deployment failed: '+str(e),file=sys.stderr)
        raise SystemExit(1) from None
