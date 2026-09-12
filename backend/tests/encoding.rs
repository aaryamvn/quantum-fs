use quantam_fs::{
    crypto::{
        identity::IdentityDocument,
        sign::{FLUSH_CONTEXT, IDENTITY_CONTEXT, MANIFEST_CONTEXT, WRAP_CONTEXT},
        wrap::WrapMessage,
    },
    encoding,
    ids::{ChunkId, Epoch, FileId, PeerId, Seq},
    protocol::{
        manifest::Manifest,
        packet::{PacketHeader, PayloadType, PROTOCOL_VERSION},
    },
    sync::host::FlushChallenge,
    Result,
};

fn packet(sender: u8, receiver: u8) -> PacketHeader {
    PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: PeerId([sender; 32]),
        receiver_id: PeerId([receiver; 32]),
        epoch: Epoch(0x0102_0304_0506_0708),
        seq: Seq(0x1020_3040_5060_7080),
    }
}

#[test]
fn hashes_binary_identity_and_plaintext_chunk_inputs() {
    assert_eq!(
        encoding::peer_id(&[0x00, 0x80, 0xff, b':', b'/']).0,
        [
            0x7b, 0x47, 0x6b, 0x63, 0x6a, 0x6d, 0xcb, 0xb1, 0x47, 0x3e, 0xf5, 0xd6, 0x86, 0xca,
            0xae, 0x3b, 0x7e, 0xe8, 0x82, 0x8f, 0x29, 0xd1, 0x41, 0x1e, 0x16, 0x3d, 0xcf, 0x43,
            0x62, 0xce, 0x3e, 0x8b,
        ]
    );
    assert_eq!(
        encoding::chunk_id(
            &FileId([0x80; 32]),
            0x0102_0304_0506_0708,
            &[0x00, 0xff, 0x80, b'/', b':'],
        )
        .0,
        [
            0x4a, 0x86, 0xe3, 0xd1, 0xa0, 0xc2, 0xea, 0x60, 0xeb, 0x2a, 0x9c, 0x92, 0x9e, 0x09,
            0x77, 0x3c, 0x6f, 0xf8, 0xee, 0x99, 0x1b, 0x48, 0x18, 0x94, 0xc5, 0x43, 0xda, 0xb8,
            0x45, 0x80, 0xda, 0x1c,
        ]
    );
}

#[test]
fn wrap_hkdf_info_is_binary_and_requires_canonical_pair_order() -> Result<()> {
    let min = PeerId([0x00; 32]);
    let max = PeerId([0xff; 32]);
    let mut expected = b"qfs/v1/wrap/pair/".to_vec();
    expected.extend_from_slice(&[0x00; 32]);
    expected.push(b':');
    expected.extend_from_slice(&[0xff; 32]);
    expected.push(b'/');
    expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(
        encoding::wrap_hkdf_info(&min, &max, Epoch(0x0102_0304_0506_0708))?,
        expected
    );
    assert!(encoding::wrap_hkdf_info(&max, &min, Epoch(1)).is_err());
    assert!(encoding::wrap_hkdf_info(&min, &min, Epoch(1)).is_err());
    Ok(())
}

#[test]
fn packet_and_chunk_aad_have_one_fixed_width_layout() {
    let header = packet(0x80, 0xff);
    let mut packet_expected = vec![1];
    packet_expected.extend_from_slice(&[0x80; 32]);
    packet_expected.extend_from_slice(&[0xff; 32]);
    packet_expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    packet_expected.extend_from_slice(&[0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80]);
    assert_eq!(encoding::packet_aad(&header), packet_expected);

    let mut chunk_expected = packet_expected;
    chunk_expected.extend_from_slice(&[0x00; 32]);
    chunk_expected.extend_from_slice(&[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]);
    assert_eq!(
        encoding::chunk_aad(&header, &FileId([0x00; 32]), 0x8877_6655_4433_2211),
        chunk_expected
    );
}

#[test]
fn wrap_aad_is_fixed_width_and_canonically_ordered() -> Result<()> {
    let min = PeerId([0x00; 32]);
    let max = PeerId([0x80; 32]);
    let mut expected = vec![0x00; 32];
    expected.extend_from_slice(&[0x80; 32]);
    expected.extend_from_slice(&[0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10]);
    assert_eq!(
        encoding::wrap_aad(&min, &max, Epoch(0xfedc_ba98_7654_3210))?,
        expected
    );
    assert!(encoding::wrap_aad(&max, &min, Epoch(0)).is_err());
    Ok(())
}

