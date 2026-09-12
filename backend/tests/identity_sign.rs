use quantam_fs::{
    crypto::{
        identity::{IdentityDocument, LocalIdentity},
        sign::{PureMlDsa, RustCryptoPureMlDsa, FLUSH_CONTEXT, MANIFEST_CONTEXT},
    },
    encoding,
    ids::{ChunkId, FileId, PeerId},
    protocol::manifest::Manifest,
    sync::host::FlushChallenge,
};

#[test]
fn identity_rejects_swapped_ek_and_wrong_peer_id() {
    let alice = LocalIdentity::generate().unwrap();
    let bob = LocalIdentity::generate().unwrap();
    alice.document.verify().unwrap();

    let mut swapped_ek = alice.document.clone();
    swapped_ek.ek = bob.document.ek.clone();
    assert!(swapped_ek.verify().is_err());

    let mut wrong_id = alice.document.clone();
    wrong_id.peer_id = bob.document.peer_id;
    assert!(wrong_id.verify().is_err());
}

#[test]
fn rotating_ek_preserves_the_principal_and_verifying_key() {
    let original = LocalIdentity::generate().unwrap();
    let rotated = original.rotate_ek().unwrap();

    rotated.document.verify().unwrap();
    assert_eq!(original.document.peer_id, rotated.document.peer_id);
    assert_eq!(original.document.vk, rotated.document.vk);
    assert_ne!(original.document.ek, rotated.document.ek);
}

#[test]
fn independently_generated_identities_are_distinct_principals() {
    let first = LocalIdentity::generate().unwrap();
    let second = LocalIdentity::generate().unwrap();

    assert_ne!(first.document.vk, second.document.vk);
    assert_ne!(first.document.peer_id, second.document.peer_id);
    assert_ne!(first.document.ek, second.document.ek);
}

#[test]
fn manifest_and_flush_use_their_canonical_messages_and_contexts() {
    let identity = LocalIdentity::generate().unwrap();
    let provider = RustCryptoPureMlDsa;
    let manifest = Manifest {
        file_id: FileId([1; 32]),
        chunk_ids: vec![ChunkId([2; 32]), ChunkId([3; 32])],
        size: 42,
        writer_id: identity.document.peer_id,
        version: 7,
        signature: Vec::new(),
    };
    let manifest_m = encoding::manifest_m(&manifest).unwrap();
    let manifest_signature = provider
        .sign(&identity.signing_key, MANIFEST_CONTEXT, &manifest_m)
        .unwrap();
    provider
        .verify(
            &identity.document.vk,
            MANIFEST_CONTEXT,
            &manifest_m,
            &manifest_signature,
        )
        .unwrap();
    assert!(provider
        .verify(
            &identity.document.vk,
            FLUSH_CONTEXT,
            &manifest_m,
            &manifest_signature,
        )
        .is_err());

    let challenge = FlushChallenge([9; 32]);
    let flush_m = encoding::flush_m(&challenge);
    let flush_signature = provider
        .sign(&identity.signing_key, FLUSH_CONTEXT, &flush_m)
        .unwrap();
    provider
        .verify(
            &identity.document.vk,
            FLUSH_CONTEXT,
            &flush_m,
            &flush_signature,
        )
        .unwrap();

    assert!(provider
        .sign(&identity.signing_key, b"qfs/v1/pkt", b"packet")
        .is_err());
}

#[test]
fn malformed_ml_dsa_inputs_fail_closed() {
    let identity = LocalIdentity::generate().unwrap();
    let provider = RustCryptoPureMlDsa;
    let signature = provider
        .sign(&identity.signing_key, MANIFEST_CONTEXT, b"manifest")
        .unwrap();

    assert!(provider
        .verify(&[0; 31], MANIFEST_CONTEXT, b"manifest", &signature)
        .is_err());
    assert!(provider
        .verify(
            &identity.document.vk,
            MANIFEST_CONTEXT,
            b"manifest",
            &signature[..signature.len() - 1],
        )
        .is_err());
}

#[test]
fn invalid_xwing_public_key_fails_identity_validation() {
    let identity = LocalIdentity::generate().unwrap();
    let document = IdentityDocument {
        ek: vec![0; 8],
        peer_id: PeerId(identity.document.peer_id.0),
        ..identity.document
    };
    assert!(document.verify().is_err());
}
