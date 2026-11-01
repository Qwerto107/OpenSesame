#!/usr/bin/env python3
"""检查 Unix 发行程序的架构、系统依赖及隔离启动。"""

import argparse
import os
from pathlib import Path
import plistlib
import struct
import subprocess
import tempfile
import time
import json

ROOT = Path(__file__).resolve().parents[2]
MACHINES = {'linux-amd64': 62, 'linux-arm64': 183,
            'darwin-amd64': 0x01000007, 'darwin-arm64': 0x0100000c}


def check_binary(platform, smoke_test=False):
    if platform.startswith('linux-'):
        executable = ROOT / 'dist/OpenSesame'
        header = executable.read_bytes()[:64]
        if header[:6] != b'\x7fELF\x02\x01' or struct.unpack_from('<H', header, 18)[0] != MACHINES[platform]:
            raise RuntimeError('Linux 程序格式或架构错误')
        dependencies = subprocess.run(['ldd', str(executable)], capture_output=True, text=True, check=True)
        if 'not found' in dependencies.stdout:
            raise RuntimeError(dependencies.stdout)
    else:
        extension = '.zip'
        executable = ROOT / 'dist/OpenSesame'
        header = executable.read_bytes()[:32]
        if header[:4] != b'\xcf\xfa\xed\xfe' or struct.unpack_from('<I', header, 4)[0] != MACHINES[platform]:
            raise RuntimeError('macOS 程序格式或架构错误')
        dependencies = subprocess.run(['otool', '-L', str(executable)], capture_output=True, text=True, check=True)
        for line in dependencies.stdout.splitlines()[1:]:
            library = line.strip().split(' (', 1)[0]
            if not library.startswith(('/System/Library/', '/usr/lib/')):
                raise RuntimeError(f'发现非系统运行库依赖：{library}')
        # 从发行 ZIP 取出真正交付的 app，校验签名并用它进行启动测试。
        import zipfile
        with tempfile.TemporaryDirectory(prefix='opensesame-app-check-') as folder:
            archive = ROOT / f'dist/release/opensesame-{platform}{extension}'
            with zipfile.ZipFile(archive) as package:
                base = Path(folder).resolve()
                for entry in package.infolist():
                    path = (base / entry.filename).resolve()
                    if base not in path.parents:
                        raise RuntimeError('发行包路径越界')
                    package.extract(entry, base)
                    if not entry.is_dir():
                        path.chmod((entry.external_attr >> 16) & 0o777)
            app = base / f'opensesame-{platform}/OpenSesame.app'
            with (app / 'Contents/Info.plist').open('rb') as stream:
                info = plistlib.load(stream)
            if info['CFBundleExecutable'] != 'OpenSesame' or info['CFBundleIconFile'] != 'OpenSesame.icns':
                raise RuntimeError('app 元数据错误')
            if not info.get('NSLocalNetworkUsageDescription'):
                raise RuntimeError('app 缺少本地网络访问说明')
            if (app / 'Contents/Resources/OpenSesame.icns').read_bytes()[:4] != b'icns':
                raise RuntimeError('app 图标错误')
            subprocess.run(['codesign', '--verify', '--strict', str(app)], check=True)
            if smoke_test:
                check_startup(app / 'Contents/MacOS/OpenSesame')
        print('macOS 架构、系统依赖、图标及 app 签名检查通过')
        return
    print('Linux 架构及运行库检查通过')
    if smoke_test:
        check_startup(executable)


def check_startup(executable):
    with tempfile.TemporaryDirectory(prefix='opensesame-startup-') as folder:
        root = Path(folder)
        for language in ('zh-CN', 'en'):
            env = os.environ.copy()
            env['SLINT_BACKEND'] = 'winit-software'
            if os.uname().sysname == 'Darwin':
                env['HOME'] = str(root / language)
                directory = Path(env['HOME']) / 'Library/Application Support/OpenSesame'
            else:
                env['XDG_CONFIG_HOME'] = str(root / language)
                directory = Path(env['XDG_CONFIG_HOME']) / 'OpenSesame'
            directory.mkdir(parents=True)
            (directory / 'config.json').write_text(json.dumps({
                'version': 1, 'servers': [],
                'settings': {'language': language, 'auto_knock': False, 'auto_check': False}
            }), encoding='utf-8')
            with (root / 'stderr.log').open('w+b') as errors:
                process = subprocess.Popen([str(executable)], cwd=root, env=env, stdout=subprocess.DEVNULL, stderr=errors)
                try:
                    time.sleep(4)
                    if process.poll() is not None:
                        raise RuntimeError(f'{language} 程序提前退出：{process.returncode}')
                    errors.seek(0)
                    detail = errors.read().decode('utf-8', errors='replace')
                    if detail.strip():
                        raise RuntimeError(detail)
                finally:
                    if process.poll() is None:
                        process.terminate()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
        print('中英文发行程序隔离启动检查通过')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=tuple(MACHINES))
    parser.add_argument('--smoke-test', action='store_true')
    arguments = parser.parse_args()
    check_binary(arguments.platform, arguments.smoke_test)
