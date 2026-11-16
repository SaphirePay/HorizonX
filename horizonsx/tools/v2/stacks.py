#!/usr/bin/env python3
"""Where every thread of a running engine is, with no debugger: a frozen server shows what it's waiting for.

Usage: stacks.py [--all] [--match TEXT]    every horizon-client.exe; the main thread unless --all
       stacks.py --match TEXT --profile N  N samples of a server's main thread, added up by function
       stacks.py --match TEXT --profile N --thread PREFIX   the busy threads named PREFIX* instead (horizonsx-)
Needs the build's PDB: target/cargo-quick/fast/horizon_client.pdb, of the same build as the running exe.
"""
import ctypes
import json
import subprocess
import sys
from ctypes import byref, c_uint32, c_uint64, c_void_p, create_string_buffer, create_unicode_buffer, wintypes

SYMBOLS = sys.argv[sys.argv.index('--symbols') + 1] if '--symbols' in sys.argv else r'D:\Horizon\fabric-port\target\cargo-quick\fast'
kernel = ctypes.WinDLL('kernel32', use_last_error=True)
dbghelp = ctypes.WinDLL('dbghelp', use_last_error=True)

for name, restype, argtypes in (
    ('OpenProcess', c_void_p, [c_uint32, wintypes.BOOL, c_uint32]),
    ('OpenThread', c_void_p, [c_uint32, wintypes.BOOL, c_uint32]),
    ('CloseHandle', wintypes.BOOL, [c_void_p]),
    ('SuspendThread', c_uint32, [c_void_p]),
    ('ResumeThread', c_uint32, [c_void_p]),
    ('GetThreadContext', wintypes.BOOL, [c_void_p, c_void_p]),
    ('CreateToolhelp32Snapshot', c_void_p, [c_uint32, c_uint32]),
    ('Thread32First', wintypes.BOOL, [c_void_p, c_void_p]),
    ('Thread32Next', wintypes.BOOL, [c_void_p, c_void_p]),
    ('GetThreadDescription', ctypes.c_long, [c_void_p, c_void_p]),
    ('LocalFree', c_void_p, [c_void_p]),
):
    function = getattr(kernel, name)
    function.restype, function.argtypes = restype, argtypes
for name, restype, argtypes in (
    ('SymSetOptions', c_uint32, [c_uint32]),
    ('SymInitializeW', wintypes.BOOL, [c_void_p, wintypes.LPCWSTR, wintypes.BOOL]),
    ('SymCleanup', wintypes.BOOL, [c_void_p]),
    ('SymFromAddrW', wintypes.BOOL, [c_void_p, c_uint64, c_void_p, c_void_p]),
    ('SymFunctionTableAccess64', c_void_p, [c_void_p, c_uint64]),
    ('SymGetModuleBase64', c_uint64, [c_void_p, c_uint64]),
    ('StackWalk64', wintypes.BOOL, [c_uint32, c_void_p, c_void_p, c_void_p, c_void_p, c_void_p, c_void_p, c_void_p, c_void_p]),
):
    function = getattr(dbghelp, name)
    function.restype, function.argtypes = restype, argtypes


class ThreadEntry(ctypes.Structure):
    _fields_ = [('size', c_uint32), ('usage', c_uint32), ('thread', c_uint32), ('owner', c_uint32), ('base', ctypes.c_long), ('delta', ctypes.c_long), ('flags', c_uint32)]


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
        return ''
    name = ctypes.wstring_at(text.value)
    kernel.LocalFree(text)
    return name


class LineW64(ctypes.Structure):
    _fields_ = [('size', c_uint32), ('key', c_void_p), ('line', c_uint32), ('file', ctypes.c_wchar_p), ('address', c_uint64)]


dbghelp.SymGetLineFromAddrW64.restype = wintypes.BOOL
dbghelp.SymGetLineFromAddrW64.argtypes = [c_void_p, c_uint64, ctypes.POINTER(c_uint32), ctypes.POINTER(LineW64)]


