#!/usr/bin/env bash
# Checks cited names and repository paths in an explicit document allowlist.
# This checks references, not the truth of a security claim or test results.
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
from pathlib import Path
import re
import sys

documents = [
    Path('README.md'), Path('SECURITY.md'), Path('THREAT_MODEL.md'),
    Path('docs/PROJECT.md'), Path('docs/ARCHITECTURE.md'),
    Path('docs/TESTNET.md'), Path('docs/spikes/R1.md'),
    Path('docs/OPERATIONAL-READINESS.md'),
    Path('web/site/index.html'), Path('web/dashboard/index.html'),
]
excluded_directories = {
    '.git', 'target', 'node_modules', '.aws', '.vercel', '.gh', '.ssh',
    '.config', '.near-credentials', 'credentials', 'keys',
}
excluded_name_parts = ('key', 'secret', 'token', 'credential', 'mnemonic', 'seed', 'private')

def allowed(path):
    name = path.name.lower()
    return not (
        any(part.lower() in excluded_directories for part in path.parts)
        or name == '.env' or name.startswith('.env.') or name.endswith('.env')
        or any(part in name for part in excluded_name_parts)
    )

def sources(directory):
    for path in sorted(directory.iterdir()):
        if not allowed(path) or path.is_symlink() or path == Path('contracts/vault/tests/common/mod.rs'):
            continue
        if path.is_dir():
            yield from sources(path)
        elif path.suffix in ('.rs', '.sh'):
            yield path

texts = [path.read_text() for directory in ('contracts', 'agent', 'keeper', 'scripts', 'spikes')
         for path in sources(Path(directory))]
identifiers = set(re.findall(r'\b[A-Za-z_][A-Za-z0-9_]*\b', '\n'.join(texts)))
external = {'execute_intents', 'auth_by_predecessor_id', 'version_key'}
missing = []
skipped = set()
for document in documents:
    text = document.read_text()
    names = set(re.findall(r'`([a-z][a-z0-9]*(?:_[a-z0-9]+)+)`', text))
    if document.suffix == '.html':
        names.update(re.findall(r'test:\s*"([a-z][a-z0-9_]+)"', text))
    for name in sorted(names):
        if name not in identifiers and name not in external:
            missing.append(f'{document}: missing identifier: {name}')
    for path_text in sorted(set(re.findall(r'`((?:scripts|docs|contracts|agent|keeper|spikes|web)/[A-Za-z0-9_./-]+)`', text))):
        path = Path(path_text)
        if not allowed(path):
            skipped.add(path_text)
        elif document == Path('docs/spikes/R1.md') and path_text.startswith('contracts/defuse/'):
            # R1 labels these as source paths in the upstream near/intents repository.
            continue
        elif not path.exists():
            missing.append(f'{document}: missing path: {path_text}')
for path in sorted(skipped):
    print(f'skipped: {path}')
for message in missing:
    print(message)
if missing:
    sys.exit(1)
print('Allowed local references checked. External citations and security claims are not verified; no tests were run.')
PY
