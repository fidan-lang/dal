"""Compare a release image with the owner's exact-image private-publication acceptance."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess

ACCEPTANCE_SECRET_ENV = 'GHCR_RUNTIME_ACCEPTANCE'
RUNTIME_FIELDS = {'platform', 'config', 'packages_sha256', 'native_files_sha256',
                  'library_links_sha256', 'binary_modes_sha256', 'binaries', 'glibc_packages',
                  'installed_packages', 'native_files', 'privilege_bearing_files'}
CONFIG_FIELDS = ('User', 'Env', 'Entrypoint', 'Cmd', 'WorkingDir', 'Healthcheck', 'ExposedPorts')
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


def run(arguments):
    result = subprocess.run(arguments, capture_output=True, text=True, timeout=90)
    if result.returncode:
        raise ValueError('Local release-image inspection failed; publication blocked')
    return result.stdout


def capture(image):
    metadata = json.loads(run(['docker', 'image', 'inspect', image]))[0]
    if metadata.get('Os') != 'linux' or metadata.get('Architecture') != 'amd64':
        raise ValueError('Only the reviewed Linux AMD64 runtime is accepted')
    if (metadata['Config'].get('Labels') or {}).get('org.opencontainers.image.source'):
        raise ValueError('Private image must not acquire repository-source linkage')
    output = run(['docker', 'run', '--rm', '--network', 'none', '--read-only', '--cap-drop', 'ALL',
                  '--cap-add', 'DAC_READ_SEARCH', '--security-opt', 'no-new-privileges:true', '--user', '0:0', '--memory', '256m',
                  '--entrypoint', '/bin/sh', image, '-ec', PROBE])
    # The read-only inspector needs DAC_READ_SEARCH for _apt-owned cache directories; production capabilities remain dropped. No DAL executable is started.
    sections = {name: [] for name in ('PACKAGES', 'FILES', 'LINKS', 'MODES', 'PRIVILEGES')}
    current = None
    for line in output.splitlines():
        if line in sections:
            current = line
        elif current is None:
            raise ValueError('Unexpected image inspection output')
        else:
            sections[current].append(line)
    if sections['PRIVILEGES']:
        raise ValueError('Release image contains SUID/SGID files')
    packages = {}
    for line in sections['PACKAGES']:
        name, architecture, version, status = line.split('\t')
        if status == 'installed':
            packages[name + ':' + architecture] = version
    files = {}
    for line in sections['FILES']:
        digest, name = line.split('  ', 1)
        if len(digest) != 64:
            raise ValueError('Invalid runtime file digest')
        files[name] = digest
    if not packages or not files or not any(name.endswith('/libc.so.6') for name in files):
        raise ValueError('Runtime package/library evidence incomplete')
    return {'platform': 'linux/amd64', 'config': {key: metadata['Config'].get(key) for key in CONFIG_FIELDS},
            'packages_sha256': fingerprint(packages), 'native_files_sha256': fingerprint(files),
            'library_links_sha256': fingerprint(sorted(sections['LINKS'])),
            'binary_modes_sha256': fingerprint(sorted(sections['MODES'])),
            'binaries': {name: value for name, value in files.items() if name.startswith('/usr/local/bin/')},
            'glibc_packages': {name: value for name, value in packages.items() if name in ('libc6:amd64', 'libc-bin:amd64')},
            'installed_packages': len(packages), 'native_files': len(files), 'privilege_bearing_files': 0}


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def verify(actual, accepted):
    differences = sorted(key for key in accepted if actual.get(key) != accepted[key])
    if set(actual) != set(accepted) or differences:
        # Report field names only; never dump environment values or provider errors.
        raise ValueError('Release exposure differs from accepted images: ' + ', '.join(differences or ['schema']))


def load_acceptance():
    # Consume the step-scoped secret before starting any Docker subprocess.
    raw = os.environ.pop(ACCEPTANCE_SECRET_ENV, '')
    try:
        value = json.loads(raw)
        if (value.get('scope') != 'private-publication-only' or value.get('owner') != 'AppSolves'
                or value.get('deployment_approved') is not False
                or set(value['images']) != {'server', 'worker'}):
            raise ValueError()
        cves = value['accepted_glibc_cves']
        if not isinstance(cves, list) or not cves or len(set(cves)) != len(cves):
            raise ValueError()
        if not all(isinstance(cve, str) and re.fullmatch(r'CVE-[0-9]{4}-[0-9]{4,}', cve) for cve in cves):
            raise ValueError()
        for image in value['images'].values():
            if not re.fullmatch(r'sha256:[a-f0-9]{64}', image['accepted_oci_digest']):
                raise ValueError()
            runtime = image['runtime']
            if (set(runtime) != RUNTIME_FIELDS or runtime['platform'] != 'linux/amd64'
                    or runtime['config']['User'] not in ('dal:dal', '10001:10001')
                    or type(runtime['privilege_bearing_files']) is not int
                    or runtime['privilege_bearing_files'] != 0):
                raise ValueError()
            for name in ('packages_sha256', 'native_files_sha256', 'library_links_sha256', 'binary_modes_sha256'):
                if not re.fullmatch(r'[a-f0-9]{64}', runtime[name]):
                    raise ValueError()
            for name in ('installed_packages', 'native_files'):
                if type(runtime[name]) is not int or runtime[name] <= 0:
                    raise ValueError()
            if not runtime['binaries'] or not runtime['glibc_packages']:
                raise ValueError()
    except (ValueError, KeyError, TypeError, AttributeError):
        raise ValueError('Protected GHCR_RUNTIME_ACCEPTANCE secret is missing or invalid; publication blocked') from None
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--validate-acceptance', action='store_true')
    parser.add_argument('--target', choices=('server', 'worker'))
    parser.add_argument('--image')
    parser.add_argument('--archive', type=Path)
    parser.add_argument('--receipt', type=Path)
    options = parser.parse_args()
    acceptance = load_acceptance()
    if options.validate_acceptance:
        if any((options.target, options.image, options.archive, options.receipt)):
            parser.error('--validate-acceptance cannot be combined with image inspection')
        print('Protected runtime acceptance is configured and valid')
        return
    if not all((options.target, options.image, options.archive, options.receipt)):
        parser.error('--target, --image, --archive and --receipt are required for image inspection')
    expected = acceptance['images'][options.target]
    actual = capture(options.image)
    verify(actual, expected['runtime'])
    with options.archive.open('rb') as stream:
        archive_sha256 = hashlib.file_digest(stream, 'sha256').hexdigest()
    options.receipt.parent.mkdir(parents=True, exist_ok=True)
    receipt = {'target': options.target, 'equivalent': True, 'archive_sha256': archive_sha256,
               'scope': acceptance['scope'], 'deployment_approved': False}
    # This private transport checksum is never included in public Actions artifacts.
    with os.fdopen(os.open(options.receipt, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as stream:
        stream.write(json.dumps(receipt, indent=2) + '\n')
    print('Accepted runtime binaries, all installed package versions, libraries and hardening match; private publication only')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, OSError, json.JSONDecodeError) as error:
        raise SystemExit(str(error)) from None
