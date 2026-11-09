#[path = "common/fixture.rs"]
mod fixture;

const REVIEWED: &[u8] = include_bytes!("fixtures/intents-near-mainnet.wasm");

#[test]
fn accepts_the_exact_reviewed_verifier() {
    fixture::validate_intents_fixture(REVIEWED).unwrap();
}

#[test]
fn refuses_another_valid_wasm_with_the_same_size() {
    // A valid empty WASM module followed by a custom section. The custom
    // payload has an empty name and enough padding to match the reviewed size.
    let mut replacement = b"\0asm\x01\0\0\0\0".to_vec();
    let mut payload_len = fixture::INTENTS_FIXTURE_SIZE - 12;
    loop {
        let byte = (payload_len & 0x7f) as u8;
        payload_len >>= 7;
        replacement.push(byte | if payload_len == 0 { 0 } else { 0x80 });
        if payload_len == 0 {
            break;
        }
    }
    replacement.resize(fixture::INTENTS_FIXTURE_SIZE, 0);
    assert!(replacement.starts_with(b"\0asm"));
    assert_eq!(replacement.len(), fixture::INTENTS_FIXTURE_SIZE);
    assert_eq!(
        fixture::validate_intents_fixture(&replacement),
        Err("Intents verifier fixture SHA256 does not match the reviewed fixture")
    );
}

#[test]
fn refuses_a_different_sized_wasm() {
    assert_eq!(
        fixture::validate_intents_fixture(b"\0asm\x01\0\0\0"),
        Err("Intents verifier fixture size does not match the reviewed fixture")
    );
}