#[test]
fn identity_message_length_prefixes_binary_keys_and_excludes_signature() -> Result<()> {
    let mut document = IdentityDocument {
        peer_id: PeerId([0x80; 32]),
        ek: vec![0x00, 0xff, 0x80],
        vk: vec![b':', b'/'],
        created_at: 0x0102_0304_0506_0708,
        signature: vec![0xaa],
    };
    let mut expected = vec![0x80; 32];
    expected.extend_from_slice(&[0, 0, 0, 3, 0x00, 0xff, 0x80]);
    expected.extend_from_slice(&[0, 0, 0, 2, b':', b'/']);
    expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(encoding::identity_m(&document)?, expected);
    document.signature = vec![0xbb, 0xcc];
    assert_eq!(encoding::identity_m(&document)?, expected);
    Ok(())
}

#[test]
fn wrap_message_length_prefixes_ciphertexts_and_excludes_signature() -> Result<()> {
    let mut message = WrapMessage {
        kem_ct: vec![0x00, 0x80],
        wrap_ct: vec![0xff, b':', b'/'],
        epoch: Epoch(0x0102_0304_0506_0708),
        min_id: PeerId([0x00; 32]),
        max_id: PeerId([0xff; 32]),
        signature: vec![1],
    };
    let mut expected = vec![0, 0, 0, 2, 0x00, 0x80, 0, 0, 0, 3, 0xff, b':', b'/'];
    expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    expected.extend_from_slice(&[0x00; 32]);
    expected.extend_from_slice(&[0xff; 32]);
    assert_eq!(encoding::wrap_m(&message)?, expected);
    message.signature = vec![2, 3];
    assert_eq!(encoding::wrap_m(&message)?, expected);
    Ok(())
}

#[test]
fn manifest_message_preserves_chunk_order_and_excludes_signature() -> Result<()> {
    let mut manifest = Manifest {
        file_id: FileId([0x80; 32]),
        chunk_ids: vec![ChunkId([0x00; 32]), ChunkId([0xff; 32])],
        size: 0x1020_3040_5060_7080,
        writer_id: PeerId([0x7f; 32]),
        version: 0x0102_0304_0506_0708,
        signature: vec![0xaa],
    };
    let mut expected = vec![0x80; 32];
    expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    expected.extend_from_slice(&[0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80]);
    expected.extend_from_slice(&[0x7f; 32]);
    expected.extend_from_slice(&[0, 0, 0, 2]);
    expected.extend_from_slice(&[0x00; 32]);
    expected.extend_from_slice(&[0xff; 32]);
    assert_eq!(encoding::manifest_m(&manifest)?, expected);
    manifest.signature = vec![0xbb, 0xcc];
    assert_eq!(encoding::manifest_m(&manifest)?, expected);
    Ok(())
}

#[test]
fn flush_message_is_exact_challenge_and_wrap_nonce_is_zero() {
    let challenge = FlushChallenge([0x80; 32]);
    assert_eq!(encoding::flush_m(&challenge), [0x80; 32]);
    assert_eq!(encoding::WRAP_GCM_NONCE, [0; 12]);
}

#[test]
fn nonce_reserves_top_bits_for_type_and_unsigned_direction() {
    let low_to_high = packet(0x00, 0x80);
    let high_to_low = packet(0xff, 0x80);
    let counter = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80];

    let mut expected = [0u8; 12];
    expected[4..].copy_from_slice(&counter);
    assert_eq!(encoding::nonce(&low_to_high, PayloadType::Packet), expected);
    expected[0] = 0x80;
    assert_eq!(
        encoding::nonce(&low_to_high, PayloadType::ChunkBody),
        expected
    );
    expected[0] = 0x40;
    assert_eq!(encoding::nonce(&high_to_low, PayloadType::Packet), expected);
    expected[0] = 0xc0;
    assert_eq!(
        encoding::nonce(&high_to_low, PayloadType::ChunkBody),
        expected
    );
}

#[test]
fn fips_204_contexts_are_the_only_v1_signing_domains() {
    assert_eq!(IDENTITY_CONTEXT, b"qfs/v1/id");
    assert_eq!(WRAP_CONTEXT, b"qfs/v1/wrap");
    assert_eq!(MANIFEST_CONTEXT, b"qfs/v1/manifest");
    assert_eq!(FLUSH_CONTEXT, b"qfs/v1/flush");
}
