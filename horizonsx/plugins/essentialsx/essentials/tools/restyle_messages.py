#!/usr/bin/env python3
"""Writes a complete, restyled English message file for EssentialsX on Horizon servers: every message in small
capitals (the server's lettering) and bright colours instead of Minecraft's dark defaults.

    python tools/restyle_messages.py OUT/messages_en.properties

It starts from the bundled English (resources/messages/messages.properties with messages_en.properties on top,
as Essentials reads them) and changes only the look:
- letters outside tags, {arguments} and placeholders become small capitals (ᴛᴇxᴛ); `<command>` and colour tags stay;
- the colour tags become the server palette (dark red -> soft red, dark grey -> light lavender, ...); <primary> and
  <secondary> stay and take their colours from message-colors in config.yml;
- sign headers (signFormat*) are left as they are (existing signs are matched against them).

Put the file in plugins/Essentials/messages/: Essentials then reads only the files there, so it holds every message.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
RES = ROOT / 'resources' / 'messages'

SMALL = dict(zip('abcdefghijklmnopqrtuvwyz', 'ᴀʙᴄᴅᴇꜰɢʜɪᴊᴋʟᴍɴᴏᴘǫʀᴛᴜᴠᴡʏᴢ'))

COLORS = {
    'dark_red': '#f87171', 'red': '#fca5a5', 'green': '#4ade80', 'dark_green': '#86efac', 'yellow': '#fde047',
    'gold': '#fbbf24', 'aqua': '#67e8f9', 'dark_aqua': '#22d3ee', 'blue': '#93c5fd', 'dark_blue': '#60a5fa',
    'light_purple': '#f0abfc', 'dark_purple': '#c084fc', 'gray': '#e4e4e7', 'grey': '#e4e4e7',
    'dark_gray': '#c4b5fd', 'dark_grey': '#c4b5fd', 'black': '#d4d4d8',
}

# MiniMessage tags (anything else in <...> is an argument name like <player>, shown as text).
TAGS = set(COLORS) | {'white', 'primary', 'secondary', 'reset', 'bold', 'b', 'italic', 'i', 'em', 'underlined', 'u',
                      'strikethrough', 'st', 'obfuscated', 'obf', 'click', 'hover', 'insert', 'insertion', 'gradient',
                      'rainbow', 'transition', 'font', 'key', 'lang', 'tr', 'translate', 'newline', 'br', 'shadow',
                      'color', 'colour', 'c', 'pride', 'score', 'selector', 'nbt', 'command'}

SKIP_KEYS = re.compile(r'^signFormat')

B = '<#c4b5fd> • <#f4f4f5>'  # a bullet line
# Messages written again, not only restyled: the server status (/tps, /gc, /lag) and /list.
CUSTOM = {
    # /tps in three short lines: an empty message is not sent, so the memory details and the line per world go.
    'uptime': '<gradient:#a855f7:#22d3ee><bold>sᴇʀᴠᴇʀ</bold></gradient> <#c4b5fd>· <#f4f4f5>ᴜᴘ <#67e8f9>{0}',
    'tps': B + 'sᴇʀᴠᴇʀ ᴛᴘs <bold>{0}</bold>',
    'gctotal': B + 'ᴍᴇᴍᴏʀʏ <#67e8f9>{0} ᴍʙ',
    'gcmax': '',
    'gcfree': '',
    'gcWorld': '',
    'listAmount': '<gradient:#a855f7:#22d3ee><bold>ᴏɴʟɪɴᴇ</bold></gradient> <#67e8f9>{0}<#c4b5fd>/<#67e8f9>{1}',
    'listAmountHidden': '<gradient:#a855f7:#22d3ee><bold>ᴏɴʟɪɴᴇ</bold></gradient> <#67e8f9>{0}<#c4b5fd>/<#67e8f9>{2} <#c4b5fd>(+{1} ʜɪᴅᴅᴇɴ)',
    'listGroupTag': '<#c084fc>{0}<#c4b5fd> » <reset>',
    'errorWithMessage': '<#f87171>✖ <#fca5a5>{0}',
    'commandHelpLine1': '<gradient:#a855f7:#22d3ee><bold>ʜᴇʟᴘ</bold></gradient> <white>/{0}',
    'commandHelpLine2': B + '{0}',
    'commandHelpLine3': '<#c4b5fd>ᴜsᴀɢᴇ',
    'commandHelpLine4': '<#c4b5fd>ᴀʟɪᴀsᴇs <#f4f4f5>{0}',
    'commandHelpLineUsage': '<#c4b5fd> • <reset>{0} <#c4b5fd>- <#f4f4f5>{1}',
}


def parse_properties(text):
    """Java Properties.load (as the Rust rewrite reads it): ordered (key, value)."""
    out = {}
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i].lstrip(' \t\f')
        i += 1
        if not line or line[0] in '#!':
            continue
        while (len(line) - len(line.rstrip('\\'))) % 2 == 1 and i <= len(lines):
            line = line[:-1] + (lines[i].lstrip(' \t\f') if i < len(lines) else '')
            i += 1
        j = 0
        while j < len(line) and line[j] not in '=: \t\f':
            j += 2 if line[j] == '\\' else 1
        key = line[:j]
        k = j
        while k < len(line) and line[k] in ' \t\f':
            k += 1
        if k < len(line) and line[k] in '=:':
            k += 1
        while k < len(line) and line[k] in ' \t\f':
            k += 1
        out[unescape(key)] = unescape(line[k:])
    return out


def unescape(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == '\\' and i + 1 < len(s):
            n = s[i + 1]
            if n == 'u' and i + 5 < len(s):
                out.append(chr(int(s[i + 2:i + 6], 16)))
                i += 6
                continue
            out.append({'t': '\t', 'n': '\n', 'r': '\r', 'f': '\f'}.get(n, n))
            i += 2
            continue
        out.append(c)
        i += 1
    return ''.join(out)


def escape_value(v):
    v = v.replace('\\', '\\\\').replace('\n', '\\n').replace('\r', '\\r').replace('\t', '\\t')
    if v.startswith(' '):
        v = '\\' + v
    return v


def escape_key(k):
    return re.sub(r'([=: #!\\])', r'\\\1', k)


def small(text):
    # s and x have no small capital in the game's font: they become lower case.
    return ''.join((SMALL.get(c.lower(), c.lower())) if c.isalpha() and c.isascii() else c for c in text)


def recolor_tag(inner):
    """`dark_red` -> `#f87171`, `/dark_red` -> `/#f87171`; other tags as they are."""
    closing = inner.startswith('/')
    name = inner[1:] if closing else inner
    if name in COLORS:
        return ('/' if closing else '') + COLORS[name]
    if name.startswith('color:') or name.startswith('c:'):
        head, _, col = name.partition(':')
        if col in COLORS:
            return ('/' if closing else '') + head + ':' + COLORS[col]
    return inner


USAGE = re.compile(r'CommandUsage\d*$')


def restyle(value, usage=False):
    """`usage`: a command's syntax line: its <required> and [optional] argument names stay plain, as Essentials
    colours them by matching plain letters."""
    out = []
    i, n = 0, len(value)
    while i < n:
        c = value[i]
        if c == '\\' and i + 1 < n and value[i + 1] == '<':
            out.append('\\<')
            i += 2
            continue
        if c == '<':
            end = value.find('>', i)
            if end == -1:
                out.append(small(value[i:]))
                break
            inner = value[i + 1:end]
            name = inner.lstrip('/!').split(':')[0].lower()
            if name in TAGS or name.startswith('#') or name.startswith('@'):
                out.append('<' + recolor_tag(inner) + '>')
            else:
                out.append('<' + (inner if usage else small(inner)) + '>')  # an argument name: <player>
            i = end + 1
            continue
        if c == '{':
            depth, j = 0, i
            while j < n:
                if value[j] == '{':
                    depth += 1
                elif value[j] == '}':
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            out.append(value[i:j + 1])
            i = j + 1
            continue
        if usage and c == '[':
            end = value.find(']', i)
            if end != -1:
                out.append(value[i:end + 1])
                i = end + 1
                continue
        if c == '%':
            m = re.match(r'%[A-Za-z0-9_]+%', value[i:])
            if m:
                out.append(m.group(0))
                i += len(m.group(0))
                continue
        out.append(small(c))
        i += 1
    return ''.join(out)


def only_tags(value):
    return re.sub(r'<[^>]*>', '', value).strip() == ''


def main():
    out_path = pathlib.Path(sys.argv[1])
    base = parse_properties((RES / 'messages.properties').read_text(encoding='utf-8'))
    base.update(parse_properties((RES / 'messages_en.properties').read_text(encoding='utf-8')))
    lines = ['# EssentialsX messages restyled for Horizon: small capitals and bright colours.',
             '# Made by plugin-ports/essentials/tools/restyle_messages.py from the bundled English; edit freely.',
             '# Essentials reads only the files in this folder, so this one holds every message.']
    changed = 0
    for key in sorted(base):
        value = base[key]
        # Colour-only messages (usage argument colours) take the palette; there are no letters to change.
        if key in CUSTOM:
            new = CUSTOM[key]
        else:
            new = value if SKIP_KEYS.match(key) else restyle(value, bool(USAGE.search(key)))
        changed += new != value
        lines.append(f'{escape_key(key)}={escape_value(new)}')
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(base)} messages, {changed} restyled -> {out_path}')


if __name__ == '__main__':
    main()
