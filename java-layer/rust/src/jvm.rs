//! Finding Java, starting it lean, and calling into `horizon.javalayer.Bridge`.
use horizon_bukkit::Plugin;
use jni::objects::{GlobalRef, JClass, JObject, JValue};
use jni::{JNIEnv, JavaVM, sys};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Java {
    pub vm: JavaVM,
    pub bridge: GlobalRef,
    pub plugin: Plugin,
}

pub static JAVA: Mutex<Option<std::sync::Arc<Java>>> = Mutex::new(None);

pub fn java() -> Option<std::sync::Arc<Java>> {
    JAVA.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The jars Java plugins come in: `plugins/*.jar`.
pub fn plugin_jars(plugins: &Path) -> Vec<PathBuf> {
    let mut jars: Vec<PathBuf> = std::fs::read_dir(plugins)
        .map(|dir| dir.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jar")) && p.is_file()).collect())
        .unwrap_or_default();
    jars.sort();
    jars
}

/// `JAVA_VERSION="25.0.1"` in a Java home's `release` file: its feature number (25).
fn java_version(home: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(home.join("release")).ok()?;
    let line = text.lines().find(|l| l.starts_with("JAVA_VERSION="))?;
    let version = line.trim_start_matches("JAVA_VERSION=").trim_matches('"');
    let first = version.split(['.', '-', '+']).next()?;
    first.parse().ok().map(|v: u32| if v == 1 { 8 } else { v })
}

fn jvm_library(home: &Path) -> Option<PathBuf> {
    let candidates = if cfg!(windows) {
        vec![home.join("bin/server/jvm.dll"), home.join("jre/bin/server/jvm.dll")]
    } else if cfg!(target_os = "macos") {
        vec![home.join("lib/server/libjvm.dylib")]
    } else {
        vec![home.join("lib/server/libjvm.so"), home.join("jre/lib/server/libjvm.so")]
    };
    candidates.into_iter().find(|p| p.is_file())
}

/// Every place a Java home may be, best first: the owner's choice, one beside the server, JAVA_HOME, `java` on
/// the PATH, then the usual install folders. The first of version 25 or newer wins (Paper 26.3's API is 25).
fn java_homes(server: &Path) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Ok(home) = std::env::var("HORIZON_JAVA_HOME") {
        homes.push(PathBuf::from(home));
    }
    homes.push(server.join("java-layer").join("runtime"));
    if let Ok(home) = std::env::var("JAVA_HOME") {
        homes.push(PathBuf::from(home));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let java = dir.join(if cfg!(windows) { "java.exe" } else { "java" });
            if java.is_file() {
                if let Ok(real) = std::fs::canonicalize(&java) {
                    if let Some(home) = real.parent().and_then(Path::parent) {
                        homes.push(plain(home));
                    }
                }
            }
        }
    }
    let roots: Vec<PathBuf> = if cfg!(windows) {
        let mut roots = vec![PathBuf::from(r"C:\Program Files\Java"), PathBuf::from(r"C:\Program Files\Eclipse Adoptium"), PathBuf::from(r"C:\Program Files\Microsoft"), PathBuf::from(r"C:\Program Files\Zulu")];
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            roots.push(Path::new(&local).join("Programs").join("Eclipse Adoptium"));
        }
        roots
    } else {
        vec![PathBuf::from("/usr/lib/jvm"), PathBuf::from("/opt/java"), PathBuf::from("/Library/Java/JavaVirtualMachines")]
    };
    for root in roots {
        if let Ok(dir) = std::fs::read_dir(&root) {
            let mut found: Vec<PathBuf> = dir.filter_map(Result::ok).map(|e| e.path()).collect();
            found.sort();
            found.reverse();
            for home in found {
                homes.push(home.join("Contents").join("Home"));
                homes.push(home);
            }
        }
    }
    homes
}

/// A path without Windows' `\\?\` prefix (what `canonicalize` gives): Java finds its own files (lib/modules) from
/// where jvm.dll was loaded, and with that prefix it finds none ("jimage file name is null").
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

