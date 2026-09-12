use quantam_fs::{
    encoding::{
        decode_control_record, decode_mailbox_frame, decode_pull_request, encode_control_record,
        encode_mailbox_frame, encode_pull_request, MailboxFrame,
    },
    ids::{ChunkId, FileId, PeerId},
    protocol::{manifest::Manifest, pull::PullRequest},
    sync::host::{ControlRecord, ControlUpdate},
    Result,
};

#[test]
fn mailbox_frames_have_exact_typed_binary_layouts() -> Result<()> {
    let control = MailboxFrame::Control {
        ciphertext: vec![0x80, 0xff],
    };
    assert_eq!(encode_mailbox_frame(&control)?, [0, 0, 0, 0, 2, 0x80, 0xff]);
    assert_eq!(
        decode_mailbox_frame(&encode_mailbox_frame(&control)?)?,
        control
    );

    let chunk = MailboxFrame::ChunkBody {
        file_id: FileId([0x11; 32]),
        index: 0x0102_0304_0506_0708,
        ciphertext: vec![0xaa, 0xbb, 0xcc],
    };
    let mut expected = vec![1];
    expected.extend_from_slice(&[0x11; 32]);
    expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    expected.extend_from_slice(&[0, 0, 0, 3, 0xaa, 0xbb, 0xcc]);
    assert_eq!(encode_mailbox_frame(&chunk)?, expected);
    assert_eq!(decode_mailbox_frame(&expected)?, chunk);
    Ok(())
}

#[test]
fn mailbox_decoder_rejects_unknown_truncated_and_trailing_data() -> Result<()> {
    assert!(decode_mailbox_frame(&[2, 0, 0, 0, 0]).is_err());
    assert!(decode_mailbox_frame(&[0, 0, 0, 0, 2, 0xaa]).is_err());

    let mut encoded = encode_mailbox_frame(&MailboxFrame::Control {
        ciphertext: vec![0xaa],
    })?;
    encoded.push(0);
    assert!(decode_mailbox_frame(&encoded).is_err());
    Ok(())
}

#[test]
fn pull_request_codec_is_raw_ids_with_a_bounded_count() -> Result<()> {
    let request = PullRequest::new(vec![ChunkId([0x11; 32]), ChunkId([0x22; 32])])?;
    let mut expected = vec![0, 0, 0, 2];
    expected.extend_from_slice(&[0x11; 32]);
    expected.extend_from_slice(&[0x22; 32]);
    assert_eq!(encode_pull_request(&request)?, expected);
    assert_eq!(decode_pull_request(&expected)?, request);

    let oversized_count = 33u32.to_be_bytes();
    assert!(decode_pull_request(&oversized_count).is_err());
    assert!(decode_pull_request(&[0, 0, 0, 1]).is_err());

    expected.push(0);
    assert!(decode_pull_request(&expected).is_err());
    Ok(())
}

#[test]
fn control_record_uses_canonical_manifest_message_and_detached_signature() -> Result<()> {
    let manifest = Manifest {
        file_id: FileId([0x10; 32]),
        version: 0x0102_0304_0506_0708,
        size: 0x1112_1314_1516_1718,
        writer_id: PeerId([0x20; 32]),
        chunk_ids: vec![ChunkId([0x30; 32])],
        signature: vec![0x40, 0x41],
    };
    let record = ControlRecord {
        id: 0x2122_2324_2526_2728,
        update: ControlUpdate::NewManifest(manifest.clone()),
    };

    let mut message = vec![0x10; 32];
    message.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    message.extend_from_slice(&[0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]);
    message.extend_from_slice(&[0x20; 32]);
    message.extend_from_slice(&[0, 0, 0, 1]);
    message.extend_from_slice(&[0x30; 32]);

    let mut expected = vec![0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0];
    expected.extend_from_slice(&(message.len() as u32).to_be_bytes());
    expected.extend_from_slice(&message);
    expected.extend_from_slice(&[0, 0, 0, 2, 0x40, 0x41]);
    assert_eq!(encode_control_record(&record)?, expected);

    let decoded = decode_control_record(&expected)?;
    assert_eq!(decoded.id, record.id);
    match decoded.update {
        ControlUpdate::NewManifest(decoded_manifest) => assert_eq!(decoded_manifest, manifest),
        _ => panic!("expected manifest control record"),
    }
    Ok(())
}

#[test]
fn file_control_records_round_trip_without_coalescing_semantics() -> Result<()> {
    let variants = [
        (1, ControlUpdate::Add(FileId([1; 32]))),
        (2, ControlUpdate::Clear(FileId([2; 32]))),
        (3, ControlUpdate::Remove(FileId([3; 32]))),
    ];
    for (kind, update) in variants {
        let record = ControlRecord { id: 9, update };
        let encoded = encode_control_record(&record)?;
        assert_eq!(encoded[8], kind);
        let decoded = decode_control_record(&encoded)?;
        assert_eq!(decoded.id, 9);
        match (record.update, decoded.update) {
            (ControlUpdate::Add(left), ControlUpdate::Add(right))
            | (ControlUpdate::Clear(left), ControlUpdate::Clear(right))
            | (ControlUpdate::Remove(left), ControlUpdate::Remove(right)) => {
                assert_eq!(left, right);
            }
            _ => panic!("control record changed kind"),
        }
    }
    Ok(())
}

#[test]
fn control_decoder_rejects_unknown_truncated_invalid_count_and_trailing_data() -> Result<()> {
    let mut unknown = vec![0; 8];
    unknown.push(4);
    assert!(decode_control_record(&unknown).is_err());

    let mut truncated = vec![0; 8];
    truncated.extend_from_slice(&[1, 0xaa]);
    assert!(decode_control_record(&truncated).is_err());

    let mut invalid_manifest = vec![0; 8];
    invalid_manifest.push(0);
    let mut message = vec![0; 32 + 8 + 8 + 32];
    message.extend_from_slice(&u32::MAX.to_be_bytes());
    invalid_manifest.extend_from_slice(&(message.len() as u32).to_be_bytes());
    invalid_manifest.extend_from_slice(&message);
    invalid_manifest.extend_from_slice(&0u32.to_be_bytes());
    assert!(decode_control_record(&invalid_manifest).is_err());

    let record = ControlRecord {
        id: 1,
        update: ControlUpdate::Remove(FileId([5; 32])),
    };
    let mut trailing = encode_control_record(&record)?;
    trailing.push(0);
    assert!(decode_control_record(&trailing).is_err());
    Ok(())
}
