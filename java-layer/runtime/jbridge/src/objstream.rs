//! Bukkit's object streams (`org.bukkit.util.io.BukkitObjectOutputStream` / `BukkitObjectInputStream`): what
//! plugins keep inventories and item metadata in (CoreProtect's block and container metadata, a dozen converted
//! plugins' saved inventories). Java writes its own serialization; here each object is one JSON line after a
//! `HZOS1` header, with Java's kinds kept (an Integer comes back an Integer, a Long a Long) and an ItemStack
//! through the SDK's `serialize` / `deserialize`, as Bukkit's stream does through ConfigurationSerializable.
//! The bytes only need to come back here; Java's own (`AC ED` ...) don't read, as an IOException says.
use jrt::{R, V};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

const HEADER: &[u8] = b"HZOS1\n";

pub struct JObjOut {
    out: V,
}

pub struct JObjIn {
    records: Mutex<VecDeque<Value>>,
}

fn io_error(text: impl Into<String>) -> V {
    jrt::exc("IOException", text.into())
}

fn write_raw(out: &V, bytes: &[u8]) -> Result<(), V> {
    jrt::call(out, "write", &[jrt::javaio::to_byte_array(bytes)]).map(|_| ())
}

/// A value as JSON that says its Java kind.
fn encode(value: &V) -> Result<Value, V> {
    Ok(match value {
        V::Null => Value::Null,
        V::Bool(b) => json!({ "z": b }),
        V::Char(c) => json!({ "c": c.to_string() }),
        V::Int(i) => json!({ "i": i }),
        V::Long(l) => json!({ "j": l }),
        V::Double(d) => json!({ "d": d }),
        V::Str(_) | V::Sb(_) => json!({ "s": value.text() }),
        V::Arr(items) => json!({ "A": jrt::lock(items).iter().map(encode).collect::<Result<Vec<_>, _>>()? }),
        V::List(items) => json!({ "L": jrt::lock(items).iter().map(encode).collect::<Result<Vec<_>, _>>()? }),
        V::Map(map) => {
            let pairs = jrt::lock(map).values().map(|(k, v)| Ok(json!([encode(k)?, encode(v)?]))).collect::<Result<Vec<_>, V>>()?;
            json!({ "M": pairs })
        }
        V::Set(map) => json!({ "S": jrt::lock(map).values().map(|(k, _)| encode(k)).collect::<Result<Vec<_>, _>>()? }),
        other => {
            if let Some(stack) = crate::held::<horizon_bukkit::ItemStack>(other) {
                let saved: serde_json::Map<String, Value> = stack.serialize().into_iter().collect();
                return Ok(json!({ "item": saved }));
            }
            // Collections a plugin made from its own classes (an ArrayList kept as an object, a HashMap subclass):
            // their items, when they give them.
            if jrt::instance_of(other, "Map") {
                let entries = jrt::call(other, "entrySet", &[])?.items()?;
                let pairs = entries.iter().map(|e| Ok(json!([encode(&jrt::call(e, "getKey", &[])?)?, encode(&jrt::call(e, "getValue", &[])?)?]))).collect::<Result<Vec<_>, V>>()?;
                return Ok(json!({ "M": pairs }));
            }
            if jrt::instance_of(other, "Collection") {
                return Ok(json!({ "L": other.items()?.iter().map(encode).collect::<Result<Vec<_>, _>>()? }));
            }
            return Err(jrt::exc("NotSerializableException", other.text()));
        }
    })
}

