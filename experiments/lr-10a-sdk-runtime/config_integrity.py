"""Metadata-only experimental contract. No CLI, credential or admission path.

Stable metadata is not byte equality. Unknown changes have no approval fallback.
There is no real-runtime legitimate-write contract yet. The sole proven-write
case below owns a newly created synthetic file; it accepts no external path or
caller-supplied claim of legitimacy and can never authorize host operations.
"""
import errno
import hashlib
import os
from pathlib import Path
import stat
import tempfile

LEGACY_FIELDS = frozenset(('inode', 'bytes', 'mtime_ns', 'ctime_ns'))
FIELDS = LEGACY_FIELDS | frozenset(('device', 'mode', 'uid', 'gid', 'links'))


def snapshot(path):
    """Stat only, anchored directories, no symlink traversal or content opens.

    A snapshot has a race boundary and is not continuous filesystem monitoring.
    Parent identity is checked across open. No permissions, locks or files change.
    PermissionError means metadata access denied, not unreadable file contents.
    """
    path = Path(path)
    if not path.is_absolute() or '..' in path.parts or len(path.parts) < 2:
        return {'state': 'failed', 'reason': 'invalid_path'}
    fd = None
    try:
        flags = os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        fd = os.open('/', flags)
        for name in path.parts[1:-1]:
            before = os.stat(name, dir_fd=fd, follow_symlinks=False)
            if stat.S_ISLNK(before.st_mode) or not stat.S_ISDIR(before.st_mode):
                return {'state': 'failed', 'reason': 'unexpected_path_type'}
            child = os.open(name, flags, dir_fd=fd)
            try:
                opened = os.fstat(child)
            except BaseException:
                os.close(child)
                raise
            if (before.st_dev, before.st_ino) != (opened.st_dev, opened.st_ino):
                os.close(child)
                return {'state': 'failed', 'reason': 'path_identity_changed'}
            os.close(fd)
            fd = child
        meta = os.stat(path.name, dir_fd=fd, follow_symlinks=False)
        if not stat.S_ISREG(meta.st_mode):
            return {'state': 'failed', 'reason': 'unexpected_file_type'}
        return {'state': 'observed', 'metadata': {
            'inode': meta.st_ino, 'bytes': meta.st_size,
            'mtime_ns': meta.st_mtime_ns, 'ctime_ns': meta.st_ctime_ns,
            'device': meta.st_dev, 'mode': stat.S_IMODE(meta.st_mode),
            'uid': meta.st_uid, 'gid': meta.st_gid, 'links': meta.st_nlink}}
    except InterruptedError:
        return {'state': 'interrupted', 'reason': 'observation_interrupted'}
    except OSError as error:
        if error.errno in (errno.ENOENT, errno.EACCES, errno.EPERM):
            return {'state': 'unavailable', 'reason': (
                'missing' if error.errno == errno.ENOENT else 'metadata_access_denied')}
        return {'state': 'failed', 'reason': 'metadata_query_failed'}
    finally:
        if fd is not None:
            os.close(fd)


def historical_snapshot(metadata):
    """Adapt the four fields already published, never restat a personal file."""
    if isinstance(metadata, dict) and metadata.get('state') == 'unavailable':
        return {'state': 'unavailable', 'reason': 'historical_unavailable'}
    return {'state': 'observed', 'metadata': metadata}


