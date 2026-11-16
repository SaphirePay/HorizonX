//! `getServer().getMessenger()`: plugin channels. The SDK keeps them on the plugin itself, so the
//! messenger is a handle whose calls go to this plugin's channels.
use jrt::{R, V};

pub struct MessengerH;

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if name == "getMessenger" && a.is_empty() {
        return Some(Ok(jrt::native(MessengerH)));
    }
    this.native::<MessengerH>()?;
    let channel = || a.get(1).map(V::text).unwrap_or_default();
    Some(Ok(match name {
        "registerOutgoingPluginChannel" => {
            crate::the_plugin().register_outgoing_plugin_channel(&channel());
            V::Null
        }
        "registerIncomingPluginChannel" => {
            let listener = a.get(2).cloned().unwrap_or(V::Null);
            crate::the_plugin().register_incoming_plugin_channel(&channel(), move |channel, player, data| {
                let args = [jrt::s(channel), crate::wrap(player.clone()), jrt::javaio::to_byte_array(data)];
                let _ = jrt::report("onPluginMessageReceived", jrt::call(&listener, "onPluginMessageReceived", &args));
            });
            V::Null
        }
        "unregisterOutgoingPluginChannel" | "unregisterIncomingPluginChannel" => V::Null,
        "isOutgoingChannelRegistered" | "isIncomingChannelRegistered" | "isRegistrationValid" => V::Bool(true),
        "isReservedChannel" => V::Bool(false),
        "getOutgoingChannels" | "getIncomingChannels" | "getIncomingChannelRegistrations" => jrt::list(Vec::new()),
        "hashCode" => V::Int(0),
        "toString" => jrt::s("Messenger"),
        _ => return None,
    }))
}
