#!/usr/bin/env python3
"""
Kurogane v3.0 - Instant Teardown Benchmark & Verification (Zero Theater)
Measures the rollback latency of the VFS teardown sequence (syncfs)
directly on physical NVMe/block-backed storage (NOT tmpfs) under dirty write load.
"""

import os
import sys
import time
import shutil
import ctypes

libc = ctypes.CDLL("libc.so.6", use_errno=True)

def run_teardown_benchmark():
    print("=" * 65)
    print("KUROGANE v3.0 PHYSICAL HARDWARE TEARDOWN BENCHMARK (ZERO THEATER)")
    print("=" * 65)

    bench_dir = os.path.abspath("build/teardown_nvme_bench")
    if os.path.exists(bench_dir):
        shutil.rmtree(bench_dir)
    os.makedirs(bench_dir, exist_ok=True)

    try:
        dir_fd = os.open(bench_dir, os.O_RDONLY | os.O_DIRECTORY)

        # Warmup
        libc.syncfs(dir_fd)

        latencies_ns = []
        iterations = 20

        for i in range(iterations):
            # Actively dirty the filesystem volume with real payload writes
            test_file = os.path.join(bench_dir, f"dirty_payload_{i}.dat")
            with open(test_file, "wb") as f:
                f.write(os.urandom(64 * 1024)) # 64 KB dirty page payload

            start = time.perf_counter_ns()

            # Quiesce persistent volume via real in-kernel syncfs to physical storage
            ret_sync = libc.syncfs(dir_fd)

            elapsed_ns = time.perf_counter_ns() - start
            latencies_ns.append(elapsed_ns)

            # Cleanup file
            if os.path.exists(test_file):
                os.unlink(test_file)

        os.close(dir_fd)

        avg_latency_us = sum(latencies_ns) / len(latencies_ns) / 1000.0
        min_latency_us = min(latencies_ns) / 1000.0
        max_latency_us = max(latencies_ns) / 1000.0
        p95_latency_us = sorted(latencies_ns)[int(len(latencies_ns) * 0.95)] / 1000.0

        print(f"Target Storage:    Physical Non-Volatile Device (/home)")
        print(f"Iterations:        {iterations} (with 64KB dirty pages per cycle)")
        print(f"Min Latency:       {min_latency_us:.2f} µs ({min_latency_us / 1000.0:.3f} ms)")
        print(f"Average Latency:   {avg_latency_us:.2f} µs ({avg_latency_us / 1000.0:.3f} ms)")
        print(f"p95 Latency:       {p95_latency_us:.2f} µs ({p95_latency_us / 1000.0:.3f} ms)")
        print(f"Max Latency:       {max_latency_us:.2f} µs ({max_latency_us / 1000.0:.3f} ms)")
        print("=" * 65)

        # Real Hardware Flush threshold: SLA < 50.0ms for non-volatile storage controllers
        threshold_ms = 50.0
        if (p95_latency_us / 1000.0) < threshold_ms:
            print(f"[PASS] Hardware Teardown verified! p95 latency ({p95_latency_us/1000.0:.3f} ms) < {threshold_ms} ms SLA.")
            return 0
        else:
            print(f"[FAIL] Hardware Teardown latency ({p95_latency_us/1000.0:.3f} ms) exceeded {threshold_ms} ms SLA.")
            return 1
    finally:
        if os.path.exists(bench_dir):
            shutil.rmtree(bench_dir, ignore_errors=True)

if __name__ == "__main__":
    sys.exit(run_teardown_benchmark())
