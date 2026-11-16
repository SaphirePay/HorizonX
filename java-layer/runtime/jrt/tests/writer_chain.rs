use jrt::*;
#[test]
fn buffered_writer_over_a_file_stream_writes_on_close() {
    let path = std::env::temp_dir().join("jrt_writer_chain.yml");
    let _ = std::fs::remove_file(&path);
    let file = new_other("File", &[s(path.to_str().unwrap())]).ok().unwrap();
    let out = new_other("FileOutputStream", &[file, V::Bool(false)]).ok().unwrap();
    let utf8 = other_field("StandardCharsets", "UTF_8");
    let osw = new_other("OutputStreamWriter", &[out, utf8]).ok().unwrap();
    let w = new_other("BufferedWriter", &[osw]).ok().unwrap();
    call(&w, "write", &[s("a: 1\n")]).ok().unwrap();
    call(&w, "close", &[]).ok().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a: 1\n");
}

#[test]
fn a_string_writer_writes_pieces_of_text() {
    let w = new_other("StringWriter", &[]).ok().unwrap();
    call(&w, "write", &[s("héllo world"), V::Int(1), V::Int(4)]).ok().unwrap();
    call(&w, "append", &[s("abcdef"), V::Int(2), V::Int(4)]).ok().unwrap();
    assert_eq!(call(&w, "toString", &[]).ok().unwrap().text(), "éllocd");
}