def source_line(process, address):
    """`file:line` of an address (the innermost inlined code there), from the PDB's line tables."""
    line = LineW64()
    line.size = ctypes.sizeof(LineW64)
    displacement = c_uint32()
    if not dbghelp.SymGetLineFromAddrW64(process, address, byref(displacement), byref(line)):
        return ''
    path = (line.file or '').replace('\\', '/')
    for cut in ('/crates/', '/library/', '/registry/src/'):
        if cut in path:
            path = path.split(cut, 1)[1]
            break
    return f'{path}:{line.line}'


def symbol(process, address):
    buffer = create_string_buffer(88 + 2 * 400)
    c_uint32.from_buffer(buffer, 0).value = 88
    c_uint32.from_buffer(buffer, 80).value = 400
    displacement = c_uint64()
    if not dbghelp.SymFromAddrW(process, address, byref(displacement), buffer):
        return f'{address:#x}'
    # NameLen is the whole name's length; only MaxNameLen (400) characters of it fit in the buffer (long Rust generics).
    return ctypes.wstring_at(ctypes.addressof(buffer) + 84, min(c_uint32.from_buffer(buffer, 76).value, 399))


def walk(process, handle):
    # An x64 `CONTEXT`, sixteen-aligned.
    raw = create_string_buffer(1232 + 16)
    base = (ctypes.addressof(raw) + 15) & ~15
    c_uint32.from_address(base + 48).value = 0x10000B
    if kernel.SuspendThread(handle) == 0xFFFFFFFF:
        return []
    frames = []
    try:
        if not kernel.GetThreadContext(handle, base):
            return []
        frame = create_string_buffer(1024)
        for offset, register in ((0, 0xF8), (32, 0xA0), (48, 0x98)):
            c_uint64.from_buffer(frame, offset).value = c_uint64.from_address(base + register).value
            c_uint32.from_buffer(frame, offset + 12).value = 3
        for _ in range(70):
            if not dbghelp.StackWalk64(0x8664, process, handle, frame, base, None, ctypes.cast(dbghelp.SymFunctionTableAccess64, c_void_p), ctypes.cast(dbghelp.SymGetModuleBase64, c_void_p), None):
                break
            address = c_uint64.from_buffer(frame, 0).value
            if not address:
                break
            frames.append(address)
    finally:
        kernel.ResumeThread(handle)
    return frames


