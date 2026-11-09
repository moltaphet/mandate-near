use borsh::BorshDeserialize;
use mandate_withdrawal_journal::{
    Hex32, JournalProtocolDescriptor as Descriptor, JournalWithdrawalRecord as Record,
    JournalWithdrawalRequest as Request, JournalWithdrawalState as State,
    MAX_JOURNAL_RESPONSE_BYTES, PROTOCOL_VERSION, ProtocolError, REQUEST_HASH_SCHEME,
    canonical_hash,
};
use near_sdk::{
    AccountId,
    json_types::{U64, U128},
    serde_json::{self, json},
};

fn account(value: &str) -> AccountId {
    value.parse().unwrap()
}

fn descriptor() -> Descriptor {
    Descriptor {
        protocol_version: PROTOCOL_VERSION.into(),
        ledger_domain: Hex32::from_bytes([0x11; 32]),
        executor_id: account("intents.sandbox"),
        request_hash_scheme: REQUEST_HASH_SCHEME.into(),
    }
}

fn request() -> Request {
    Request {
        withdrawal_id: U64(7),
        request_seq: U64(3),
        token: account("usdc.sandbox"),
        receiver_id: account("receiver.sandbox"),
        amount: U128(1_000_000),
    }
}

fn vault() -> AccountId {
    account("vault.sandbox")
}

fn digest_text(value: Hex32) -> String {
    value.bytes().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn commitment_matches_independent_manual_borsh_sha256_vector() {
    // Generated independently using Python struct/to_bytes/hashlib; see generate_golden.py.
    assert_eq!(
        digest_text(canonical_hash(&descriptor(), &vault(), &request())),
        "bec91f82f8e4c69f613a54cffab6fc584545e7e726102dd4b0ad679115c3e25e"
    );
}

#[test]
fn commitment_matches_independent_maximum_integer_vector() {
    let desc = Descriptor {
        protocol_version: PROTOCOL_VERSION.into(),
        ledger_domain: Hex32::from_bytes(std::array::from_fn(|i| i as u8)),
        executor_id: account("intents.testnet"),
        request_hash_scheme: REQUEST_HASH_SCHEME.into(),
    };
    let req = Request {
        withdrawal_id: U64(u64::MAX),
        request_seq: U64(u64::MAX),
        token: account("token.testnet"),
        receiver_id: account("alice.testnet"),
        amount: U128(u128::MAX),
    };
    assert_eq!(
        digest_text(canonical_hash(&desc, &account("vault.testnet"), &req)),
        "84fcb6eff15010093ea81bc75cf3ad9ebfb2dda265e73f1e4c5e946a283cc56e"
    );
}

#[test]
fn hex32_json_is_lowercase_and_borsh_is_exactly_the_fixed_array() {
    let bytes: [u8; 32] = std::array::from_fn(|i| (i * 7) as u8);
    let value = Hex32::from_bytes(bytes);
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, format!("\"{}\"", digest_text(value)));
    assert_eq!(serde_json::from_str::<Hex32>(&json).unwrap().bytes(), bytes);
    let encoded = borsh::to_vec(&value).unwrap();
    assert_eq!(encoded, bytes);
    assert_eq!(Hex32::try_from_slice(&encoded).unwrap(), value);
    assert!(Hex32::try_from_slice(&[0; 31]).is_err());
    assert!(Hex32::try_from_slice(&[0; 33]).is_err());
}

#[test]
fn malformed_and_noncanonical_hex_json_is_refused() {
    for value in [
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        format!("0x{}", "a".repeat(62)),
        format!("{} ", "a".repeat(63)),
        "é".repeat(32),
    ] {
        assert!(serde_json::from_value::<Hex32>(json!(value)).is_err());
    }
    for value in [json!(null), json!(32), json!(vec![0u8; 32]), json!({"bytes": "a".repeat(64)})] {
        assert!(serde_json::from_value::<Hex32>(value).is_err());
    }
}

#[test]
fn request_json_order_does_not_change_the_commitment() {
    let left: Request = serde_json::from_str(
        r#"{"withdrawal_id":"7","request_seq":"3","token":"usdc.sandbox","receiver_id":"receiver.sandbox","amount":"1000000"}"#,
    ).unwrap();
    let right: Request = serde_json::from_str(
        r#"{"amount":"1000000","receiver_id":"receiver.sandbox","token":"usdc.sandbox","request_seq":"3","withdrawal_id":"7"}"#,
    ).unwrap();
    assert_eq!(left, right);
    assert_eq!(
        canonical_hash(&descriptor(), &vault(), &left),
        canonical_hash(&descriptor(), &vault(), &right)
    );
}

