"""Measure release text preparation in separate sequential processes.

Wall time includes process startup, BRD import, fonts, geometry, and teardown.
This is not a GPU benchmark and does not claim a cold filesystem cache.
"""
import argparse
import ctypes
import hashlib
import json
import platform
import os
import re
import subprocess
import time
from pathlib import Path


def run_probe(command, measure_memory):
    if not measure_memory:
        return subprocess.run(command, capture_output=True, text=True, encoding='utf-8',
                              errors='replace', timeout=300), None

    class Counters(ctypes.Structure):
        _fields_ = [('cb', ctypes.c_ulong), ('PageFaultCount', ctypes.c_ulong)] + [
            (name, ctypes.c_size_t) for name in (
                'PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
                'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage', 'QuotaNonPagedPoolUsage',
                'PagefileUsage', 'PeakPagefileUsage', 'PrivateUsage')]

    query = ctypes.WinDLL('psapi', use_last_error=True).GetProcessMemoryInfo
    query.argtypes = [ctypes.c_void_p, ctypes.POINTER(Counters), ctypes.c_ulong]
    query.restype = ctypes.c_int
    memory = {'method': 'GetProcessMemoryInfo sampled every 10 ms', 'samples': 0,
              'peak_working_set_bytes': 0, 'peak_private_commit_bytes': 0,
              'whole_lifetime_peak_verified': False}
    with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          text=True, encoding='utf-8', errors='replace') as process:
        deadline = time.monotonic() + 300
        while True:
            counters = Counters()
            counters.cb = ctypes.sizeof(counters)
            # CPython owns this Windows process handle until the Popen context exits.
            if query(int(process._handle), ctypes.byref(counters), counters.cb):
                memory['samples'] += 1
                memory['peak_working_set_bytes'] = max(memory['peak_working_set_bytes'], counters.PeakWorkingSetSize)
                memory['peak_private_commit_bytes'] = max(memory['peak_private_commit_bytes'], counters.PeakPagefileUsage)
            try:
                stdout, stderr = process.communicate(timeout=0.01)
                break
            except subprocess.TimeoutExpired:
                if time.monotonic() >= deadline:
                    process.kill()
                    process.communicate()
                    raise subprocess.TimeoutExpired(command, 300)
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
    if not memory['samples']:
        raise RuntimeError('WINDOWS_MEMORY_COUNTERS_UNAVAILABLE')
    return result, memory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--board', type=Path, required=True)
    parser.add_argument('--encoding', default='utf-8')
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--measure-memory', action='store_true')
    args = parser.parse_args()
    if args.measure_memory and os.name != 'nt':
        parser.error('--measure-memory currently requires Windows CPython')
    if not 3 <= args.runs <= 20:
        parser.error('--runs must be between 3 and 20')
    binary = args.binary.resolve(strict=True)
    board = args.board.resolve(strict=True)
    results = []
    baseline = None
    for index in range(args.runs):
        started = time.perf_counter()
        result, memory = run_probe([str(binary), str(board), args.encoding], args.measure_memory)
        elapsed = time.perf_counter() - started
        matched = re.fullmatch(
            r'TEXT_PREPARED objects=(\d+) prepared_objects=(\d+) skipped_objects=(\d+) instances=(\d+) layers=(\d+)',
            result.stdout.strip(),
        )
        counts = dict(zip(('objects', 'prepared_objects', 'skipped_objects', 'instances', 'layers'),
                          map(int, matched.groups()))) if matched else None
        consistent = counts is not None and (baseline is None or counts == baseline)
        if baseline is None:
            baseline = counts
        row = {'run': index + 1, 'elapsed_seconds': elapsed,
               'exit_code': result.returncode, 'stdout': result.stdout.strip(),
               'stderr': result.stderr.strip(), 'counts': counts,
               'valid_consistent_statistics': consistent}
        row['memory'] = memory
        results.append(row)
        print(json.dumps(row, ensure_ascii=False))
        if result.returncode or not consistent:
            break
    report = {
        'schema_version': 1, 'scope': 'cpu_text_prepare_process_wall_time',
        'platform': platform.platform(), 'binary': str(binary),
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'board': str(board), 'board_bytes': board.stat().st_size,
        'board_sha256': hashlib.sha256(board.read_bytes()).hexdigest(),
        'encoding': args.encoding, 'requested_runs': args.runs,
        'cache_state': 'uncontrolled; sequential repeated reads',
        'gpu_verified': False, 'peak_memory_measured': args.measure_memory, 'runs': results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    if len(results) != args.runs or any(row['exit_code'] or not row['valid_consistent_statistics'] for row in results):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
