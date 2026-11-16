//! The client and the relay against a fake Discord on 127.0.0.1. Nothing here contacts the real service.
mod fake;

use essentials_discord::api::{InteractionCommand, InteractionCommandArgument, InteractionCommandArgumentType, InteractionEvent, default_types};
use essentials_discord::client::model::Mentions;
use essentials_discord::client::rest::RestError;
use essentials_discord::client::secret::Secret;
use essentials_discord::client::{ClientOptions, Discord, EventSink, INTENTS, LoginError, Presence};
use essentials_discord::client::model::{Interaction, Message};
use essentials_discord::host::Host;
use fake::*;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[test]
fn login_identifies_and_comes_up() {
    let (fake, host, service) = started(&base_config());
    let auth = fake.requests_to("GET", "/users/@me")[0].auth.clone();
    assert_eq!(auth.as_deref(), Some(format!("Bot {TOKEN}").as_str()));
    let identify = &fake.frames_op(2)[0]["d"];
    assert_eq!(identify["token"], TOKEN);
    assert_eq!(identify["intents"], INTENTS);
    assert_eq!(identify["presence"]["status"], "online");
    assert_eq!(identify["presence"]["activities"][0]["name"], "Minecraft");
    assert!(service.is_live());
    let discord = service.discord().unwrap();
    assert_eq!(discord.self_user().unwrap().as_tag(), "EssBot#0000");
    assert_eq!(discord.application_id(), APP_ID);
    assert_eq!(discord.guild_ids(), [GUILD]);
    assert!(host.log_text().contains("EssBot#0000"));
    assert!(!host.log_text().contains(TOKEN));
    eventually("the emoji request", || !fake.requests_to("GET", "/emojis").is_empty());
    service.shutdown();
}

#[test]
fn heartbeats_go_out_with_the_last_sequence() {
    let fake = FakeDiscord::start();
    fake.set_heartbeat(30);
    let host = Arc::new(TestHost::default());
    let service = service_on(&fake, &host, &base_config());
    service.startup().unwrap();
    eventually("three heartbeats", || fake.frames_op(1).len() >= 3);
    assert_eq!(fake.frames_op(1).last().unwrap()["d"], 2, "READY and GUILD_CREATE were sequences 1 and 2");
    assert_eq!(fake.connections(), 1, "an acknowledged heartbeat keeps the connection");
    service.shutdown();
}

#[test]
fn a_heartbeat_with_no_ack_reconnects_and_resumes() {
    let fake = FakeDiscord::start();
    fake.set_heartbeat(30);
    let host = Arc::new(TestHost::default());
    let service = service_on(&fake, &host, &base_config());
    service.startup().unwrap();
    fake.stop_acking();
    eventually("a second connection", || fake.connections() >= 2);
    eventually("a RESUME", || !fake.frames_op(6).is_empty());
    assert_eq!(fake.frames_op(2).len(), 1, "no second IDENTIFY");
    service.shutdown();
}

#[test]
fn a_dropped_socket_resumes_the_session() {
    let (fake, host, service) = started(&base_config());
    fake.command(Command::Drop);
    eventually("a RESUME", || !fake.frames_op(6).is_empty());
    let resume = &fake.frames_op(6)[0]["d"];
    assert_eq!(resume["session_id"], "fake-session");
    assert_eq!(resume["seq"], 2);
    assert_eq!(resume["token"], TOKEN);
    assert_eq!(fake.frames_op(2).len(), 1, "no second IDENTIFY");
    // And it still works: an event from the new connection gets through.
    fake.event("MESSAGE_CREATE", message_event(PRIMARY, "after the drop"));
    eventually("the message", || {
        host.pump();
        !host.delivered.lock().unwrap().is_empty()
    });
    service.shutdown();
}

#[test]
fn a_refused_resume_identifies_again() {
    let (fake, _host, service) = started(&base_config());
    fake.refuse_next_resume();
    fake.command(Command::Drop);
    eventually("a second IDENTIFY", || fake.frames_op(2).len() == 2);
    service.shutdown();
}

#[test]
fn a_close_code_that_loses_the_session_identifies_again() {
    let (fake, _host, service) = started(&base_config());
    fake.command(Command::Close(4009));
    eventually("a second IDENTIFY", || fake.frames_op(2).len() == 2);
    assert!(fake.frames_op(6).is_empty(), "no RESUME");
    service.shutdown();
}

