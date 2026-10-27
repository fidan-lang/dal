"""Check the actual production graph, independently of inactive Cargo.lock entries."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
result = subprocess.run(["cargo", "tree", "--workspace", "--locked", "--edges", "normal",
                         "--prefix", "none", "--format", "{p}"], cwd=root,
                        capture_output=True, text=True, encoding="utf-8", check=True)
names = {line.split()[0] for line in result.stdout.splitlines() if line}
for affected in ("crypto-bigint", "rsa"):
    assert affected not in names, f"Vulnerable optional cryptography is active: {affected}"
assert {"aws-lc-rs", "jsonwebtoken", "aws-sdk-s3"} <= names
print("Production graph uses AWS-LC JWT validation and regional S3 signing; affected RSA/SigV4a code is absent.")
