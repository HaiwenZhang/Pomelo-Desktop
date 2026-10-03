"""Build the Windows x64 release executable and Inno Setup installer."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--iscc', type=Path, help='Path to Inno Setup 6 ISCC.exe')
    parser.add_argument('--toolchain', help='Rust toolchain override, e.g. stable')
    parser.add_argument('--offline', action='store_true')
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    if sys.platform != 'win32':
        parser.error('Run this script on Windows with the MSVC Rust toolchain.')
    candidates = [args.iscc] if args.iscc else [
        shutil.which('ISCC.exe'),
        Path(os.environ.get('ProgramFiles(x86)', 'C:/Program Files (x86)')) / 'Inno Setup 6/ISCC.exe',
        Path(os.environ.get('LOCALAPPDATA', 'C:/Users/Default/AppData/Local')) / 'Programs/Inno Setup 6/ISCC.exe',
    ]
    iscc = next((Path(p).resolve() for p in candidates if p and Path(p).is_file()), None)
    if iscc is None:
        parser.error('Install Inno Setup 6 (https://jrsoftware.org/isinfo.php), or pass --iscc PATH.')
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
    command = [sys.executable, str(ROOT / 'scripts/cargo.py')]
    if args.toolchain:
        command.append(f'+{args.toolchain}')
    command.extend(['build', '-p', 'pomelo', '--release', '--locked', '--target',
                    'x86_64-pc-windows-msvc', '--message-format=json-render-diagnostics'])
    if args.offline:
        command.append('--offline')
    executable = icon = None
    build_outputs = {}
    executable_package = None
    with subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, text=True, encoding='utf-8') as process:
        for line in process.stdout:
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                print(line, end='')
                continue
            package = message.get('package_id', '')
            if message.get('reason') == 'build-script-executed':
                build_outputs[package] = Path(message['out_dir'])
            if message.get('reason') == 'compiler-artifact' and message.get('target', {}).get('name') == 'pomelo' and message.get('executable'):
                executable = Path(message['executable'])
                executable_package = package
            if message.get('reason') == 'compiler-message':
                print(message['message'].get('rendered', ''), end='', file=sys.stderr)
        if process.wait() != 0:
            return 1
    if executable_package in build_outputs:
        icon = build_outputs[executable_package] / 'pomelo.ico'
    if executable is None or icon is None or not executable.is_file() or not icon.is_file():
        raise RuntimeError('Cargo did not produce the Pomelo executable and icon.')
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    subprocess.run([str(iscc), f'/DAppVersion={version}', f'/DBuildDir={executable.parent}',
                    f'/DIconFile={icon}', f'/DOutputDir={output}',
                    str(ROOT / 'scripts/windows/pomelo.iss')], cwd=ROOT, check=True)
    print(f"Installer: {output / f'Pomelo-{version}-windows-x64-setup.exe'}")
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)