def classify(before, after):
    """No legitimacy flag, tolerance, auth value or filesystem event admission."""
    result = {'classification': 'VERIFICATION_FAILED', 'metadata_mutation': None,
              'changed_fields': [], 'writer_attribution': 'INCONCLUSIVE',
              'integrity_verification': 'BLOCKED', 'content_equivalence': 'NOT_VERIFIED',
              'operational_admission': 'BLOCKED', 'scope': 'metadata_only'}
    if not isinstance(before, dict) or not isinstance(after, dict):
        return result
    states = (before.get('state'), after.get('state'))
    if 'interrupted' in states:
        result.update(classification='INCONCLUSIVE', integrity_verification='INCONCLUSIVE')
        return result
    if any(s not in ('observed', 'unavailable') for s in states):
        return result
    if 'unavailable' in states:
        result['classification'] = 'METADATA_UNAVAILABLE'
        return result
    a, b = before.get('metadata'), after.get('metadata')
    if not isinstance(a, dict) or not isinstance(b, dict):
        return result
    if set(a) not in (LEGACY_FIELDS, FIELDS) or set(a) != set(b):
        return result
    if any(type(v) is not int or v < 0 for row in (a, b) for v in row.values()):
        return result
    result['coverage'] = 'historical_four_fields' if set(a) == LEGACY_FIELDS else 'extended_metadata'
    changed = sorted(k for k in a if a[k] != b[k])
    result.update(changed_fields=changed, metadata_mutation=bool(changed))
    if changed:
        result['classification'] = 'METADATA_CHANGE_UNATTRIBUTED'
    else:
        result.update(classification='OBSERVATIONALLY_STABLE',
                      integrity_verification='INCONCLUSIVE')
    return result


def independent_observations(authenticated, integrity):
    """Auth observation survives integrity failure; finance never opens here."""
    return {'sdk_auth_observed': authenticated if type(authenticated) is bool else None,
            'config': integrity, 'financial_admission': 'BLOCKED',
            'a9_real_inference': 'NOT_RUN', 'inference_calls': 0,
            'real_session_operations': 0, 'marker_claimed': False}


def prove_synthetic_update():
    """One fixed legitimate revision of a file created here, never host config.

    Retain the replacement FD across rename, check path inode against that FD,
    and validate fixed synthetic bytes through the FD. Byte hashes are acquired
    ONLY here for synthetic data. This is a cooperative fixture contract, not
    protection against an adversary with the same UID or a Copilot write policy.
    """
    with tempfile.TemporaryDirectory(prefix='narys-a9-integrity-') as directory:
        root = Path(directory)
        if stat.S_IMODE(root.stat().st_mode) != 0o700:
            raise ValueError('unsafe_synthetic_directory')
        target = root / 'synthetic-config.json'
        original, updated = b'{"revision":1}\n', b'{"revision":2}\n'
        flags = os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC
        fd = os.open(target, flags, 0o600)
        try:
            if os.write(fd, original) != len(original):
                raise ValueError('synthetic_write_incomplete')
            os.fsync(fd)
        finally:
            os.close(fd)
        before = snapshot(target)
        replacement = root / 'synthetic-replacement.json'
        fd = os.open(replacement, flags, 0o600)
        try:
            if os.write(fd, updated) != len(updated):
                raise ValueError('synthetic_write_incomplete')
            os.fsync(fd)
            os.replace(replacement, target)
            after = snapshot(target)
            observed = after.get('metadata', {})
            pinned = os.fstat(fd)
            os.lseek(fd, 0, os.SEEK_SET)
            content = os.read(fd, len(updated) + 1)
            if ((observed.get('device'), observed.get('inode')) != (pinned.st_dev, pinned.st_ino)
                    or content != updated):
                raise ValueError('synthetic_contract_not_verified')
        finally:
            os.close(fd)
        result = classify(before, after)
        if (result['classification'] != 'METADATA_CHANGE_UNATTRIBUTED'
                or 'inode' not in result['changed_fields']):
            raise ValueError('synthetic_replacement_not_observed')
        result.update(classification='LEGITIMATE_CHANGE_PROVEN',
                      writer_attribution='KNOWN_SYNTHETIC_WRITER',
                      integrity_verification='PASS_SYNTHETIC_CONTRACT',
                      content_equivalence='DIFFERENT_VERIFIED_SYNTHETIC_BYTES',
                      scope='fixed_private_synthetic_fixture',
                      proof={'owned_replacement_fd_matches_path': True,
                             'fixed_revision_verified': True,
                             'after_sha256': hashlib.sha256(content).hexdigest()})
        return result