#[test]
fn every_committed_binding_changes_the_hash() {
    let desc = descriptor();
    let req = request();
    let original = canonical_hash(&desc, &vault(), &req);
    let mut altered_descs = Vec::new();
    let mut changed = desc.clone();
    changed.protocol_version.push('2');
    altered_descs.push(changed);
    let mut changed = desc.clone();
    changed.ledger_domain = Hex32::from_bytes([0x12; 32]);
    altered_descs.push(changed);
    let mut changed = desc.clone();
    changed.executor_id = account("other.sandbox");
    altered_descs.push(changed);
    for changed in altered_descs {
        assert_ne!(canonical_hash(&changed, &vault(), &req), original);
    }
    assert_ne!(canonical_hash(&desc, &account("other-vault.sandbox"), &req), original);
    let mut altered_requests = Vec::new();
    let mut changed = req.clone();
    changed.withdrawal_id = U64(8);
    altered_requests.push(changed);
    let mut changed = req.clone();
    changed.request_seq = U64(4);
    altered_requests.push(changed);
    let mut changed = req.clone();
    changed.token = account("other-token.sandbox");
    altered_requests.push(changed);
    let mut changed = req.clone();
    changed.receiver_id = account("other-receiver.sandbox");
    altered_requests.push(changed);
    let mut changed = req.clone();
    changed.amount = U128(1_000_001);
    altered_requests.push(changed);
    for changed in altered_requests {
        assert_ne!(canonical_hash(&desc, &vault(), &changed), original);
    }
}

#[test]
fn requests_refuse_zero_identifiers_and_amount() {
    let mut req = request();
    req.withdrawal_id = U64(0);
    assert!(req.validate().is_err());
    let mut req = request();
    req.request_seq = U64(0);
    assert!(req.validate().is_err());
    let mut req = request();
    req.amount = U128(0);
    assert!(req.validate().is_err());
    assert!(request().validate().is_ok());
}

#[test]
fn descriptor_checks_protocol_scheme_domain_and_selected_executor() {
    let desc = descriptor();
    assert!(desc.validate(&desc.executor_id).is_ok());
    assert!(desc.validate(&account("other.sandbox")).is_err());
    let mut changed = desc.clone();
    changed.protocol_version = "untrusted-version".into();
    assert!(changed.validate(&desc.executor_id).is_err());
    let mut changed = desc.clone();
    changed.request_hash_scheme = "untrusted-scheme".into();
    assert!(changed.validate(&desc.executor_id).is_err());
    let mut changed = desc.clone();
    changed.ledger_domain = Hex32::from_bytes([0; 32]);
    assert!(changed.validate(&desc.executor_id).is_err());
}

#[test]
fn unknown_fields_and_states_are_refused_at_every_json_boundary() {
    let mut req = serde_json::to_value(request()).unwrap();
    req["memo"] = json!("not supported");
    assert!(serde_json::from_value::<Request>(req).is_err());
    let mut desc = serde_json::to_value(descriptor()).unwrap();
    desc["capability"] = json!(true);
    assert!(serde_json::from_value::<Descriptor>(desc).is_err());
    let record = Record::new(&descriptor(), vault(), request(), State::Pending).unwrap();
    let mut value = serde_json::to_value(&record).unwrap();
    value["trusted"] = json!(true);
    assert!(serde_json::from_value::<Record>(value).is_err());
    let mut value = serde_json::to_value(&record).unwrap();
    value["request"]["memo"] = json!("bad");
    assert!(serde_json::from_value::<Record>(value).is_err());
    for state in [
        json!({"Delivered": {"delivered_amount":"1000000", "proof":true}}),
        json!({"Refunded": {"refunded_amount":"1000000", "proof":true}}),
        json!({"Partial": {"amount":"1"}}),
        json!({"Pending": {"extra":true}}),
        json!({"Cancelled": {"extra":true}}),
    ] {
        assert!(serde_json::from_value::<State>(state).is_err());
    }
}

#[test]
fn terminal_amount_must_equal_the_positive_request() {
    for state in [
        State::Pending,
        State::Cancelled,
        State::Delivered { delivered_amount: U128(1_000_000) },
        State::Refunded { refunded_amount: U128(1_000_000) },
    ] {
        let record = Record::new(&descriptor(), vault(), request(), state).unwrap();
        assert!(record.validate(&descriptor(), &vault(), &request()).is_ok());
    }
    for amount in [0, 1, 999_999, 1_000_001, u128::MAX] {
        for state in [
            State::Delivered { delivered_amount: U128(amount) },
            State::Refunded { refunded_amount: U128(amount) },
        ] {
            assert!(Record::new(&descriptor(), vault(), request(), state).is_err());
        }
    }
}

