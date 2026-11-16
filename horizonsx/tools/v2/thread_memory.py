#!/usr/bin/env python3
"""What a running server's threads hold: each thread's committed stack (its TEB's StackBase - StackLimit),
added up by thread name. The per-connection threads are the per-player part of it.

Usage: python -I tools/v2/thread_memory.py PID
"""
import collections
import ctypes
import sys
from ctypes import byref, c_uint32, c_uint64, c_void_p, wintypes

kernel = ctypes.WinDLL('kernel32', use_last_error=True)
ntdll = ctypes.WinDLL('ntdll')
for name, restype, argtypes in (
    ('OpenProcess', c_void_p, [c_uint32, wintypes.BOOL, c_uint32]),
    ('OpenThread', c_void_p, [c_uint32, wintypes.BOOL, c_uint32]),
    ('CloseHandle', wintypes.BOOL, [c_void_p]),
    ('CreateToolhelp32Snapshot', c_void_p, [c_uint32, c_uint32]),
    ('Thread32First', wintypes.BOOL, [c_void_p, c_void_p]),
    ('Thread32Next', wintypes.BOOL, [c_void_p, c_void_p]),
    ('GetThreadDescription', ctypes.c_long, [c_void_p, c_void_p]),
    ('LocalFree', c_void_p, [c_void_p]),
    ('ReadProcessMemory', wintypes.BOOL, [c_void_p, c_void_p, c_void_p, ctypes.c_size_t, ctypes.c_void_p]),
):
    function = getattr(kernel, name)
    function.restype, function.argtypes = restype, argtypes
ntdll.NtQueryInformationThread.restype = ctypes.c_long
ntdll.NtQueryInformationThread.argtypes = [c_void_p, ctypes.c_int, c_void_p, c_uint32, c_void_p]


class ThreadEntry(ctypes.Structure):
    _fields_ = [('size', c_uint32), ('usage', c_uint32), ('thread', c_uint32), ('owner', c_uint32), ('base', ctypes.c_long), ('delta', ctypes.c_long), ('flags', c_uint32)]


class BasicInformation(ctypes.Structure):
    _fields_ = [('exit', ctypes.c_long), ('teb', c_void_p), ('process', c_void_p), ('thread', c_void_p), ('affinity', c_uint64), ('priority', ctypes.c_long), ('base', ctypes.c_long)]


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


def name_of(handle):
    text = c_void_p()
    if kernel.GetThreadDescription(handle, byref(text)) < 0 or not text.value:
        return '?'
    name = ctypes.wstring_at(text.value)
    kernel.LocalFree(text)
    return name


def main():
    pid = int(sys.argv[1])
    process = kernel.OpenProcess(0x0410, False, pid)
    by = collections.defaultdict(lambda: [0, 0])
    for thread in threads(pid):
        handle = kernel.OpenThread(0x0040 | 0x0800, False, thread)
        if not handle:
            continue
        info = BasicInformation()
        if ntdll.NtQueryInformationThread(handle, 0, byref(info), ctypes.sizeof(info), None) == 0 and info.teb:
            # NT_TIB: ExceptionList, StackBase, StackLimit.
            tib = (c_uint64 * 3)()
            if kernel.ReadProcessMemory(process, info.teb, tib, ctypes.sizeof(tib), None):
                name = name_of(handle).rstrip('0123456789-') or '?'
                by[name][0] += 1
                by[name][1] += tib[1] - tib[2]
        kernel.CloseHandle(handle)
    total = sum(v[1] for v in by.values())
    print(f'{sum(v[0] for v in by.values())} threads, {total / 1048576:.1f} MB of committed stacks')
    for name, (count, bytes_) in sorted(by.items(), key=lambda kv: -kv[1][1]):
        print(f'  {count:4}  {bytes_ / 1048576:7.2f} MB  {bytes_ / count / 1024:6.0f} KB each  {name}')


if __name__ == '__main__':
    main()
