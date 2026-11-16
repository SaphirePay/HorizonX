//! An XMPP stream as XML: one document that never ends, read a stanza at a time as it comes in.
//! Only what RFC 6120 allows on a stream: no DTD, no entities but the five and numeric ones.
use essentials::fail_if;

const MAX_BUFFER: usize = 4 << 20;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    /// Without its prefix: `stream:features` is `features`.
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.name == name)
    }
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(|child| child.text.as_str())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The server's `<stream:stream>`, with its attributes.
    Open(Element),
    Stanza(Element),
    /// `</stream:stream>`.
    Close,
}

#[derive(Default)]
pub struct Parser {
    buffer: Vec<u8>,
    at: usize,
    open: bool,
    stack: Vec<Element>,
}

fn invalid(text: &str) -> String {
    format!("bad XML from the server: {text}")
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

/// Undoes `&lt;` and its like.
fn unescape(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("not UTF-8"))?;
    if !text.contains('&') {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let end = tail.find(';').ok_or_else(|| invalid("an unended entity"))?;
        let name = &tail[..end];
        match name {
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "amp" => out.push('&'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let code = match name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => name.strip_prefix('#').and_then(|decimal| decimal.parse().ok()),
                };
                out.push(code.and_then(char::from_u32).ok_or_else(|| invalid("an unknown entity"))?);
            }
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// What goes between tags or in an attribute. Characters XML 1.0 can't carry are dropped: one of
/// them would end the stream, and a chat line can hold anything.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if u32::from(c) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// The tag between `<` and `>`, without a self-closing slash.
fn parse_tag(tag: &[u8]) -> Result<Element, String> {
    let is_space = |byte: u8| matches!(byte, b' ' | b'\t' | b'\r' | b'\n');
    let end = tag.iter().position(|&byte| is_space(byte)).unwrap_or(tag.len());
    let name = std::str::from_utf8(&tag[..end]).map_err(|_| invalid("not UTF-8"))?;
    fail_if!(name.is_empty(), invalid("a tag with no name"));
    let mut element = Element { name: local(name), ..Element::default() };
    let mut at = end;
    loop {
        while at < tag.len() && is_space(tag[at]) {
            at += 1;
        }
        if at >= tag.len() {
            return Ok(element);
        }
        let equals = tag[at..].iter().position(|&byte| byte == b'=').ok_or_else(|| invalid("an attribute with no value"))? + at;
        let key = std::str::from_utf8(&tag[at..equals]).map_err(|_| invalid("not UTF-8"))?.trim().to_owned();
        at = equals + 1;
        while at < tag.len() && is_space(tag[at]) {
            at += 1;
        }
        let quote = *tag.get(at).ok_or_else(|| invalid("an attribute with no value"))?;
        fail_if!(quote != b'"' && quote != b'\'', invalid("an unquoted attribute"));
        let close = tag[at + 1..].iter().position(|&byte| byte == quote).ok_or_else(|| invalid("an unended attribute"))? + at + 1;
        element.attrs.push((key, unescape(&tag[at + 1..close])?));
        at = close + 1;
    }
}

/// Where the tag starting at `<` ends: a `>` in a quoted value doesn't end it.
fn tag_end(bytes: &[u8]) -> Option<usize> {
    let mut quote = None;
    for (index, &byte) in bytes.iter().enumerate() {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return Some(index),
            None => {}
        }
    }
    None
}

impl Parser {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.at > 0 {
            self.buffer.drain(..self.at);
            self.at = 0;
        }
        self.buffer.extend_from_slice(bytes);
        fail_if!(self.buffer.len() > MAX_BUFFER, invalid("a stanza too large to read"));
        Ok(())
    }

    /// A new stream on the same socket, after STARTTLS or SASL.
    pub fn restart(&mut self) {
        *self = Self::default();
    }

    fn append_text(&mut self, text: &str) {
        if let Some(top) = self.stack.last_mut() {
            top.text.push_str(text);
        }
    }

    /// The next whole thing in the buffer, or `None` until more arrives.
    pub fn next(&mut self) -> Result<Option<Event>, String> {
        loop {
            let bytes = &self.buffer[self.at..];
            if bytes.is_empty() {
                return Ok(None);
            }
            if bytes[0] != b'<' {
                let Some(end) = bytes.iter().position(|&byte| byte == b'<') else {
                    if self.stack.is_empty() {
                        // Whitespace keep-alives between stanzas.
                        self.at = self.buffer.len();
                    }
                    return Ok(None);
                };
                let text = unescape(&bytes[..end])?;
                self.at += end;
                self.append_text(&text);
                continue;
            }
            if bytes.len() < 2 {
                return Ok(None);
            }
            if bytes[1] == b'?' {
                let Some(end) = find(bytes, b"?>") else { return Ok(None) };
                self.at += end + 2;
                continue;
            }
            if bytes[1] == b'!' {
                if bytes.len() < 9 {
                    return Ok(None);
                }
                if bytes.starts_with(b"<![CDATA[") {
                    let Some(end) = find(bytes, b"]]>") else { return Ok(None) };
                    let text = std::str::from_utf8(&bytes[9..end]).map_err(|_| invalid("not UTF-8"))?.to_owned();
                    self.at += end + 3;
                    self.append_text(&text);
                    continue;
                }
                if bytes.starts_with(b"<!--") {
                    let Some(end) = find(bytes, b"-->") else { return Ok(None) };
                    self.at += end + 3;
                    continue;
                }
                return Err(invalid("a declaration, which XMPP doesn't allow"));
            }
            let Some(end) = tag_end(bytes) else { return Ok(None) };
            if bytes[1] == b'/' {
                self.at += end + 1;
                let Some(done) = self.stack.pop() else { return Ok(Some(Event::Close)) };
                match self.stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => return Ok(Some(Event::Stanza(done))),
                }
                continue;
            }
            let closed = bytes[end - 1] == b'/';
            let tag = &bytes[1..end - usize::from(closed)];
            let element = parse_tag(tag)?;
            self.at += end + 1;
            if !self.open {
                self.open = true;
                return Ok(Some(Event::Open(element)));
            }
            if closed {
                match self.stack.last_mut() {
                    Some(parent) => parent.children.push(element),
                    None => return Ok(Some(Event::Stanza(element))),
                }
                continue;
            }
            fail_if!(self.stack.len() >= MAX_DEPTH, invalid("stanzas nested too deep"));
            self.stack.push(element);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(parser: &mut Parser) -> Vec<Event> {
        let mut all = Vec::new();
        while let Some(event) = parser.next().unwrap() {
            all.push(event);
        }
        all
    }

    #[test]
    fn a_stream_in_pieces() {
        let mut parser = Parser::default();
        let text = "<?xml version='1.0'?><stream:stream id='a&amp;b' xmlns:stream='s'> <message from=\"x/y\" type='chat'><body>1 &lt; 2 &#x263A;</body><empty/></message></stream:stream>";
        let mut got = Vec::new();
        for byte in text.as_bytes() {
            parser.feed(std::slice::from_ref(byte)).unwrap();
            got.extend(events(&mut parser));
        }
        assert_eq!(got.len(), 3);
        let Event::Open(open) = &got[0] else { panic!("{got:?}") };
        assert_eq!(open.attr("id"), Some("a&b"));
        let Event::Stanza(message) = &got[1] else { panic!("{got:?}") };
        assert_eq!(message.name, "message");
        assert_eq!(message.attr("from"), Some("x/y"));
        assert_eq!(message.child_text("body"), Some("1 < 2 \u{263A}"));
        assert!(message.child("empty").is_some());
        assert_eq!(got[2], Event::Close);
    }

    #[test]
    fn a_right_angle_in_an_attribute_ends_nothing() {
        let mut parser = Parser::default();
        parser.feed(b"<s><a b='x>y'/>").unwrap();
        let got = events(&mut parser);
        let Event::Stanza(a) = &got[1] else { panic!("{got:?}") };
        assert_eq!(a.attr("b"), Some("x>y"));
    }

    #[test]
    fn whitespace_between_stanzas_is_nothing() {
        let mut parser = Parser::default();
        parser.feed(b"<s>   \n ").unwrap();
        assert_eq!(events(&mut parser).len(), 1);
        parser.feed(b"  <a/>").unwrap();
        assert_eq!(events(&mut parser).len(), 1);
    }

    #[test]
    fn what_xmpp_forbids_is_refused() {
        let mut parser = Parser::default();
        parser.feed(b"<s><!DOCTYPE a [<!ENTITY b 'c'>]>").unwrap();
        assert!(matches!(parser.next(), Ok(Some(Event::Open(_)))));
        assert!(parser.next().is_err());
        let mut parser = Parser::default();
        parser.feed(b"<s><a>&unknown;</a>").unwrap();
        parser.next().unwrap();
        assert!(parser.next().is_err());
    }

    #[test]
    fn a_stanza_nested_without_end_is_refused() {
        let mut parser = Parser::default();
        parser.feed(b"<s>").unwrap();
        parser.next().unwrap();
        parser.feed("<a>".repeat(MAX_DEPTH + 1).as_bytes()).unwrap();
        assert!(parser.next().is_err());
    }

    #[test]
    fn escaping_keeps_a_stream_legal() {
        assert_eq!(escape("<a b='c'>&\u{1B}[0m\u{8}"), "&lt;a b=&apos;c&apos;&gt;&amp;[0m");
    }
}
