//! What the derive serializes, by the names a bindgen's type carries.

use waos_wit_serialize::WitSerialize;

#[derive(WitSerialize)]
struct OutputChunk {
    #[component(name = "dropped-before")]
    dropped_before: u32,
    #[component(name = "channel")]
    channel: OutputStream,
    #[component(name = "use")]
    use_: Use,
    #[component(name = "pasted")]
    pasted: Option<bool>,
}

#[derive(WitSerialize)]
enum OutputStream {
    #[component(name = "stdout")]
    Stdout,
}

#[derive(WitSerialize)]
enum Use {
    #[component(name = "opened-for-reading")]
    OpenedForReading(String),
}

#[test]
fn a_value_serializes_by_its_wit_names() {
    let chunk = OutputChunk {
        dropped_before: 2,
        channel: OutputStream::Stdout,
        use_: Use::OpenedForReading("/data/notes".to_owned()),
        pasted: None,
    };
    assert_eq!(
        serde_json::to_string(&chunk).unwrap(),
        r#"{"dropped-before":2,"channel":"stdout","use":{"opened-for-reading":"/data/notes"},"pasted":null}"#
    );
}