fn decode(value: &Value) -> V {
    let items = |key: &str| value[key].as_array().into_iter().flatten().map(decode).collect::<Vec<_>>();
    match value {
        Value::Null => V::Null,
        Value::Object(o) if o.contains_key("z") => V::Bool(value["z"].as_bool().unwrap_or(false)),
        Value::Object(o) if o.contains_key("c") => V::Char(value["c"].as_str().and_then(|t| t.chars().next()).unwrap_or('\0')),
        Value::Object(o) if o.contains_key("i") => V::Int(value["i"].as_i64().unwrap_or(0) as i32),
        Value::Object(o) if o.contains_key("j") => V::Long(value["j"].as_i64().unwrap_or(0)),
        Value::Object(o) if o.contains_key("d") => V::Double(value["d"].as_f64().unwrap_or(0.0)),
        Value::Object(o) if o.contains_key("s") => jrt::s(value["s"].as_str().unwrap_or_default()),
        Value::Object(o) if o.contains_key("A") => jrt::arr(items("A")),
        Value::Object(o) if o.contains_key("L") => {
            let list = jrt::new_other("ArrayList", &[]).unwrap_or(V::Null);
            for item in items("L") {
                let _ = jrt::call(&list, "add", &[item]);
            }
            list
        }
        Value::Object(o) if o.contains_key("M") => {
            jrt::map_of(value["M"].as_array().into_iter().flatten().map(|pair| (decode(&pair[0]), decode(&pair[1]))).collect())
        }
        Value::Object(o) if o.contains_key("S") => {
            let set = jrt::new_other("LinkedHashSet", &[]).unwrap_or(V::Null);
            for item in items("S") {
                let _ = jrt::call(&set, "add", &[item]);
            }
            set
        }
        Value::Object(o) if o.contains_key("item") => {
            let saved: BTreeMap<String, Value> = value["item"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.clone())).collect();
            horizon_bukkit::ItemStack::deserialize(&saved).map_or(V::Null, crate::wrap)
        }
        _ => V::Null,
    }
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    match class.rsplit('.').next().unwrap_or(class) {
        "BukkitObjectOutputStream" => {
            let out = a.first().cloned().unwrap_or(V::Null);
            Some(write_raw(&out, HEADER).map(|()| jrt::native(JObjOut { out })))
        }
        "BukkitObjectInputStream" => {
            let bytes = a.first().and_then(jrt::javaio::bytes_of).unwrap_or_default();
            if bytes.starts_with(&[0xAC, 0xED]) {
                return Some(Err(io_error("Java-serialized data (written by a Java server) can't be read on Horizon")));
            }
            let Some(body) = bytes.strip_prefix(HEADER) else {
                return Some(Err(jrt::exc("StreamCorruptedException", "invalid stream header")));
            };
            let records = String::from_utf8_lossy(body).lines().filter(|line| !line.is_empty()).map(|line| serde_json::from_str(line).unwrap_or(Value::Null)).collect();
            Some(Ok(jrt::native(JObjIn { records: Mutex::new(records) })))
        }
        _ => None,
    }
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(stream) = this.native::<JObjOut>() {
        let record = |value: Value| -> R {
            let mut line = value.to_string().into_bytes();
            line.push(b'\n');
            write_raw(&stream.out, &line).map(|()| V::Null)
        };
        return Some(match name {
            "writeObject" | "writeUnshared" => encode(a.first().unwrap_or(&V::Null)).and_then(record),
            "writeInt" | "writeShort" | "writeByte" => record(json!({ "i": a[0].int() })),
            "writeLong" => record(json!({ "j": a[0].long() })),
            "writeDouble" | "writeFloat" => record(json!({ "d": a[0].double() })),
            "writeBoolean" => record(json!({ "z": a[0].truth() })),
            "writeUTF" | "writeChars" | "writeBytes" => record(json!({ "s": a[0].text() })),
            "flush" => jrt::call(&stream.out, "flush", &[]).or(Ok(V::Null)),
            "close" => jrt::call(&stream.out, "close", &[]).or(Ok(V::Null)),
            "reset" | "defaultWriteObject" => Ok(V::Null),
            _ => return None,
        });
    }
    if let Some(stream) = this.native::<JObjIn>() {
        let mut next = || jrt::lock(&stream.records).pop_front().ok_or_else(|| jrt::exc("EOFException", ""));
        return Some(match name {
            "readObject" | "readUnshared" => next().map(|v| decode(&v)),
            "readInt" | "readShort" | "readByte" => next().map(|v| V::Int(decode(&v).int())),
            "readLong" => next().map(|v| V::Long(decode(&v).long())),
            "readDouble" | "readFloat" => next().map(|v| V::Double(decode(&v).double())),
            "readBoolean" => next().map(|v| V::Bool(decode(&v).truth())),
            "readUTF" => next().map(|v| jrt::s(&decode(&v).text())),
            "available" => Ok(V::Int(jrt::lock(&stream.records).len() as i32)),
            "close" => Ok(V::Null),
            _ => return None,
        });
    }
    None
}