#[test]
fn terminal_records_cannot_be_replayed_for_changed_requests_or_domains() {
    let desc = descriptor();
    let req = request();
    let record =
        Record::new(&desc, vault(), req.clone(), State::Delivered { delivered_amount: req.amount })
            .unwrap();
    assert!(record.matches_request(&desc, &vault(), &req));
    let mut changed = req.clone();
    changed.withdrawal_id = U64(8);
    assert!(!record.matches_request(&desc, &vault(), &changed));
    let mut changed = req.clone();
    changed.request_seq = U64(4);
    assert!(record.validate(&desc, &vault(), &changed).is_err());
    assert!(record.validate(&desc, &account("other-vault.sandbox"), &req).is_err());
    let mut changed = desc.clone();
    changed.ledger_domain = Hex32::from_bytes([0x12; 32]);
    assert!(record.validate(&changed, &vault(), &req).is_err());
    let mut changed = desc.clone();
    changed.executor_id = account("other.sandbox");
    assert!(record.validate(&changed, &vault(), &req).is_err());
}

#[test]
fn record_validation_checks_every_stored_binding_and_hash() {
    let desc = descriptor();
    let req = request();
    let record = Record::new(&desc, vault(), req.clone(), State::Pending).unwrap();
    let mut altered = Vec::new();
    let mut r = record.clone();
    r.protocol_version = "wrong".into();
    altered.push(r);
    let mut r = record.clone();
    r.ledger_domain = Hex32::from_bytes([0x12; 32]);
    altered.push(r);
    let mut r = record.clone();
    r.executor_id = account("other.sandbox");
    altered.push(r);
    let mut r = record.clone();
    r.vault_id = account("other-vault.sandbox");
    altered.push(r);
    let mut r = record.clone();
    r.request.withdrawal_id = U64(8);
    altered.push(r);
    let mut r = record.clone();
    r.request.request_seq = U64(4);
    altered.push(r);
    let mut r = record.clone();
    r.request.token = account("other-token.sandbox");
    altered.push(r);
    let mut r = record.clone();
    r.request.receiver_id = account("other-receiver.sandbox");
    altered.push(r);
    let mut r = record.clone();
    r.request.amount = U128(1_000_001);
    altered.push(r);
    let mut r = record.clone();
    r.request_hash = Hex32::from_bytes([0; 32]);
    altered.push(r);
    let mut r = record.clone();
    r.state = State::Delivered { delivered_amount: U128(1) };
    altered.push(r);
    for r in altered {
        assert!(r.validate(&desc, &vault(), &req).is_err());
        assert!(!r.matches_request(&desc, &vault(), &req));
    }
}

#[test]
fn constructor_refuses_invalid_descriptor_or_request() {
    let mut desc = descriptor();
    desc.ledger_domain = Hex32::from_bytes([0; 32]);
    assert!(Record::new(&desc, vault(), request(), State::Pending).is_err());
    let mut req = request();
    req.withdrawal_id = U64(0);
    assert!(Record::new(&descriptor(), vault(), req, State::Pending).is_err());
}

#[test]
fn validated_record_survives_json_and_borsh_roundtrips() {
    let record = Record::new(
        &descriptor(),
        vault(),
        request(),
        State::Refunded { refunded_amount: request().amount },
    )
    .unwrap();
    let json = serde_json::to_vec(&record).unwrap();
    let decoded: Record = serde_json::from_slice(&json).unwrap();
    assert_eq!(decoded, record);
    assert!(decoded.validate(&descriptor(), &vault(), &request()).is_ok());
    let encoded = borsh::to_vec(&record).unwrap();
    let decoded = Record::try_from_slice(&encoded).unwrap();
    assert_eq!(decoded, record);
}

#[test]
fn maximum_supported_tuple_fits_the_response_limit() {
    let max_account = account(&format!("{}.testnet", "a".repeat(56)));
    let desc = Descriptor { executor_id: max_account.clone(), ..descriptor() };
    let req = Request {
        withdrawal_id: U64(u64::MAX),
        request_seq: U64(u64::MAX),
        token: max_account.clone(),
        receiver_id: max_account.clone(),
        amount: U128(u128::MAX),
    };
    let record = Record::new(
        &desc,
        max_account,
        req,
        State::Delivered { delivered_amount: U128(u128::MAX) },
    )
    .unwrap();
    assert!(serde_json::to_vec(&record).unwrap().len() < MAX_JOURNAL_RESPONSE_BYTES);
}

#[test]
fn protocol_error_text_is_fixed_and_contains_no_untrusted_fields() {
    let mut desc = descriptor();
    desc.protocol_version = "sensitive attacker text".into();
    let error: ProtocolError = desc.validate(&desc.executor_id).unwrap_err();
    assert!(!error.to_string().contains("sensitive"));
    assert!(error.to_string().len() <= 64);
}
