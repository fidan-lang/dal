"""VPS credential acceptance: offline packaging checks by default; --live performs read-only AWS probes.

Never print/store credential_process stdout. No production messages are sent or received.
"""
import argparse
import configparser
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
from datetime import datetime, timezone

import yaml

ROOT = Path(__file__).resolve().parents[1]
SETTINGS = json.loads((ROOT / "deploy/roles-anywhere.json").read_text())


def run(args, *, env=None, allowed=(0,)):
    process = subprocess.run([str(arg) for arg in args], cwd=ROOT, env=env,
                             capture_output=True, text=True, timeout=120)
    if process.returncode not in allowed:
        # stderr may contain credential_process diagnostics. Do not relay it.
        code = re.search(r"An error occurred \(([A-Za-z0-9_.-]+)\)", process.stderr)
        category = code[1] if code else next((name for name, marker in [
            ('TLS', 'CERTIFICATE_VERIFY_FAILED'), ('network', 'Could not connect'),
            ('timeout', 'timed out'), ('proxy', 'proxy')]
            if marker.lower() in process.stderr.lower()), 'unclassified')
        raise AssertionError(f"{Path(str(args[0])).name}: unexpected exit {process.returncode} ({category}); output withheld")
    return process


def validate_bundle(bundle, resources):
    assert resources['account'] == SETTINGS['account'] and resources['region'] == SETTINGS['region']
    override = yaml.safe_load((ROOT / 'deploy/compose.roles-anywhere.yaml').read_text())
    for name, service in [('api', 'server'), ('worker', 'worker')]:
        settings = SETTINGS['workloads'][name]
        cfg = configparser.ConfigParser(interpolation=None)
        cfg.read(bundle / name / 'config')
        assert cfg.sections() == ['profile ' + settings['profile']]
        selected = cfg[cfg.sections()[0]]
        assert selected['region'] == SETTINGS['region']
        command = selected['credential_process']
        assert '--profile-arn ' + resources['profiles'][name] in command
        assert '--trust-anchor-arn ' + resources['trust_anchor_arn'] in command
        assert '--role-arn arn:aws:iam::' + SETTINGS['account'] + ':role/' + settings['role'] in command
        assert '--session-duration ' + str(SETTINGS['session_seconds']) in command
        for forbidden in ['--no-verify-ssl', '--debug', '--endpoint', '--with-proxy']:
            assert forbidden not in command
        environment = override['services'][service]['environment']
        assert environment['AWS_PROFILE'] == settings['profile']
        for key in ['AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'AWS_SESSION_TOKEN']:
            assert environment[key] == '', 'Alternative credentials must not override the selected profile'
        mounts = override['services'][service]['volumes']
        assert len(mounts) == 2 and all(m['read_only'] for m in mounts)
        assert all(m['bind']['create_host_path'] is False for m in mounts)
        assert not any('ca' in m['target'] for m in mounts)
        assert (bundle / name / 'certificate.pem').is_file()
        assert (bundle / name / 'private-key.pem').is_file()
    for platform, filename in [('linux_amd64', 'aws_signing_helper'), ('windows_amd64', 'aws_signing_helper.exe')]:
        assert hashlib.sha256((bundle / 'helper' / filename).read_bytes()).hexdigest() == SETTINGS['helper'][platform]['sha256']
    assert not list(bundle.rglob('*.pfx')) and not list(bundle.rglob('*.dpapi'))
    for secret in bundle.rglob('private-key.pem'):
        assert run(['git', 'check-ignore', str(secret)]).returncode == 0, 'Workload key is not ignored by Git'


def clean_env():
    return {key: value for key, value in os.environ.items() if not key.startswith('AWS_')}


def helper_args(bundle, resources, certificate, workload):
    return ['credential-process', '--certificate', bundle / certificate / 'certificate.pem',
            '--private-key', bundle / certificate / 'private-key.pem',
            '--trust-anchor-arn', resources['trust_anchor_arn'],
            '--profile-arn', resources['profiles'][workload],
            '--role-arn', 'arn:aws:iam::' + SETTINGS['account'] + ':role/' + SETTINGS['workloads'][workload]['role'],
            '--session-duration', str(SETTINGS['session_seconds'])]


def verify_credentials(process, workload):
    credentials = json.loads(process.stdout)
    assert credentials['Version'] == 1
    assert credentials['AccessKeyId'].startswith('ASIA') and credentials['SecretAccessKey'] and credentials['SessionToken']
    expiry = datetime.fromisoformat(credentials['Expiration'].replace('Z', '+00:00'))
    remaining = (expiry - datetime.now(timezone.utc)).total_seconds()
    assert 600 < remaining <= SETTINGS['session_seconds'] + 60
    env = clean_env()
    env.update(AWS_ACCESS_KEY_ID=credentials['AccessKeyId'], AWS_SECRET_ACCESS_KEY=credentials['SecretAccessKey'],
               AWS_SESSION_TOKEN=credentials['SessionToken'], AWS_EC2_METADATA_DISABLED='true', AWS_DEFAULT_REGION=SETTINGS['region'])
    identity = json.loads(run(['aws', 'sts', 'get-caller-identity', '--output', 'json'], env=env).stdout)
    assert identity['Account'] == SETTINGS['account']
    assert identity['Arn'].startswith('arn:aws:sts::' + SETTINGS['account'] + ':assumed-role/' + SETTINGS['workloads'][workload]['role'] + '/')
    return env


def live_checks(bundle, resources, images, rust_probe):
    executable = bundle / 'helper' / 'aws_signing_helper.exe'
    assert os.name == 'nt', 'Run the administrator smoke on Windows; Linux container probes follow.'
    for workload in SETTINGS['workloads']:
        env = verify_credentials(run([executable, *helper_args(bundle, resources, workload, workload)], env=clean_env()), workload)
        if workload == 'worker':
            result = run(['aws', 'sqs', 'get-queue-attributes', '--queue-url', f"https://sqs.{SETTINGS['region']}.amazonaws.com/{SETTINGS['account']}/{SETTINGS['queue_name']}", '--attribute-names', 'QueueArn', '--output', 'json'], env=env)
            assert json.loads(result.stdout)['Attributes']['QueueArn'].endswith(':' + SETTINGS['queue_name'])
        else:
            # An intentionally nonexistent username proves AdminGetUser authorization without touching a user.
            result = run(['aws', 'cognito-idp', 'admin-get-user', '--user-pool-id', SETTINGS['user_pool_id'], '--username', 'dal-readonly-credential-probe-nonexistent-84c9a91d'], env=env, allowed=(254, 255))
            assert 'UserNotFoundException' in result.stderr, 'Cognito permission probe was not authorized'
        if workload == 'api':
            forbidden = ['aws', 'sqs', 'get-queue-attributes', '--queue-url', f"https://sqs.{SETTINGS['region']}.amazonaws.com/{SETTINGS['account']}/{SETTINGS['queue_name']}", '--attribute-names', 'QueueArn']
        else:
            forbidden = ['aws', 'cognito-idp', 'admin-get-user', '--user-pool-id', SETTINGS['user_pool_id'], '--username', 'dal-readonly-credential-probe-nonexistent-84c9a91d']
        denied = run(forbidden, env=env, allowed=(254, 255))
        assert 'AccessDenied' in denied.stderr, 'Unapproved service operation must be denied'
        print('PASS Windows helper / STS identity / read-only service authorization:', workload)
        if rust_probe:
            command = ' '.join('"' + str(part) + '"' for part in [executable, *helper_args(bundle, resources, workload, workload)])
            local_config = bundle.parent / ('vps-aws-smoke-' + workload + '.config')
            local_config.write_text('[profile ' + SETTINGS['workloads'][workload]['profile'] + ']\nregion=' + SETTINGS['region'] + '\ncredential_process=' + command + '\n', encoding='utf-8')
            sdk_env = clean_env()
            sdk_env.update(AWS_PROFILE=SETTINGS['workloads'][workload]['profile'], AWS_CONFIG_FILE=str(local_config), AWS_SHARED_CREDENTIALS_FILE=os.devnull, AWS_EC2_METADATA_DISABLED='true')
            assert run([rust_probe], env=sdk_env).stdout.strip() == 'SDK credential_process: temporary credentials loaded; expiry present'
            print('PASS actual Rust SDK default credential_process provider:', workload)
    for certificate, workload in [('api', 'worker'), ('worker', 'api')]:
        denied = run([executable, *helper_args(bundle, resources, certificate, workload)], env=clean_env(), allowed=tuple(range(1, 256)))
        assert not denied.stdout.strip(), 'Denied helper returned data'
        assert 'AccessDenied' in denied.stderr, 'Cross-workload denial was not an authorization failure'
        print('PASS certificate cannot assume other workload role:', certificate)
    for workload, image in zip(('api', 'worker'), images):
        command = ['docker', 'run', '--rm', '--read-only', '--user', '10001:10001', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges', '--memory', '128m', '--cpus', '1',
                   '--mount', f"type=bind,src={bundle / 'helper' / 'aws_signing_helper'},dst=/opt/aws_signing_helper,readonly",
                   '--mount', f"type=bind,src={bundle / workload},dst=/run/dal-aws,readonly", '--entrypoint', '/opt/aws_signing_helper', image,
                   'credential-process', '--certificate', '/run/dal-aws/certificate.pem', '--private-key', '/run/dal-aws/private-key.pem',
                   '--trust-anchor-arn', resources['trust_anchor_arn'], '--profile-arn', resources['profiles'][workload],
                   '--role-arn', 'arn:aws:iam::' + SETTINGS['account'] + ':role/' + SETTINGS['workloads'][workload]['role'], '--session-duration', str(SETTINGS['session_seconds'])]
        verify_credentials(run(command, env=clean_env()), workload)
        print('PASS Linux AMD64 helper in non-root/read-only DAL image:', workload)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--resources', type=Path, required=True)
    parser.add_argument('--live', action='store_true')
    parser.add_argument('--rust-probe', type=Path)
    parser.add_argument('--images', nargs=2, default=['dal-server:freeze-local', 'dal-worker:freeze-local'])
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    resources = json.loads(args.resources.read_text(encoding='utf-8-sig'))
    validate_bundle(bundle, resources)
    print('PASS isolated workload configuration, helper hashes, immutable mounts and Git exclusion')
    if args.live:
        live_checks(bundle, resources, args.images, args.rust_probe.resolve() if args.rust_probe else None)


if __name__ == '__main__':
    main()