#[test]
fn a_reconnect_op_resumes() {
    let (fake, _host, service) = started(&base_config());
    fake.command(Command::Close(4000));
    eventually("a RESUME", || !fake.frames_op(6).is_empty());
    service.shutdown();
}

struct Silent;

impl EventSink for Silent {
    fn on_message(&self, _message: &Message) {}
    fn on_interaction(&self, _interaction: Interaction) {}
    fn on_fatal(&self, _reason: &str) {}
}

fn client_options(fake: &FakeDiscord, token: &str) -> ClientOptions {
    let mut options = ClientOptions::new(Secret::new(token), Presence { status: "online".to_owned(), activity: None });
    options.api_base = format!("http://127.0.0.1:{}", fake.rest_port);
    options.reconnect_base = Duration::from_millis(10);
    options.identify_gap = Duration::ZERO;
    options.login_timeout = Duration::from_secs(10);
    options
}

#[test]
fn close_codes_that_must_stop_stop_retrying() {
    for (code, text) in [(4004, "token"), (4014, "intents"), (4013, "intents"), (4011, "harding"), (4012, "version")] {
        let fake = FakeDiscord::start();
        fake.close_on_identify(code);
        let host: Arc<dyn Host> = Arc::new(TestHost::default());
        let error = Discord::connect(client_options(&fake, TOKEN), host, Arc::new(Silent)).err().expect("the login should fail");
        assert!(error.to_string().contains(text), "{code}: {error}");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(fake.connections(), 1, "{code} must not be retried");
    }
}

