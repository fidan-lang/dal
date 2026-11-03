"""Compare a release image with the owner's exact-image private-publication acceptance."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BASELINE = ROOT / 'config/container-release-acceptance.json'
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=('server', 'worker'), required=True)
    parser.add_argument('--image', required=True)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    options = parser.parse_args()
    acceptance = json.loads(BASELINE.read_text())
    if acceptance.get('scope') != 'private-publication-only' or acceptance.get('owner') != 'AppSolves':
        raise ValueError('Explicit private-publication owner acceptance is missing')
    expected = acceptance['images'][options.target]
    actual = capture(options.image)
    verify(actual, expected['runtime'])
    with options.archive.open('rb') as stream:
        archive_sha256 = hashlib.file_digest(stream, 'sha256').hexdigest()
    options.receipt.parent.mkdir(parents=True, exist_ok=True)
    options.receipt.write_text(json.dumps({'target': options.target, 'equivalent': True,
        'accepted_oci_digest': expected['accepted_oci_digest'], 'archive_sha256': archive_sha256,
        'runtime_sha256': hashlib.sha256(json.dumps(actual, sort_keys=True).encode()).hexdigest(),
        'scope': acceptance['scope'], 'deployment_approved': False}, indent=2) + '\n')
    print('Accepted runtime binaries, all installed package versions, libraries and hardening match; private publication only')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, OSError, json.JSONDecodeError) as error:
        raise SystemExit(str(error)) from None
