//! The game's side of Java plugins: events, commands and tab completion handed to `horizon.javalayer.Bridge`.
use crate::conv;
use horizon_bukkit::CommandSender;
use jni::objects::{JClass, JObject, JObjectArray, JString, JValue};

fn with_bridge<T>(default: T, body: impl for<'l> FnOnce(&mut jni::JNIEnv<'l>, &JClass) -> jni::errors::Result<T>) -> T {
    let Some(java) = crate::jvm::java() else { return default };
    let Ok(mut env) = java.vm.attach_current_thread_permanently() else { return default };
    let class: &JClass = java.bridge.as_obj().into();
    let out = env.with_local_frame(32, |env| body(env, class));
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    out.unwrap_or(default)
}

/// An SDK event for Java's listeners of `class` at priority `slot`. The event is live while they run; Java's own
/// copy of it (a real Bukkit event object) keeps what they read.
pub fn fire<T: 'static>(class: &'static str, slot: u8, event: &mut T) {
    let (holder, id) = jbridge::ev_open(event);
    with_bridge((), |env, bridge| {
        let handle = conv::to_java(env, &holder)?;
        let name = env.new_string(class)?;
        env.call_static_method(bridge, "fire", "(Ljava/lang/String;Ljava/lang/Object;I)V", &[JValue::Object(&name), JValue::Object(&handle), JValue::Int(i32::from(slot))])?;
        Ok(())
    });
    jbridge::ev_close(id);
}

fn sender_and_args<'l>(env: &mut jni::JNIEnv<'l>, sender: &CommandSender, args: &[&str]) -> jni::errors::Result<(JObject<'l>, JObjectArray<'l>)> {
    let sender = conv::to_java(env, &jbridge::wrap(sender.clone()))?;
    let list = crate::jvm::strings(env, &args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())?;
    Ok((sender, list))
}

pub fn command(owner: &str, name: &str, sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    with_bridge(true, |env, bridge| {
        let (who, list) = sender_and_args(env, sender, args)?;
        let (owner, name, label) = (env.new_string(owner)?, env.new_string(name)?, env.new_string(label)?);
        env.call_static_method(
            bridge,
            "command",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/String;)Z",
            &[JValue::Object(&owner), JValue::Object(&name), JValue::Object(&who), JValue::Object(&label), JValue::Object(&list)],
        )?
        .z()
    })
}

pub fn tab(owner: &str, name: &str, sender: &CommandSender, alias: &str, args: &[&str]) -> Vec<String> {
    with_bridge(Vec::new(), |env, bridge| {
        let (who, list) = sender_and_args(env, sender, args)?;
        let (owner, name, alias) = (env.new_string(owner)?, env.new_string(name)?, env.new_string(alias)?);
        let out = env
            .call_static_method(
                bridge,
                "tab",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/String;)[Ljava/lang/String;",
                &[JValue::Object(&owner), JValue::Object(&name), JValue::Object(&who), JValue::Object(&alias), JValue::Object(&list)],
            )?
            .l()?;
        if out.is_null() {
            return Ok(Vec::new());
        }
        let array = JObjectArray::from(out);
        let length = env.get_array_length(&array)?;
        let mut items = Vec::new();
        for i in 0..length {
            let item = env.get_object_array_element(&array, i)?;
            if !item.is_null() {
                let text: String = env.get_string(&JString::from(item))?.into();
                items.push(text);
            }
        }
        Ok(items)
    })
}

/// A packet the game sends a player, through their channel's handlers in Java (PacketEvents'): `None` as it was,
/// `Some(None)` cancelled, `Some(Some(hex))` the body that goes instead.
pub fn packet_out(uuid: &str, hex: &str) -> Option<Option<String>> {
    through_channel("packetOut", uuid, hex)
}

fn through_channel(method: &str, uuid: &str, hex: &str) -> Option<Option<String>> {
    with_bridge(None, |env, bridge| {
        let uuid = env.new_string(uuid)?;
        let hex = env.new_string(hex)?;
        let out = env
            .call_static_method(bridge, method, "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", &[JValue::Object(&uuid), JValue::Object(&hex)])?
            .l()?;
        if out.is_null() {
            return Ok(Some(None));
        }
        let text: String = env.get_string(&JString::from(out))?.into();
        Ok(if text.is_empty() { None } else { Some(Some(text)) })
    })
}

/// The game's packets to players as bytes (`pl_send_raw`, fired while Java listens to `raw:out`).
pub fn listen_raw_packets() {
    use horizon_bukkit::horizon_mod::{Answer, Host, query};
    for (hook, method) in [("pl_send_raw", "packetOut"), ("pl_receive_raw", "packetIn")] {
        Host.on(hook, move |value| {
            let (Some(uuid), Some(body)) = (value["uuid"].as_str(), value["body"].as_str()) else {
                return Answer::Pass;
            };
            match through_channel(method, uuid, body) {
                None => Answer::Pass,
                Some(None) => Answer::Fail,
                Some(Some(hex)) => {
                    query(serde_json::json!({ "op": "packet_edit", "field": "raw", "value": hex }));
                    Answer::Pass
                }
            }
        });
    }
}
