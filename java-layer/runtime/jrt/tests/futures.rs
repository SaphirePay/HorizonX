use jrt::*;
#[test]
fn a_failure_after_a_stage_arrives_wrapped_as_in_java() {
    let failed = new_other("CompletableFuture", &[]).ok().unwrap();
    call(&failed, "completeExceptionally", &[exc("IllegalStateException", "Tebex not setup!")]).ok().unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (a, b) = (seen.clone(), seen.clone());
    let direct = func(move |x| { a.lock().unwrap().push(call(&x[0], "getMessage", &[]).ok().unwrap().text()); Ok(V::Null) });
    let after = func(move |x| { b.lock().unwrap().push(call(&x[0], "getMessage", &[]).ok().unwrap().text()); Ok(V::Null) });
    call(&failed, "exceptionally", &[direct]).ok().unwrap();
    let stage = call(&failed, "thenAccept", &[func(|_| Ok(V::Null))]).ok().unwrap();
    call(&stage, "exceptionally", &[after]).ok().unwrap();
    assert_eq!(*seen.lock().unwrap(), ["Tebex not setup!", "java.lang.IllegalStateException: Tebex not setup!"]);
}