def profile(row, samples, every):
    """`samples` stacks of a server's main thread, added up: where its time goes, by function."""
    import collections
    import time
    pid = row['ProcessId']
    process = kernel.OpenProcess(0x1FFFFF, False, pid)
    if not process or not dbghelp.SymInitializeW(process, SYMBOLS, True):
        print(pid, 'no access', ctypes.get_last_error())
        return
    main = None
    # --thread PREFIX: every thread whose name starts with PREFIX (region and worker threads: horizonsx-), each sample
    # walking all of them; an idle one (parked, waiting) is left out.
    prefix = sys.argv[sys.argv.index('--thread') + 1] if '--thread' in sys.argv else None
    group = []
    for thread in threads(pid):
        handle = kernel.OpenThread(0x1FFFFF, False, thread)
        if not handle:
            continue
        if prefix is not None and title(handle).startswith(prefix):
            group.append(handle)
        elif prefix is None and title(handle) == 'main':
            main = handle
            break
    print(f'sampling {len(group)} threads named {prefix}*' if prefix is not None else 'sampling the main thread')
    names = {}
    def name(address):
        if address not in names:
            names[address] = symbol(process, address)
        return names[address]
    alone, within, callers = collections.Counter(), collections.Counter(), collections.Counter()
    kept = []
    total = 0
    lines = {}
    def where(address):
        if address not in lines:
            lines[address] = source_line(process, address)
        return lines[address]
    idle_words = ('ZwWaitFor', 'NtWaitFor', 'WaitOnAddress', 'park', 'Sleep', 'sleep', 'ZwDelayExecution', 'NtRemoveIoCompletion')
    for _ in range(samples):
        if prefix is not None:
            for handle in group:
                addresses = walk(process, handle)
                frames = [name(address) for address in addresses]
                if not frames:
                    continue
                if any(w in f for f in frames[:3] for w in idle_words):
                    # Parked: kept only when it waits inside a parallel phase (a region waiting for helpers).
                    if not any('in_place_scope' in f or 'v24claim' in f or '::claim' in f for f in frames):
                        continue
                    frames = ['(waiting for helpers)'] + frames
                if '--lines' in sys.argv:
                    frames = [f'{f} @{where(a)}' if i < 4 else f for i, (f, a) in enumerate(zip(frames, addresses))]
                total += 1
                kept.append(frames)
                alone[frames[0]] += 1
                for frame in set(frames):
                    within[frame] += 1
                for callee, caller in zip(frames, frames[1:12]):
                    if WATCH and WATCH in callee:
                        callers[caller] += 1
                        break
            time.sleep(every)
            continue
        addresses = walk(process, main)
        frames = [name(address) for address in addresses]
        # The first frames also say where in the source (inlined code shows as its own file and line).
        if '--lines' in sys.argv:
            frames = [f'{f} @{where(a)}' if i < 4 else f for i, (f, a) in enumerate(zip(frames, addresses))]
        if not frames:
            continue
        if any('sleep' in frame for frame in frames[:4]):
            alone['(idle between ticks)'] += 1
            total += 1
            time.sleep(every)
            continue
        total += 1
        kept.append(frames)
        alone[frames[0]] += 1
        for frame in set(frames):
            within[frame] += 1
        for callee, caller in zip(frames, frames[1:12]):
            if WATCH and WATCH in callee:
                callers[caller] += 1
                break
        time.sleep(every)
    print(f'{total} samples')
    print('-- where it was at the top of the stack')
    for frame, count in alone.most_common(40):
        print(f'{100 * count / total:6.1f}%  {frame[:110]}')
    print('-- anywhere in the stack')
    for frame, count in within.most_common(60):
        print(f'{100 * count / total:6.1f}%  {frame[:110]}')
    if '--keep' in sys.argv:
        import json
        with open(sys.argv[sys.argv.index('--keep') + 1], 'w', encoding='utf-8') as out:
            for frames in kept:
                out.write(json.dumps(frames) + chr(10))
    if WATCH:
        print(f'-- what called {WATCH}')
        for frame, count in callers.most_common(30):
            print(f'{100 * count / total:6.1f}%  {frame[:110]}')
    dbghelp.SymCleanup(process)


WATCH = sys.argv[sys.argv.index('--callers') + 1] if '--callers' in sys.argv else None


def engines():
    text = subprocess.run(['powershell.exe', '-NoProfile', '-Command', "Get-CimInstance Win32_Process -Filter \"name='horizon-client.exe'\" | Select-Object ProcessId,CommandLine | ConvertTo-Json"], capture_output=True, text=True, encoding='utf-8', errors='replace').stdout
    rows = json.loads(text or '[]')
    return [rows] if isinstance(rows, dict) else rows


def main():
    everything = '--all' in sys.argv
    match = sys.argv[sys.argv.index('--match') + 1] if '--match' in sys.argv else None
    dbghelp.SymSetOptions(0x2 | 0x10 | 0x40)  # UNDNAME, LOAD_LINES, as plugin-tools' copy
    for row in engines():
        pid = row['ProcessId']
        if match and match not in (row['CommandLine'] or ''):
            continue
        if '--profile' in sys.argv:
            if '--offscreen' not in (row['CommandLine'] or ''):
                profile(row, int(sys.argv[sys.argv.index('--profile') + 1]), 0.02)
            continue
        process = kernel.OpenProcess(0x1FFFFF, False, pid)
        if not process or not dbghelp.SymInitializeW(process, SYMBOLS, True):
            print(pid, 'no access', ctypes.get_last_error())
            continue
        print(f'== {pid} {(row["CommandLine"] or "")[:150]}')
        for thread in threads(pid):
            handle = kernel.OpenThread(0x1FFFFF, False, thread)
            if not handle:
                continue
            name = title(handle)
            if everything or name == 'main' or name.startswith('Server'):
                print(f'-- {thread} {name}')
                for address in walk(process, handle):
                    print('   ', symbol(process, address)[:130])
            kernel.CloseHandle(handle)
        dbghelp.SymCleanup(process)
        kernel.CloseHandle(process)


if __name__ == '__main__':
    main()