#[test]
fn a_refused_token_fails_the_login_and_is_never_logged() {
    let fake = FakeDiscord::start();
    fake.answer_once("GET", "/users/@me", 401, &[], r#"{"message": "401: Unauthorized", "code": 0}"#);
    let host = Arc::new(TestHost::default());
    let service = service_on(&fake, &host, &base_config());
    let error = service.startup().unwrap_err();
    assert_eq!(error, LoginError::InvalidToken.to_string());
    assert!(!error.contains(TOKEN));
    assert!(!host.log_text().contains(TOKEN));
    assert!(service.discord().is_none());
    assert_eq!(fake.connections(), 0, "no gateway with a refused token");
}

#[test]
fn a_token_refused_by_the_gateway_says_so() {
    let fake = FakeDiscord::start();
    fake.close_on_identify(4004);
    let host: Arc<dyn Host> = Arc::new(TestHost::default());
    let error = Discord::connect(client_options(&fake, TOKEN), host, Arc::new(Silent)).err().unwrap();
    assert!(matches!(error, LoginError::InvalidToken));
    assert_eq!(format!("{:?}", client_options(&fake, TOKEN).token), "Secret(***)");
}

#[test]
fn the_placeholder_token_never_logs_in() {
    let fake = FakeDiscord::start();
    let host = Arc::new(TestHost::default());
    let service = service_on(&fake, &host, "token: \"INSERT-TOKEN-HERE\"\n");
    assert!(service.startup().is_err());
    assert!(fake.requests().is_empty());
}

#[test]
fn a_429_waits_and_goes_again() {
    let (fake, _host, service) = started(&base_config());
    fake.answer_once("POST", "/messages", 429, &[("Retry-After", "1")], r#"{"message": "You are being rate limited.", "retry_after": 0.25, "global": false}"#);
    let discord = service.discord().unwrap();
    assert!(discord.send_message(PRIMARY, "one", Mentions::NoGroups, None).wait(Duration::from_secs(5)).unwrap().is_ok());
    let tries = fake.requests_to("POST", "/messages");
    assert_eq!(tries.len(), 2);
    assert!(tries[1].at - tries[0].at >= Duration::from_millis(240), "waited {:?}", tries[1].at - tries[0].at);
    service.shutdown();
}

#[test]
fn a_global_429_holds_every_route() {
    let (fake, _host, service) = started(&base_config());
    fake.answer_once("POST", "/messages", 429, &[("X-RateLimit-Global", "true")], r#"{"retry_after": 0.25, "global": true}"#);
    let discord = service.discord().unwrap();
    let first = discord.send_message(PRIMARY, "one", Mentions::NoGroups, None);
    eventually("the limited try", || !fake.requests_to("POST", "/messages").is_empty());
    std::thread::sleep(Duration::from_millis(20));
    let other = discord.send_message(STAFF, "two", Mentions::NoGroups, None);
    assert!(first.wait(Duration::from_secs(5)).unwrap().is_ok());
    assert!(other.wait(Duration::from_secs(5)).unwrap().is_ok());
    let tries = fake.requests_to("POST", "/messages");
    let limited = tries[0].at;
    assert!(tries[1..].iter().all(|row| row.at - limited >= Duration::from_millis(240)));
    service.shutdown();
}

#[test]
fn an_empty_bucket_holds_the_next_request_back() {
    let (fake, _host, service) = started(&base_config());
    fake.answer_once("POST", "/messages", 200, &[("X-RateLimit-Bucket", "abc"), ("X-RateLimit-Remaining", "0"), ("X-RateLimit-Reset-After", "0.3")], r#"{"id": "1"}"#);
    let discord = service.discord().unwrap();
    let first = discord.send_message(PRIMARY, "one", Mentions::NoGroups, None);
    let second = discord.send_message(PRIMARY, "two", Mentions::NoGroups, None);
    assert!(first.wait(Duration::from_secs(5)).unwrap().is_ok());
    assert!(second.wait(Duration::from_secs(5)).unwrap().is_ok());
    let tries = fake.requests_to("POST", "/messages");
    assert_eq!(tries[0].body["content"], "one");
    assert_eq!(tries[1].body["content"], "two");
    assert!(tries[1].at - tries[0].at >= Duration::from_millis(280), "waited {:?}", tries[1].at - tries[0].at);
    service.shutdown();
}

#[test]
fn a_server_error_is_tried_again_and_a_refusal_is_not() {
    let (fake, _host, service) = started(&base_config());
    let discord = service.discord().unwrap();
    fake.answer_once("POST", "/messages", 502, &[], "bad gateway");
    assert!(discord.send_message(PRIMARY, "one", Mentions::NoGroups, None).wait(Duration::from_secs(5)).unwrap().is_ok());
    assert_eq!(fake.requests_to("POST", "/messages").len(), 2);
    fake.answer_once("POST", "/messages", 403, &[], r#"{"code": 50013, "message": "Missing Permissions"}"#);
    let error = discord.send_message(PRIMARY, "two", Mentions::NoGroups, None).wait(Duration::from_secs(5)).unwrap().unwrap_err();
    assert!(matches!(error, RestError::Status { status: 403, code: 50013, .. }));
    assert_eq!(fake.requests_to("POST", "/messages").len(), 3);
    service.shutdown();
}

#[test]
fn discord_chat_reaches_the_game() {
    let (fake, host, service) = started(&base_config());
    fake.event("MESSAGE_CREATE", message_event(PRIMARY, "hello \u{1F604} <:party:4242>"));
    eventually("the relay", || {
        host.pump();
        !host.delivered.lock().unwrap().is_empty()
    });
    let relay = host.delivered.lock().unwrap()[0].clone();
    assert_eq!(relay.keys, ["primary"]);
    assert_eq!(relay.formatted, "\u{a7}6[#general] \u{a7}3someone#0000\u{a7}7: \u{a7}fhello :smile: :party:");
    assert_eq!(relay.raw, "hello \u{1F604} <:party:4242>");
    assert_eq!(relay.channel.name, "general");
    assert!(relay.member.unwrap().has_role(&COLOR_ROLE.to_string()));
    service.shutdown();
}

#[test]
fn what_java_skips_is_skipped() {
    let (fake, host, service) = started(&base_config());
    let mut bot = message_event(PRIMARY, "from a bot");
    bot["author"]["bot"] = json!(true);
    let mut own = message_event(PRIMARY, "from me");
    own["author"]["id"] = json!(SELF_ID.to_string());
    own["author"]["bot"] = json!(true);
    let mut hook = message_event(PRIMARY, "from a webhook");
    hook["webhook_id"] = json!("1");
    hook.as_object_mut().unwrap().remove("member");
    for event in [bot, own, hook, message_event(PRIMARY, "!filtered"), message_event(CONSOLE + 1, "unknown channel"), message_event(CONSOLE, "   ")] {
        fake.event("MESSAGE_CREATE", event);
    }
    fake.event("MESSAGE_CREATE", message_event(STAFF, "marker"));
    eventually("the marker", || {
        host.pump();
        !host.delivered.lock().unwrap().is_empty()
    });
    let delivered = host.delivered.lock().unwrap();
    assert_eq!(delivered.len(), 1, "{:?}", delivered.iter().map(|relay| &relay.raw).collect::<Vec<_>>());
    assert_eq!(delivered[0].raw, "marker");
    drop(delivered);
    service.shutdown();
}

#[test]
fn colours_go_with_the_permitted_roles_and_attachments_with_the_text() {
    let (fake, host, service) = started(&base_config());
    let mut plain = message_event(PRIMARY, "&cplain");
    plain["attachments"] = json!([{ "url": "https://cdn/x.png" }]);
    let mut admin = message_event(PRIMARY, "&cadmin");
    admin["member"]["roles"] = json!([ADMIN_ROLE.to_string()]);
    fake.event("MESSAGE_CREATE", plain);
    fake.event("MESSAGE_CREATE", admin);
    eventually("both", || {
        host.pump();
        host.delivered.lock().unwrap().len() == 2
    });
    let delivered = host.delivered.lock().unwrap();
    assert!(delivered[0].formatted.ends_with("plain https://cdn/x.png"), "{}", delivered[0].formatted);
    assert!(delivered[1].formatted.ends_with("\u{a7}cadmin"), "{}", delivered[1].formatted);
    drop(delivered);
    service.shutdown();
}

#[test]
fn game_chat_reaches_discord() {
    let (fake, host, service) = started(&base_config());
    service.dispatch_discord_message(default_types::CHAT, "\u{a7}cSteve: hello", false, None, None, Some("uuid".to_owned()));
    host.pump();
    eventually("the message", || !fake.requests_to("POST", &format!("/channels/{PRIMARY}/messages")).is_empty());
    let sent = &fake.requests_to("POST", "/messages")[0];
    assert_eq!(sent.body["content"], "Steve: hello");
    assert_eq!(sent.body["allowed_mentions"], json!({ "parse": ["users"] }));
    assert_eq!(sent.auth.as_deref(), Some(format!("Bot {TOKEN}").as_str()));
    service.dispatch_discord_message(default_types::KICK, "kicked", true, None, None, None);
    host.pump();
    eventually("the staff message", || !fake.requests_to("POST", &format!("/channels/{STAFF}/messages")).is_empty());
    assert_eq!(fake.requests_to("POST", &format!("/channels/{STAFF}/messages"))[0].body["allowed_mentions"], json!({ "parse": ["users", "roles", "everyone"] }));
    service.dispatch_discord_message(default_types::PRIVATE_CHAT, "hidden", false, None, None, None);
    host.pump();
    service.shutdown();
    assert!(fake.requests().iter().all(|row| row.body["content"] != "hidden"), "a type set to none sends nothing");
}

#[test]
fn a_listener_can_stop_a_message() {
    fn cancel_all(event: &mut essentials_discord::api::DiscordMessageEvent) {
        event.cancelled = true;
    }
    let (fake, host, service) = started(&base_config());
    service.add_message_listener(cancel_all);
    service.dispatch_discord_message(default_types::CHAT, "stopped", false, None, None, None);
    host.pump();
    std::thread::sleep(Duration::from_millis(50));
    assert!(fake.requests_to("POST", "/messages").is_empty());
    service.shutdown();
}

#[test]
fn with_avatars_on_chat_goes_by_webhook() {
    let (fake, host, service) = started(&format!("{}show-avatar: true\n", base_config()));
    assert!(!fake.requests_to("POST", &format!("/channels/{PRIMARY}/webhooks")).is_empty());
    service.dispatch_discord_message(default_types::CHAT, "by hook", false, Some("https://a/b.png".to_owned()), Some("Steve".to_owned()), None);
    host.pump();
    eventually("the webhook", || !fake.requests_to("POST", "/webhooks/8800/fake-hook-token").is_empty());
    let sent = &fake.requests_to("POST", "/webhooks/8800/")[0];
    assert!(sent.path.ends_with("?wait=false"));
    assert_eq!(sent.auth, None, "a webhook's token is in its URL");
    assert_eq!(sent.body["content"], "by hook");
    assert_eq!(sent.body["username"], "Steve");
    assert_eq!(sent.body["avatar_url"], "https://a/b.png");
    assert_eq!(sent.body["allowed_mentions"], json!({ "parse": ["users"] }));
    fake.answer_once("POST", "/webhooks/8800/", 429, &[], r#"{"retry_after": 0.01}"#);
    service.shutdown();
    assert!(!host.log_text().contains("fake-hook-token"));
}

struct Echo {
    ran: Arc<AtomicBool>,
    arguments: Vec<InteractionCommandArgument>,
}

impl InteractionCommand for Echo {
    fn is_disabled(&self) -> bool {
        false
    }
    fn is_ephemeral(&self) -> bool {
        true
    }
    fn get_name(&self) -> String {
        "echo".to_owned()
    }
    fn get_description(&self) -> String {
        "x".repeat(150)
    }
    fn get_arguments(&self) -> Vec<InteractionCommandArgument> {
        self.arguments.clone()
    }
    fn on_command(&self, event: InteractionEvent) {
        self.ran.store(true, Ordering::SeqCst);
        event.reply(&format!("\u{a7}cgot {}", event.get_string_argument("text").unwrap_or_default()));
        event.reply(&format!("in {}", event.get_channel_id()));
    }
}

fn interaction(name: &str, member_roles: serde_json::Value) -> serde_json::Value {
    json!({
        "id": "6000",
        "type": 2,
        "token": "fake-interaction-token",
        "application_id": APP_ID.to_string(),
        "guild_id": GUILD.to_string(),
        "channel_id": PRIMARY.to_string(),
        "member": { "user": { "id": USER_ID.to_string(), "username": "someone", "discriminator": "0" }, "roles": member_roles },
        "data": { "name": name, "options": [{ "name": "text", "type": 3, "value": "hi" }] },
    })
}

fn echo(ran: &Arc<AtomicBool>) -> Arc<dyn InteractionCommand> {
    Arc::new(Echo { ran: ran.clone(), arguments: vec![InteractionCommandArgument::new("text", "what to say", InteractionCommandArgumentType::String, true)] })
}

#[test]
fn a_slash_command_registers_defers_runs_and_answers() {
    let (fake, host, service) = started(&base_config());
    let ran = Arc::new(AtomicBool::new(false));
    service.interaction_controller().register_command(echo(&ran)).unwrap();
    service.interaction_controller().process_batch_registration();
    eventually("the registration", || service.interaction_controller().get_command("echo").is_some());
    let put = &fake.requests_to("PUT", &format!("/applications/{APP_ID}/guilds/{GUILD}/commands"))[0];
    assert_eq!(put.body[0]["name"], "echo");
    assert_eq!(put.body[0]["description"].as_str().unwrap().chars().count(), 100);
    assert_eq!(put.body[0]["options"][0], json!({ "type": 3, "name": "text", "description": "what to say", "required": true }));

    fake.event("INTERACTION_CREATE", interaction("echo", json!([])));
    eventually("the defer", || !fake.requests_to("POST", "/interactions/6000/fake-interaction-token/callback").is_empty());
    let defer = &fake.requests_to("POST", "/callback")[0];
    assert_eq!(defer.body, json!({ "type": 5, "data": { "flags": 64 } }));
    assert_eq!(defer.auth, None);
    eventually("the command", || {
        host.pump();
        ran.load(Ordering::SeqCst)
    });
    eventually("both edits", || fake.requests_to("PATCH", "@original").len() == 2);
    let edits = fake.requests_to("PATCH", &format!("/webhooks/{APP_ID}/fake-interaction-token/messages/@original"));
    assert_eq!(edits[0].body["content"], "got hi");
    assert_eq!(edits[1].body["content"], format!("got hi\nin {PRIMARY}"));
    assert_eq!(edits[0].body["allowed_mentions"], json!({ "parse": ["users"] }));
    assert!(edits[0].at >= defer.at, "an edit can't pass the defer");
    service.shutdown();
    assert_eq!(fake.requests_to("PUT", "/commands").last().unwrap().body, json!([]), "shutdown takes the commands away");
    assert!(!host.log_text().contains("fake-interaction-token"));
}

#[test]
fn a_command_not_allowed_is_refused_after_the_defer() {
    struct Execute(Arc<AtomicBool>);
    impl InteractionCommand for Execute {
        fn is_disabled(&self) -> bool {
            false
        }
        fn is_ephemeral(&self) -> bool {
            false
        }
        fn get_name(&self) -> String {
            "execute".to_owned()
        }
        fn get_description(&self) -> String {
            "run".to_owned()
        }
        fn get_arguments(&self) -> Vec<InteractionCommandArgument> {
            Vec::new()
        }
        fn on_command(&self, _event: InteractionEvent) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let (fake, host, service) = started(&base_config());
    let ran = Arc::new(AtomicBool::new(false));
    service.interaction_controller().register_command(Arc::new(Execute(ran.clone()))).unwrap();
    service.interaction_controller().process_batch_registration();
    eventually("the registration", || service.interaction_controller().get_command("execute").is_some());
    fake.event("INTERACTION_CREATE", interaction("execute", json!([COLOR_ROLE.to_string()])));
    eventually("the refusal", || !fake.requests_to("PATCH", "@original").is_empty());
    host.pump();
    assert!(!ran.load(Ordering::SeqCst));
    assert_eq!(fake.requests_to("POST", "/callback")[0].body, json!({ "type": 5, "data": {} }));
    assert!(fake.requests_to("PATCH", "@original")[0].body["content"].as_str().unwrap().contains("You do not have access to that command."));
    fake.event("INTERACTION_CREATE", interaction("execute", json!([ADMIN_ROLE.to_string()])));
    eventually("the command", || {
        host.pump();
        ran.load(Ordering::SeqCst)
    });
    service.shutdown();
}

#[test]
fn an_unknown_slash_command_is_left_alone() {
    let (fake, host, service) = started(&base_config());
    fake.event("INTERACTION_CREATE", interaction("nope", json!([])));
    fake.event("MESSAGE_CREATE", message_event(PRIMARY, "marker"));
    eventually("the marker", || {
        host.pump();
        !host.delivered.lock().unwrap().is_empty()
    });
    assert!(fake.requests_to("POST", "/callback").is_empty());
    service.shutdown();
}

#[test]
fn the_console_channel_relays_the_log_and_runs_commands() {
    let folder = std::env::temp_dir().join(format!("essx-console-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let log = folder.join("latest.log");
    std::fs::write(&log, "[10:00:00] [Server thread/INFO]: already here\n").unwrap();
    essentials_discord::console::set_log_file(log.clone());
    let config = format!("{}\nconsole-override: 1\n", base_config()).replace("console:\n  channel: 000000000000000000\n", &format!("console:\n  channel: {CONSOLE}\n  command-relay: true\n  log-level: warn\n  console-filter: \"secret\"\n"));
    let (fake, host, service) = started(&config);
    let hook_made = fake.requests_to("POST", &format!("/channels/{CONSOLE}/webhooks"));
    assert_eq!(hook_made[0].body["name"], "EssX Console Relay");
    std::thread::sleep(Duration::from_millis(400));
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(file, "[10:00:01] [Server thread/INFO]: too quiet").unwrap();
        writeln!(file, "[10:00:02] [Server thread/WARN]: my_name *matters*").unwrap();
        writeln!(file, "[10:00:03] [Server thread/ERROR]: the secret word").unwrap();
    }
    eventually("the console line", || !fake.requests_to("POST", "/webhooks/8800/").is_empty());
    let sent = fake.requests_to("POST", "/webhooks/8800/")[0].clone();
    let content = sent.body["content"].as_str().unwrap();
    assert!(content.contains("WARN] my\\_name \\*matters\\*"), "{content}");
    assert!(content.starts_with("\n[<t:"), "{content}");
    assert!(!content.contains("too quiet") && !content.contains("secret") && !content.contains("already here"), "{content}");
    assert_eq!(sent.body["username"], "EssentialsX Console Relay");

    fake.event("MESSAGE_CREATE", message_event(CONSOLE, "say hi"));
    eventually("the command", || {
        host.pump();
        !host.commands.lock().unwrap().is_empty()
    });
    assert_eq!(host.commands.lock().unwrap()[0], "say hi");
    eventually("the reply", || !fake.requests_to("POST", &format!("/channels/{CONSOLE}/messages")).is_empty());
    let reply = fake.requests_to("POST", &format!("/channels/{CONSOLE}/messages"))[0].clone();
    assert_eq!(reply.body["content"], "ran say hi");
    assert_eq!(reply.body["message_reference"]["message_id"], "5000");
    let mut bot = message_event(CONSOLE, "say bot");
    bot["author"]["bot"] = json!(true);
    fake.event("MESSAGE_CREATE", bot);
    fake.event("MESSAGE_CREATE", message_event(PRIMARY, "marker"));
    eventually("the marker", || {
        host.pump();
        host.delivered.lock().unwrap().iter().any(|relay| relay.raw == "marker")
    });
    assert_eq!(host.commands.lock().unwrap().len(), 1, "a bot can't run commands unless told it may");
    service.shutdown();
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn members_roles_and_private_messages_go_through_the_service() {
    let (fake, _host, service) = started(&base_config());
    let member = service.get_member_by_id(&USER_ID.to_string()).wait(Duration::from_secs(5)).unwrap().unwrap();
    assert_eq!(member.get_tag(), "someone#0000");
    assert!(member.has_role(&COLOR_ROLE.to_string()));
    assert!(service.get_member_by_id("123").wait(Duration::from_secs(5)).unwrap().is_none());
    assert!(service.try_member_by_id("123").wait(Duration::from_secs(5)).unwrap().unwrap().is_none());
    fake.answer_once("GET", "/members/", 500, &[], "");
    fake.answer_once("GET", "/members/", 500, &[], "");
    fake.answer_once("GET", "/members/", 500, &[], "");
    assert!(service.try_member_by_id("456").wait(Duration::from_secs(10)).unwrap().is_err(), "a failure isn't someone leaving");
    let admin = service.get_role(&ADMIN_ROLE.to_string()).unwrap();
    assert!(admin.can_interact());
    let colour = service.get_role(&COLOR_ROLE.to_string()).unwrap();
    assert_eq!(colour.get_color_raw(), 0xff0000);
    assert!(service.get_role(&GUILD.to_string()).unwrap().is_public_role());
    assert!(!service.get_role(&BOT_ROLE.to_string()).unwrap().can_interact());
    service.modify_member_roles(&member, std::slice::from_ref(&admin), std::slice::from_ref(&colour)).wait(Duration::from_secs(5)).unwrap();
    assert_eq!(fake.requests_to("PATCH", &format!("/guilds/{GUILD}/members/{USER_ID}"))[0].body, json!({ "roles": [ADMIN_ROLE.to_string()] }));
    assert_eq!(member.send_private_message("psst").wait(Duration::from_secs(5)), Some(true));
    assert_eq!(fake.requests_to("POST", "/users/@me/channels")[0].body, json!({ "recipient_id": USER_ID.to_string() }));
    assert_eq!(fake.requests_to("POST", "/channels/7700/messages")[0].body["content"], "psst");
    service.shutdown();
}

#[test]
fn presence_changes_reach_the_gateway() {
    let (fake, _host, service) = started(&base_config());
    service.discord().unwrap().set_presence(&Presence { status: "dnd".to_owned(), activity: Some((3, "you".to_owned())) });
    eventually("the presence", || !fake.frames_op(3).is_empty());
    assert_eq!(fake.frames_op(3)[0]["d"]["status"], "dnd");
    service.shutdown();
}

#[test]
fn the_cache_follows_the_gateway() {
    let (fake, _host, service) = started(&base_config());
    fake.event("GUILD_ROLE_CREATE", json!({ "guild_id": GUILD.to_string(), "role": { "id": "42", "name": "New", "position": 1, "permissions": "0" } }));
    fake.event("CHANNEL_CREATE", json!({ "id": "43", "guild_id": GUILD.to_string(), "name": "new-channel", "type": 0, "position": 9 }));
    fake.event("GUILD_EMOJIS_UPDATE", json!({ "guild_id": GUILD.to_string(), "emojis": [{ "id": "44", "name": "fresh" }] }));
    fake.event("GUILD_ROLE_DELETE", json!({ "guild_id": GUILD.to_string(), "role_id": COLOR_ROLE.to_string() }));
    let discord = service.discord().unwrap();
    eventually("all of them", || discord.guild(GUILD).is_some_and(|guild| guild.roles.contains_key(&42) && guild.channels.contains_key(&43) && guild.emojis.len() == 1 && !guild.roles.contains_key(&COLOR_ROLE)));
    assert_eq!(service.parse_message_emotes("hi :fresh:"), "hi <:fresh:44>");
    service.shutdown();
}

#[test]
fn shutdown_ends_everything_and_says_goodbye() {
    let (fake, host, service) = started(&format!("{}show-avatar: true\n", base_config()));
    let discord = service.discord().unwrap();
    let started_at = Instant::now();
    service.shutdown();
    assert!(started_at.elapsed() < Duration::from_secs(8), "{:?}", started_at.elapsed());
    assert!(service.discord().is_none());
    assert!(!service.is_live());
    assert!(discord.is_stopped());
    eventually("the close frame", || fake.closes_seen() == [1000]);
    let after = fake.connections();
    assert_eq!(discord.send_message(PRIMARY, "too late", Mentions::All, None).wait(Duration::from_secs(1)), Some(Err(RestError::Shutdown)));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake.connections(), after, "nothing reconnects");
    host.pump();
    let _ = host;
}

#[test]
fn the_stop_message_goes_out_before_the_client_does() {
    let (fake, _host, service) = started(&base_config());
    service.shutdown();
    let sent = fake.requests_to("POST", &format!("/channels/{PRIMARY}/messages"));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].body["content"], ":octagonal_sign: The server has stopped!");
    assert_eq!(sent[0].body["allowed_mentions"], json!({ "parse": ["users", "roles", "everyone"] }));
}

#[test]
fn a_guild_it_is_not_in_fails_the_startup() {
    let fake = FakeDiscord::start();
    let host = Arc::new(TestHost::default());
    let service = service_on(&fake, &host, &base_config().replace(&format!("guild: {GUILD}"), "guild: 999999999999999999"));
    let error = service.startup().unwrap_err();
    assert!(!error.is_empty());
    assert!(service.is_invalid_startup());
    service.shutdown();
    assert!(service.discord().is_none());
}

/// The gateway over real TLS, as Discord's is: the client's reads time out over and over between
/// frames, and a timeout in the middle of TLS must lose nothing.
#[test]
fn the_gateway_works_over_tls_through_idle_time() {
    use std::net::TcpListener;
    use std::sync::Mutex;
    use tungstenite::Message as Frame;

    let key = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let certificate = key.cert.der().to_vec();
    let private = rustls::pki_types::PrivateKeyDer::try_from(key.key_pair.serialize_der()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![rustls::pki_types::CertificateDer::from(certificate.clone())], private)
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let beats = Arc::new(Mutex::new(0));
    let counted = beats.clone();
    std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_nodelay(true).unwrap();
        let tls = rustls::StreamOwned::new(rustls::ServerConnection::new(Arc::new(config)).unwrap(), tcp);
        let mut socket = tungstenite::accept(tls).unwrap();
        socket.send(Frame::text(json!({ "op": 10, "d": { "heartbeat_interval": 150 } }).to_string())).unwrap();
        let ready = json!({ "op": 0, "t": "READY", "s": 1, "d": {
            "session_id": "tls-session",
            "user": { "id": SELF_ID.to_string(), "username": "EssBot", "discriminator": "0" },
            "application": { "id": APP_ID.to_string() },
            "guilds": [],
        } });
        let mut sent_message = false;
        while let Ok(frame) = socket.read() {
            let Frame::Text(text) = frame else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
            match value["op"].as_u64() {
                Some(2) => socket.send(Frame::text(ready.to_string())).unwrap(),
                Some(1) => {
                    socket.send(Frame::text(json!({ "op": 11 }).to_string())).unwrap();
                    let mut count = counted.lock().unwrap();
                    *count += 1;
                    if *count >= 4 && !sent_message {
                        sent_message = true;
                        socket.send(Frame::text(json!({ "op": 0, "t": "MESSAGE_CREATE", "s": 2, "d": message_event(PRIMARY, "over tls") }).to_string())).unwrap();
                    }
                }
                _ => {}
            }
        }
    });
    struct Tell(Mutex<std::sync::mpsc::Sender<String>>);
    impl EventSink for Tell {
        fn on_message(&self, message: &Message) {
            self.0.lock().unwrap().send(message.content.clone()).unwrap();
        }
        fn on_interaction(&self, _interaction: Interaction) {}
        fn on_fatal(&self, _reason: &str) {}
    }
    let (tell, seen) = std::sync::mpsc::channel();
    let fake = FakeDiscord::start();
    fake.answer_once("GET", "/gateway/bot", 200, &[], &json!({ "url": format!("wss://localhost:{port}"), "session_start_limit": { "remaining": 1 } }).to_string());
    let mut options = client_options(&fake, TOKEN);
    options.extra_roots = vec![certificate];
    let host: Arc<dyn Host> = Arc::new(TestHost::default());
    let discord = Discord::connect(options, host, Arc::new(Tell(Mutex::new(tell)))).unwrap();
    assert_eq!(seen.recv_timeout(Duration::from_secs(10)).unwrap(), "over tls");
    assert!(*beats.lock().unwrap() >= 4);
    discord.shutdown();
}

#[test]
fn startup_uses_the_gateway_address_rest_gives() {
    let (fake, _host, service) = started(&base_config());
    assert_eq!(fake.requests_to("GET", "/gateway/bot").len(), 1);
    assert_eq!(fake.connections(), 1);
    let _ = fake.gateway_port;
    service.shutdown();
}