/// The Java to run: the owner's choice (HORIZON_JAVA_HOME, or java-layer/runtime) if it is 25 or newer, else the
/// NEWEST one found, so a plugin built for Java 26 (class version 70) or later loads as long as such a Java is
/// installed. A newer Java runs the Paper API (built for 25) as well.
fn find_java(server: &Path) -> Result<(PathBuf, PathBuf), String> {
    let mut seen = Vec::new();
    let mut best: Option<(u32, PathBuf, PathBuf)> = None;
    let chosen: Vec<PathBuf> = std::env::var("HORIZON_JAVA_HOME").ok().map(PathBuf::from).into_iter().chain([server.join("java-layer").join("runtime")]).map(|h| plain(&h)).collect();
    for home in java_homes(server).iter().map(|h| plain(h)) {
        let Some(version) = java_version(&home) else { continue };
        if version < 25 {
            seen.push(format!("{} (Java {version})", home.display()));
            continue;
        }
        let Some(library) = jvm_library(&home) else { continue };
        if chosen.contains(&home) {
            return Ok((home, library));
        }
        if best.as_ref().is_none_or(|(have, _, _)| version > *have) {
            best = Some((version, home, library));
        }
    }
    if let Some((_, home, library)) = best {
        return Ok((home, library));
    }
    Err(if seen.is_empty() {
        "Java 25 or newer is needed for Java plugins and none was found (set HORIZON_JAVA_HOME, or put one in java-layer/runtime)".to_owned()
    } else {
        format!("Java 25 or newer is needed for Java plugins; only older ones were found: {}", seen.join(", "))
    })
}

/// `plugins/JavaLayer/config.yml`: `memory: 1G` (Java's heap cap) and `options: -X...` (more JVM flags).
fn settings(plugin: &Plugin) -> (String, Vec<String>) {
    let folder = plugin.get_data_folder();
    let path = folder.join("config.yml");
    if !path.is_file() {
        let _ = std::fs::create_dir_all(&folder);
        let _ = std::fs::write(&path, "# Java plugins (plugins/*.jar) run in one Java VM, started only when there is a jar.\n# Heap cap for all Java plugins together (a cap, not what is used: the heap grows only as needed and gives memory back).\nmemory: 1G\n# More JVM flags, space separated (e.g. -XX:+UseZGC).\noptions: ''\n");
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let value = |key: &str| text.lines().find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().trim_matches(['\'', '"']).to_owned()));
    let memory = std::env::var("HORIZON_JAVA_MEMORY").ok().or_else(|| value("memory:")).filter(|m| !m.is_empty()).unwrap_or_else(|| "1G".to_owned());
    let options = value("options:").unwrap_or_default().split_whitespace().map(str::to_owned).collect();
    (memory, options)
}

/// The API and the layer's own jar: `java-layer/lib/*.jar` beside the server (or HORIZON_JAVA_LIBS).
fn class_path(server: &Path) -> Result<String, String> {
    let dir = std::env::var("HORIZON_JAVA_LIBS").map(PathBuf::from).unwrap_or_else(|_| server.join("java-layer").join("lib"));
    let mut jars: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e} (the Java layer's libraries go there)", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jar"))
        .collect();
    jars.sort();
    if !jars.iter().any(|j| j.file_name().is_some_and(|n| n.to_string_lossy().starts_with("horizon-javalayer"))) {
        return Err(format!("{} has no horizon-javalayer.jar", dir.display()));
    }
    // The Java server's own classes (lib/server: net.minecraft, CraftBukkit, Paper's internals) come first: Paper's
    // patched Brigadier, DataFixerUpper and authlib must win over the plain libraries. The CraftBukkit classes the
    // layer defines itself are not in that copy (build.py takes them out). Nothing in them loads until used.
    let mut server_jars: Vec<PathBuf> = std::fs::read_dir(dir.join("server"))
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jar")).collect())
        .unwrap_or_default();
    server_jars.sort();
    server_jars.extend(jars);
    let jars = server_jars;
    let separator = if cfg!(windows) { ";" } else { ":" };
    Ok(jars.iter().map(|j| j.display().to_string()).collect::<Vec<_>>().join(separator))
}

type CreateVm = unsafe extern "system" fn(*mut *mut sys::JavaVM, *mut *mut std::ffi::c_void, *mut std::ffi::c_void) -> sys::jint;
type CreatedVms = unsafe extern "system" fn(*mut *mut sys::JavaVM, sys::jsize, *mut sys::jsize) -> sys::jint;

