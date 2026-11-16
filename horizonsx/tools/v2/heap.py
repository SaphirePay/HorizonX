#!/usr/bin/env python3
"""Which code holds a running server's memory: names the stacks of a heap-profile build's heap-profile.txt.

Usage: python -I tools/v2/heap.py SERVER_DIR SYMBOLS_DIR [--top 40] [--depth 6] [--by function|stack] [--lines]
SERVER_DIR holds heap-profile.txt (written every 15 s by `build.py NAME --heap-profile` builds); the process
must still run, as its addresses are named from its own modules (dbghelp, the build's PDB in SYMBOLS_DIR).
--by function (default): bytes by the first frame that isn't allocator, collection or std plumbing, with the
  frames under it; --by stack: the stacks as sampled.
"""
import collections
import ctypes
import pathlib
import sys
from ctypes import byref, c_uint32, c_uint64, create_string_buffer, wintypes

kernel = ctypes.WinDLL('kernel32', use_last_error=True)
dbghelp = ctypes.WinDLL('dbghelp', use_last_error=True)
kernel.OpenProcess.restype, kernel.OpenProcess.argtypes = ctypes.c_void_p, [c_uint32, wintypes.BOOL, c_uint32]
dbghelp.SymSetOptions.restype, dbghelp.SymSetOptions.argtypes = c_uint32, [c_uint32]
dbghelp.SymInitializeW.restype, dbghelp.SymInitializeW.argtypes = wintypes.BOOL, [ctypes.c_void_p, wintypes.LPCWSTR, wintypes.BOOL]
dbghelp.SymFromAddrW.restype, dbghelp.SymFromAddrW.argtypes = wintypes.BOOL, [ctypes.c_void_p, c_uint64, ctypes.c_void_p, ctypes.c_void_p]
dbghelp.SymGetLineFromAddrW64.restype, dbghelp.SymGetLineFromAddrW64.argtypes = wintypes.BOOL, [ctypes.c_void_p, c_uint64, ctypes.c_void_p, ctypes.c_void_p]


class Line(ctypes.Structure):
    _fields_ = [('size', c_uint32), ('key', ctypes.c_void_p), ('line', c_uint32), ('file', ctypes.c_wchar_p), ('address', c_uint64)]

# Frames that say nothing about who holds the memory.
PLUMBING = ('heap_profile', 'alloc::', 'core::', 'hashbrown::', 'std::', '__rust', 'RtlCaptureStackBackTrace', 'HeapAlloc',
            'RtlAllocateHeap', 'smallvec::', 'indexmap::', 'Vec<', 'Box<', 'impl$', 'rust_begin', 'RawVec', 'BTreeMap', 'clone',
            'horizon_threads::', 'ntdll', 'serde_json::', 'from_iter', 'collect', 'extend', 'spec_')


def option(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def main():
    folder, symbols = pathlib.Path(sys.argv[1]), sys.argv[2]
    lines = (folder / 'heap-profile.txt').read_text(encoding='utf-8').splitlines()
    head = lines[0]
    pid = int(head.rsplit('pid ', 1)[1])
    process = kernel.OpenProcess(0x1FFFFF, False, pid)
    dbghelp.SymSetOptions(0x2 | 0x10 | 0x40)
    if not process or not dbghelp.SymInitializeW(process, symbols, True):
        sys.exit(f'cannot read process {pid}: {ctypes.get_last_error()}')
    names = {}

    def name(address):
        if address not in names:
            buffer = create_string_buffer(88 + 2 * 400)
            c_uint32.from_buffer(buffer, 0).value = 88
            c_uint32.from_buffer(buffer, 80).value = 400
            displacement = c_uint64()
            text = ctypes.wstring_at(ctypes.addressof(buffer) + 84, c_uint32.from_buffer(buffer, 76).value) if dbghelp.SymFromAddrW(process, address, byref(displacement), buffer) else f'{address:#x}'
            if '--lines' in sys.argv:
                line, offset = Line(), c_uint32()
                line.size = ctypes.sizeof(Line)
                # The call site: the return address minus one.
                if dbghelp.SymGetLineFromAddrW64(process, address - 1, byref(offset), byref(line)) and line.file:
                    text += f' @{line.file.rsplit(chr(92), 1)[-1]}:{line.line}'
            names[address] = text
        return names[address]

    total = int(head.split('sampled live bytes ', 1)[1].split(' ', 1)[0])
    rows = []
    for line in lines[1:]:
        parts = line.split()
        rows.append((int(parts[0]), int(parts[1]), [name(int(a, 16)) for a in parts[2:]]))
    print(f'{head}\nestimated live heap {total / 1048576:.0f} MB in {len(rows)} stacks')
    top, depth = int(option('--top', '40')), int(option('--depth', '6'))
    if option('--by', 'function') == 'stack':
        for bytes_, count, frames in rows[:top]:
            print(f'\n{bytes_ / 1048576:8.1f} MB  {count} samples')
            for frame in frames[:depth + 4]:
                print(f'      {frame[:150]}')
        return
    by = collections.defaultdict(lambda: [0, collections.Counter()])
    for bytes_, _, frames in rows:
        useful = [f for f in frames if not any(p in f for p in PLUMBING)]
        key = useful[0] if useful else (frames[0] if frames else '?')
        by[key][0] += bytes_
        by[key][1][' < '.join(f.split('::')[-1][:40] for f in useful[1:depth])] += bytes_
    for key, (bytes_, callers) in sorted(by.items(), key=lambda kv: -kv[1][0])[:top]:
        print(f'{bytes_ / 1048576:8.1f} MB {100 * bytes_ / max(total, 1):5.1f}%  {key[:120]}')
        for chain, b in callers.most_common(3):
            print(f'              {b / 1048576:7.1f} MB  < {chain[:160]}')


if __name__ == '__main__':
    main()
