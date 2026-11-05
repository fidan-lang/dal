"""Live release hardening gate. The previous image is not an approval baseline.

Recompiled application binaries and patched OS packages are EXPECTED to differ.
The gate enforces stable structural guarantees and emits archive checksum proof.
Run Trivy as a mandatory separate step before publication.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

PROBE = r'''set -eu
printf 'PACKAGES\n'
dpkg-query -W -f='${Package}\t${Architecture}\t${Version}\t${db:Status-Status}\n'
printf 'FILES\n'
find /usr/lib/x86_64-linux-gnu /usr/local/bin -type f \( -name '*.so*' -o -path '/usr/local/bin/*' \) -exec sha256sum {} \;
printf 'LINKS\n'
find /usr/lib/x86_64-linux-gnu -type l -printf '%p\t%l\n'
printf 'MODES\n'
find /usr/local/bin -type f -printf '%p\t%m\t%U\t%G\n'
printf 'PRIVILEGES\n'
find / -xdev -type f -perm /6000 -print
'''
EXPECTED = {
    'server': {'entrypoint': ['/usr/local/bin/dal-server'],
               'health': ['/usr/local/bin/dal-healthcheck'],
               'binaries': {'/usr/local/bin/dal-server', '/usr/local/bin/dal-healthcheck'}},
    'worker': {'entrypoint': ['/usr/local/bin/dal-worker'],
               'health': ['/usr/local/bin/dal-worker', '--healthcheck'],
               'binaries': {'/usr/local/bin/dal-worker'}},
}


def command(args):
    result = subprocess.run(args, capture_output=True, text=True, timeout=90, check=False)
    if result.returncode:
        raise ValueError('Local image inspection failed (command withheld)')
    return result.stdout


def capture(image):
    meta = json.loads(command(['docker', 'image', 'inspect', image]))[0]
    if meta.get('Os') != 'linux' or meta.get('Architecture') != 'amd64':
        raise ValueError('Expected Linux AMD64 release image')
    cfg = meta.get('Config') or {}
    if (cfg.get('Labels') or {}).get('org.opencontainers.image.source'):
        raise ValueError('Private image must not be repository-linked')
    raw = command(['docker', 'run', '--rm', '--network', 'none', '--read-only',
                   '--cap-drop', 'ALL', '--cap-add', 'DAC_READ_SEARCH',
                   '--security-opt', 'no-new-privileges:true', '--user', '0:0',
                   '--memory', '256m', '--entrypoint', '/bin/sh', image, '-ec', PROBE])
    sections = {name: [] for name in ('PACKAGES', 'FILES', 'LINKS', 'MODES', 'PRIVILEGES')}
    current = None
    for line in raw.splitlines():
        if line in sections:
            current = line
        elif current is None:
            raise ValueError('Invalid runtime evidence')
        else:
            sections[current].append(line)
    if sections['PRIVILEGES']:
        raise ValueError('SUID or SGID files in release image')
    packages = {}
    for line in sections['PACKAGES']:
        name, arch, version, status = line.split('\t')
        if status == 'installed':
            packages[name+':'+arch] = version
    files = {}
    for line in sections['FILES']:
        digest, name = line.split('  ', 1)
        if not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError('Invalid runtime binary fingerprint')
        files[name] = digest
    if not packages or not files or 'libc6:amd64' not in packages:
        raise ValueError('Incomplete runtime package/library evidence')
    return meta, packages, files, sections


def validate(target, meta, packages, files, sections):
    policy = EXPECTED[target]
    cfg = meta['Config']
    if cfg.get('User') not in ('dal:dal', '10001:10001'):
        raise ValueError('Runtime must use unprivileged DAL user')
    if cfg.get('Entrypoint') != policy['entrypoint']:
        raise ValueError('Unexpected image entrypoint')
    if cfg.get('Cmd') not in (None, []):
        raise ValueError('Unexpected image command override')
    if cfg.get('WorkingDir') != '/tmp':
        raise ValueError('Unexpected runtime working directory')
    for assignment in cfg.get('Env') or []:
        name = assignment.split('=', 1)[0].upper()
        if any(marker in name for marker in ('SECRET', 'PASSWORD', 'TOKEN', 'DATABASE_URL', 'AWS_ACCESS_KEY', 'AWS_SESSION', 'PRIVATE_KEY')):
            raise ValueError('Potential credential baked into image environment')
    hc = cfg.get('Healthcheck') or {}
    test = hc.get('Test', [])
    if test != ['CMD', *policy['health']]:
        raise ValueError('Unexpected image healthcheck')
    if set(name for name in files if name.startswith('/usr/local/bin/')) != policy['binaries']:
        raise ValueError('Runtime executable set changed unexpectedly')
    if len(packages) < 10 or len(files) < len(policy['binaries']):
        raise ValueError('Incomplete runtime inventory')
    if sections['PRIVILEGES']:
        raise ValueError('Privilege-bearing files detected')
    if target == 'server' and (cfg.get('ExposedPorts') or {}) != {'8080/tcp': {}}:
        raise ValueError('Expected only DAL API container port')
    if target == 'worker' and cfg.get('ExposedPorts'):
        raise ValueError('Worker must expose no ports')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--target', required=True, choices=tuple(EXPECTED))
    p.add_argument('--image', required=True)
    p.add_argument('--archive', required=True, type=Path)
    p.add_argument('--receipt', required=True, type=Path)
    args = p.parse_args()
    meta, packages, files, sections = capture(args.image)
    validate(args.target, meta, packages, files, sections)
    with args.archive.open('rb') as stream:
        archive_sha = hashlib.file_digest(stream, 'sha256').hexdigest()
    receipt = {'target': args.target, 'validated': True,
               'archive_sha256': archive_sha,
               'installed_packages': len(packages), 'native_files': len(files)}
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    with os.fdopen(os.open(args.receipt, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as stream:
        json.dump(receipt, stream)
        stream.write('\n')
    print('Current runtime hardening and package inventory verified; no stale hash baseline')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.TimeoutExpired, json.JSONDecodeError) as e:
        raise SystemExit(str(e)) from None