fn load_library(path: &Path) -> Result<libloading::Library, String> {
    #[cfg(windows)]
    {
        // Its own folder first for what it loads (vcruntime, jli): Java's bin isn't on the server's PATH.
        const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;
        unsafe { libloading::os::windows::Library::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH) }.map(Into::into).map_err(|e| format!("{}: {e}", path.display()))
    }
    #[cfg(not(windows))]
    {
        unsafe { libloading::Library::new(path) }.map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The process's Java VM: the one already there (this layer was reloaded; a process can hold only one), else a
/// new one, lean: a small heap that gives memory back, a serial collector, fewer compiler threads.
fn vm(plugin: &Plugin, server: &Path) -> Result<JavaVM, String> {
    let (home, library) = find_java(server)?;
    let lib = load_library(&library)?;
    let created: libloading::Symbol<CreatedVms> = unsafe { lib.get(b"JNI_GetCreatedJavaVMs") }.map_err(|e| e.to_string())?;
    let mut existing: *mut sys::JavaVM = std::ptr::null_mut();
    let mut count: sys::jsize = 0;
    if unsafe { created(&mut existing, 1, &mut count) } == sys::JNI_OK && count > 0 && !existing.is_null() {
        std::mem::forget(lib);
        plugin.info("Java VM already running in this process: reusing it");
        return unsafe { JavaVM::from_raw(existing) }.map_err(|e| e.to_string());
    }
    let create: libloading::Symbol<CreateVm> = unsafe { lib.get(b"JNI_CreateJavaVM") }.map_err(|e| e.to_string())?;
    let (memory, extra) = settings(plugin);
    let mut options = vec![
        format!("-Djava.class.path={}", class_path(server)?),
        format!("-Xmx{memory}"),
        "-Xms8m".to_owned(),
        "-XX:+UseSerialGC".to_owned(),
        // Java 25: 8-byte object headers (vanilla's ~2 million small objects: AABBs, shapes, block states).
        "-XX:+UseCompactObjectHeaders".to_owned(),
        "-XX:MinHeapFreeRatio=10".to_owned(),
        "-XX:MaxHeapFreeRatio=30".to_owned(),
        // A small young generation (default: a third of the heap, ~100 MB committed at once): short-lived
        // objects die young here too, and pauses stay under ~10 ms.
        "-Xmn32m".to_owned(),
        "-XX:ReservedCodeCacheSize=64m".to_owned(),
        "-XX:CICompilerCount=2".to_owned(),
        "-XX:-UsePerfData".to_owned(),
        "-XX:TieredStopAtLevel=4".to_owned(),
        "-Xss1m".to_owned(),
        "-Xrs".to_owned(),
        "-Xshare:auto".to_owned(),
        "-Djava.awt.headless=true".to_owned(),
        "-Dfile.encoding=UTF-8".to_owned(),
        "-Dstdout.encoding=UTF-8".to_owned(),
        "--enable-native-access=ALL-UNNAMED".to_owned(),
        "--sun-misc-unsafe-memory-access=allow".to_owned(),
        format!("-Dhorizon.javalayer.home={}", home.display()),
    ];
    // Java 26 warns of every final field a plugin's library sets by reflection (EssentialsX's Gson) and will refuse
    // it later: allowed for plugins, as their code is in no module. Older Javas don't know the flag (and would not start).
    if java_version(&home).is_some_and(|v| v >= 26) {
        options.push("--enable-final-field-mutation=ALL-UNNAMED".to_owned());
    }
    let cds = server.join("java-layer").join("horizon-javalayer.jsa");
    if cds.is_file() {
        options.push(format!("-XX:SharedArchiveFile={}", cds.display()));
    }
    options.extend(extra);
    let strings: Vec<std::ffi::CString> = options.iter().map(|o| std::ffi::CString::new(o.as_str()).unwrap_or_default()).collect();
    let mut raw: Vec<sys::JavaVMOption> = strings.iter().map(|s| sys::JavaVMOption { optionString: s.as_ptr() as *mut _, extraInfo: std::ptr::null_mut() }).collect();
    let mut args = sys::JavaVMInitArgs { version: sys::JNI_VERSION_1_8, nOptions: raw.len() as sys::jint, options: raw.as_mut_ptr(), ignoreUnrecognized: sys::JNI_TRUE };
    let mut vm: *mut sys::JavaVM = std::ptr::null_mut();
    let mut env: *mut std::ffi::c_void = std::ptr::null_mut();
    let code = unsafe { create(&mut vm, &mut env, (&mut args as *mut sys::JavaVMInitArgs).cast()) };
    std::mem::forget(lib);
    if code != sys::JNI_OK {
        return Err(format!("JNI_CreateJavaVM failed ({code}) with {}", home.display()));
    }
    plugin.info(&format!("Java {} from {} (heap cap {memory})", java_version(&home).unwrap_or(0), home.display()));
    unsafe { JavaVM::from_raw(vm) }.map_err(|e| e.to_string())
}

pub fn start(plugin: &Plugin, plugins: &Path, jars: &[PathBuf]) -> Result<(), String> {
    // The server folder absolute: `plugins` can come relative ("plugins"), whose parent is "", and Java reads
    // new File("", x) as the drive's root (the library cache and config/ landed there that way). The plugins folder
    // itself stays as given: on Paper a plugin's data folder is relative (plugins/X), and plugins build paths on it
    // ("./" + getDataFolder(): FlectonePulse's H2 file).
    let absolute = std::path::absolute(plugins).unwrap_or_else(|_| plugins.to_path_buf());
    let server = absolute.parent().unwrap_or(&absolute).to_path_buf();
    let vm = vm(plugin, &server)?;
    let mut env = vm.attach_current_thread_permanently().map_err(|e| e.to_string())?;
    let native = env.find_class("horizon/javalayer/Native").map_err(|e| describe_and(&mut env, e))?;
    crate::natives::register(&mut env, &native).map_err(|e| describe_and(&mut env, e))?;
    crate::conv::init(&mut env).map_err(|e| describe_and(&mut env, e))?;
    let bridge = env.find_class("horizon/javalayer/Bridge").map_err(|e| describe_and(&mut env, e))?;
    let bridge = env.new_global_ref(bridge).map_err(|e| e.to_string())?;
    drop(env);
    *JAVA.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::sync::Arc::new(Java { vm, bridge, plugin: plugin.clone() }));
    // Bukkit's scheduler beats here: its tasks run on the server thread, one beat a tick.
    horizon_bukkit::scheduler::run_task_timer(1, 1, tick);
    let jar_list = jars.iter().map(|j| j.display().to_string()).collect::<Vec<_>>();
    let started = call_bridge("start", "(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)V", |env| {
        let server = env.new_string(server.display().to_string())?;
        let plugins = env.new_string(plugins.display().to_string())?;
        let list = strings(env, &jar_list)?;
        Ok(vec![JObject::from(server), JObject::from(plugins), JObject::from(list)])
    });
    started.map(|_| ())
}

