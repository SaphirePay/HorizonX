#!/usr/bin/env python3
"""Which threads of a running engine use its CPU: each thread's CPU time over SECONDS, added up by thread name (the
name with its trailing number dropped, so horizon-lan-out-17 counts as horizon-lan-out).

Usage: python -I tools/v2/thread_cpu.py PID [SECONDS]

The tick's own report times phases; this also counts what runs off the tick (chunk loading and lighting on the
worker threads, the connections' writers), which no span sees.
"""
import collections
import ctypes
import re
import sys
import time
from ctypes import byref, c_uint32, c_void_p, wintypes

kernel = ctypes.WinDLL('kernel32', use_last_error=True)
kernel.OpenThread.restype, kernel.OpenThread.argtypes = c_void_p, [c_uint32, wintypes.BOOL, c_uint32]
kernel.CloseHandle.argtypes = [c_void_p]
kernel.GetThreadTimes.argtypes = [c_void_p] + [ctypes.POINTER(wintypes.FILETIME)] * 4
kernel.GetThreadDescription.argtypes = [c_void_p, ctypes.POINTER(c_void_p)]
kernel.GetThreadDescription.restype = ctypes.c_long
kernel.LocalFree.argtypes = [c_void_p]
kernel.CreateToolhelp32Snapshot.restype = c_void_p
kernel.CreateToolhelp32Snapshot.argtypes = [c_uint32, c_uint32]


class ThreadEntry(ctypes.Structure):
    _fields_ = [('size', c_uint32), ('usage', c_uint32), ('thread', c_uint32), ('owner', c_uint32),
                ('base', ctypes.c_long), ('delta', ctypes.c_long), ('flags', c_uint32)]


kernel.Thread32First.argtypes = kernel.Thread32Next.argtypes = [c_void_p, ctypes.POINTER(ThreadEntry)]


def threads(pid):
    snapshot = kernel.CreateToolhelp32Snapshot(4, 0)
    entry = ThreadEntry()
    entry.size = ctypes.sizeof(ThreadEntry)
    found = []
    more = kernel.Thread32First(snapshot, byref(entry))
    while more:
        if entry.owner == pid:
            found.append(entry.thread)
        more = kernel.Thread32Next(snapshot, byref(entry))
    kernel.CloseHandle(snapshot)
    return found


def title(handle):
    text = c_void_p()
    if kernel.GetThreadDescription(handle, byref(text)) < 0 or not text.value:
        return '?'
    name = ctypes.wstring_at(text.value)
    kernel.LocalFree(text)
    return name or '?'


def times(pid):
    """{thread id: (name, CPU seconds)}."""
    out = {}
    for tid in threads(pid):
        handle = kernel.OpenThread(0x0800 | 0x0400, False, tid)  # QUERY_LIMITED_INFORMATION | QUERY_INFORMATION
        if not handle:
            continue
        c, e, k, u = (wintypes.FILETIME() for _ in range(4))
        if kernel.GetThreadTimes(handle, byref(c), byref(e), byref(k), byref(u)):
            ticks = lambda f: (f.dwHighDateTime << 32) | f.dwLowDateTime  # noqa: E731
            out[tid] = (title(handle), (ticks(k) + ticks(u)) / 1e7)
        kernel.CloseHandle(handle)
    return out


def main():
    pid = int(sys.argv[1])
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 10.0
    before = times(pid)
    t0 = time.time()
    time.sleep(seconds)
    after = times(pid)
    wall = time.time() - t0
    groups, counts = collections.Counter(), collections.Counter()
    for tid, (name, cpu) in after.items():
        group = re.sub(r'[-_ ]?\d+$', '', name)
        groups[group] += cpu - before.get(tid, (name, 0.0))[1]
        counts[group] += 1
    total = sum(groups.values()) / wall
    print(f'{total:.2f} cores over {wall:.1f} s, {len(after)} threads')
    for group, cpu in groups.most_common(25):
        print(f'{cpu / wall:7.2f} cores  {counts[group]:5d} threads  {group}')


if __name__ == '__main__':
    main()
