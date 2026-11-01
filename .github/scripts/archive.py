#!/usr/bin/env python3
"""固定 Unix 发行包的顺序、权限、所有者和时间，并生成 SHA-256。"""

import argparse
from datetime import datetime, timezone
import gzip
import hashlib
from pathlib import Path
import stat
import tarfile
import uuid
import zipfile


def create_archive(source, destination, epoch):
    source = source.resolve(strict=True)
    if not source.is_dir():
        raise ValueError('打包源必须是目录')
    entries = [source, *sorted(source.rglob('*'))]
    if any(path.is_symlink() or not (path.is_file() or path.is_dir()) for path in entries):
        raise ValueError('发行文件只允许普通文件和目录')
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f'{destination.name}.{uuid.uuid4().hex}.tmp')
    try:
        if destination.name.endswith('.tar.gz'):
            with temporary.open('wb') as output:
                with gzip.GzipFile(filename='', fileobj=output, mode='wb', mtime=epoch) as compressed:
                    with tarfile.open(fileobj=compressed, mode='w') as archive:
                        for path in entries:
                            name = path.relative_to(source.parent).as_posix()
                            info = archive.gettarinfo(str(path), arcname=name)
                            info.uid = info.gid = 0
                            info.uname = info.gname = ''
                            info.mtime = epoch
                            info.mode = 0o755 if path.is_dir() or path.name == 'OpenSesame' else 0o644
                            info.pax_headers = {}
                            if path.is_file():
                                with path.open('rb') as stream:
                                    archive.addfile(info, stream)
                            else:
                                archive.addfile(info)
        elif destination.suffix == '.zip':
            timestamp = datetime.fromtimestamp(epoch, timezone.utc)
            if not 1980 <= timestamp.year <= 2107:
                raise ValueError('时间超出 ZIP 支持范围')
            with zipfile.ZipFile(temporary, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
                for path in entries:
                    name = path.relative_to(source.parent).as_posix()
                    mode = 0o755 if path.is_dir() or path.name == 'OpenSesame' else 0o644
                    info = zipfile.ZipInfo(name + ('/' if path.is_dir() else ''), timestamp.timetuple()[:6])
                    info.create_system = 3
                    info.external_attr = ((stat.S_IFDIR if path.is_dir() else stat.S_IFREG) | mode) << 16
                    info.compress_type = zipfile.ZIP_DEFLATED
                    if path.is_dir():
                        info.external_attr |= 0x10
                    archive.writestr(info, b'' if path.is_dir() else path.read_bytes())
        else:
            raise ValueError('不支持的发行包格式')
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)
    digest = hashlib.sha256()
    with destination.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    destination.with_name(destination.name + '.sha256').write_bytes(
        f'{digest.hexdigest()}  {destination.name}\n'.encode('ascii'))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('destination', type=Path)
    parser.add_argument('epoch', type=int)
    arguments = parser.parse_args()
    create_archive(arguments.source, arguments.destination, arguments.epoch)