fn describe_and(env: &mut JNIEnv, error: jni::errors::Error) -> String {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    error.to_string()
}

pub fn strings<'l>(env: &mut JNIEnv<'l>, items: &[String]) -> jni::errors::Result<jni::objects::JObjectArray<'l>> {
    let array = env.new_object_array(items.len() as i32, "java/lang/String", JObject::null())?;
    for (i, item) in items.iter().enumerate() {
        let text = env.new_string(item)?;
        env.set_object_array_element(&array, i as i32, &text)?;
        env.delete_local_ref(text)?;
    }
    Ok(array)
}

/// A static method of Bridge with arguments made in the call's own frame; Java's exception, if any, is logged.
pub fn call_bridge<'a, F>(name: &str, sig: &str, args: F) -> Result<Option<sys::jvalue>, String>
where
    F: for<'l> FnOnce(&mut JNIEnv<'l>) -> jni::errors::Result<Vec<JObject<'l>>>,
{
    let Some(java) = java() else { return Err("Java is not running".to_owned()) };
    let mut env = java.vm.attach_current_thread_permanently().map_err(|e| e.to_string())?;
    let class: &JClass = java.bridge.as_obj().into();
    let out = env.with_local_frame(32, |env| -> jni::errors::Result<Option<sys::jvalue>> {
        let objects = args(env)?;
        let values: Vec<JValue> = objects.iter().map(JValue::Object).collect();
        let result = env.call_static_method(class, name, sig, &values);
        match result {
            Ok(value) => Ok(Some(value.as_jni())),
            Err(error) => Err(error),
        }
    });
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    out.map_err(|e| format!("Bridge.{name}: {e}"))
}

pub fn tick() {
    let _ = call_bridge("tick", "()V", |_| Ok(Vec::new()));
}

pub fn stop() {
    if java().is_some() {
        let _ = call_bridge("stop", "()V", |_| Ok(Vec::new()));
    }
    *JAVA.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
