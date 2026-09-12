use std::{
    collections::BTreeSet,
    net::{Ipv6Addr, SocketAddr, SocketAddrV6},
};

use quantam_fs::{
    crypto::{identity::IdentityDocument, wrap::WrapMessage},
    encoding,
    ids::{Epoch, FileId, PeerId, Seq},
    net::{
        directory::{DirForget, DirectoryAd},
        frame::{
            DIR_AD_KIND, DIR_FORGET_KIND, DIR_LOOKUP_KIND, DIR_PUT_KIND, EPOCH_HINT_KIND,
            FLUSH_CHALLENGE_KIND, FLUSH_SIGNATURE_KIND, GCM_CHUNK_KIND, GCM_PACKET_KIND,
            HEARTBEAT_KIND, IDENTITY_KIND, JOIN_KIND, WRAP_ACK_KIND, WRAP_KIND,
        },
        join::{FlushOffer, JoinRequest, NetControl, NetWelcome, VaultMetadata},
        JoinCode, VaultId,
    },
    protocol::{
        packet::{ControlPacket, PacketHeader, PROTOCOL_VERSION},
        pull::ChunkBodyFrame,
    },
    sync::host::{FlushChallenge, MailboxEnvelope},
    Result,
};

fn document(marker: u8) -> IdentityDocument {
    IdentityDocument {
        peer_id: PeerId([marker; 32]),
        ek: vec![marker, 0x80],
        vk: vec![marker, 0xff, 0],
        created_at: 7,
        signature: vec![marker, 9],
    }
}

fn header() -> PacketHeader {
    PacketHeader {
        version: PROTOCOL_VERSION,
        sender_id: PeerId([1; 32]),
        receiver_id: PeerId([2; 32]),
        epoch: Epoch(3),
        seq: Seq(4),
    }
}

#[test]
fn identity_wrap_packet_chunk_and_epoch_codecs_round_trip() -> Result<()> {
    let identity = document(1);
    assert!(encoding::decode_identity(&encoding::encode_identity(&identity)?)? == identity);

    let wrap = WrapMessage {
        kem_ct: vec![1, 2],
        wrap_ct: vec![3, 4],
        epoch: Epoch(5),
        min_id: PeerId([1; 32]),
        max_id: PeerId([2; 32]),
        signature: vec![6],
    };
    assert!(encoding::decode_wrap(&encoding::encode_wrap(&wrap)?)? == wrap);

    let packet = ControlPacket {
        header: header(),
        ciphertext: vec![0x80, 0xff],
    };
    assert_eq!(
        encoding::decode_control_packet(&encoding::encode_control_packet(&packet)?)?,
        packet
    );

    let chunk = ChunkBodyFrame {
        header: header(),
        file_id: FileId([3; 32]),
        index: 6,
        ciphertext: vec![7, 8],
    };
    assert_eq!(
        encoding::decode_chunk_body_frame(&encoding::encode_chunk_body_frame(&chunk)?)?,
        chunk
    );
    assert_eq!(
        encoding::decode_epoch_hint(&encoding::epoch_hint_m(&PeerId([8; 32]), Epoch(9)))?,
        (PeerId([8; 32]), Epoch(9))
    );
    Ok(())
}

#[test]
fn directory_ad_and_forget_use_raw_ids_codes_and_numeric_address() -> Result<()> {
    let ad = DirectoryAd {
        peer_id: PeerId([1; 32]),
        vault_id: VaultId([2; 32]),
        addr: SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 4040, 0, 0)),
        ek: vec![3, 0x80],
        vk: vec![4, 0xff],
        issued_at: 0x0102_0304_0506_0708,
        signature: vec![5, 6],
    };
    let message = encoding::directory_ad_m(&ad)?;
    assert!(message.windows(8).any(|bytes| bytes == b"[::1]:40"));
    assert_eq!(
        encoding::decode_directory_ad(&encoding::encode_directory_ad(&ad)?)?,
        ad
    );

    let forget = DirForget {
        peer_id: PeerId([7; 32]),
        vault_id: VaultId([8; 32]),
        join_code: JoinCode([9; 16]),
        issued_at: 10,
        signature: vec![11],
    };
    let forget_m = encoding::dir_forget_m(&forget);
    assert_eq!(&forget_m[64..80], &[9; 16]);
    assert_eq!(
        encoding::decode_dir_forget(&encoding::encode_dir_forget(&forget)?)?,
        forget
    );
    Ok(())
}

#[test]
fn join_request_decoder_binds_the_prior_identity_exchange() -> Result<()> {
    let exchanged = document(3);
    let request = JoinRequest {
        vault_id: VaultId([4; 32]),
        join_code: JoinCode([5; 16]),
        document: exchanged.clone(),
        signature: vec![6, 7],
    };
    let message = encoding::join_request_m(&request)?;
    assert_eq!(&message[..32], &[4; 32]);
    assert_eq!(&message[32..48], &[5; 16]);
    let encoded = encoding::encode_join_request(&request)?;
    assert!(encoding::decode_join_request(&encoded, &document(8)).is_err());
    let decoded = encoding::decode_join_request(&encoded, &exchanged)?;
    assert!(decoded == request);
    Ok(())
}

#[test]
fn all_network_decoders_reject_trailing_or_truncated_bytes() -> Result<()> {
    let identity = encoding::encode_identity(&document(1))?;
    assert!(encoding::decode_identity(&identity[..identity.len() - 1]).is_err());
    let mut packet = encoding::encode_control_packet(&ControlPacket {
        header: header(),
        ciphertext: vec![1],
    })?;
    packet.push(0);
    assert!(encoding::decode_control_packet(&packet).is_err());
    assert!(encoding::decode_epoch_hint(&[0; 39]).is_err());
    Ok(())
}

#[test]
fn frame_tags_and_wrap_ack_match_the_confirmed_wire_table() -> Result<()> {
    assert_eq!(
        [
            IDENTITY_KIND,
            EPOCH_HINT_KIND,
            WRAP_KIND,
            WRAP_ACK_KIND,
            DIR_LOOKUP_KIND,
            DIR_AD_KIND,
            DIR_PUT_KIND,
            DIR_FORGET_KIND,
            JOIN_KIND,
            GCM_PACKET_KIND,
            GCM_CHUNK_KIND,
            FLUSH_CHALLENGE_KIND,
            FLUSH_SIGNATURE_KIND,
            HEARTBEAT_KIND,
        ],
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
    );
    assert_eq!(
        encoding::encode_wrap_ack(Epoch(0x0102_0304_0506_0708), true),
        [1, 2, 3, 4, 5, 6, 7, 8, 1]
    );
    assert_eq!(
        encoding::decode_wrap_ack(&[1, 2, 3, 4, 5, 6, 7, 8, 0])?,
        (Epoch(0x0102_0304_0506_0708), false)
    );
    assert!(encoding::decode_wrap_ack(&[0; 8]).is_err());
    assert!(encoding::decode_wrap_ack(&[0, 0, 0, 0, 0, 0, 0, 0, 2]).is_err());
    assert!(encoding::decode_wrap_ack(&[0; 10]).is_err());
    Ok(())
}

#[test]
fn mailbox_transport_digest_is_ordered_and_ignores_host_timestamp() -> Result<()> {
    let frame = encoding::encode_mailbox_frame(&encoding::MailboxFrame::Control {
        ciphertext: vec![1, 2, 3],
    })?;
    let first = MailboxEnvelope {
        recipient_id: PeerId([2; 32]),
        sender_id: PeerId([1; 32]),
        epoch: Epoch(3),
        seq: Seq(4),
        queued_at: 10,
        ciphertext: frame,
    };
    let mut same_wire = first.clone();
    same_wire.queued_at = 99;
    assert_eq!(
        encoding::mailbox_digest(std::slice::from_ref(&first))?,
        encoding::mailbox_digest(&[same_wire])?
    );
    let decoded = encoding::decode_mailbox_envelope(&encoding::encode_mailbox_envelope(&first)?)?;
    assert_eq!(decoded.queued_at, 0);
    assert_eq!(decoded.sender_id, first.sender_id);
    assert_eq!(decoded.ciphertext, first.ciphertext);

    let mut second = first.clone();
    second.seq = Seq(5);
    assert_ne!(
        encoding::mailbox_digest(&[first.clone(), second.clone()])?,
        encoding::mailbox_digest(&[second, first.clone()])?
    );
    assert_eq!(
        encoding::flush_transport_digest(std::slice::from_ref(&first), &[])?,
        encoding::mailbox_digest(std::slice::from_ref(&first))?
    );
    assert_ne!(
        encoding::flush_transport_digest(std::slice::from_ref(&first), &[document(7)])?,
        encoding::flush_transport_digest(std::slice::from_ref(&first), &[document(8)])?
    );
    Ok(())
}

#[test]
fn welcome_flush_and_encrypted_controls_are_canonical() -> Result<()> {
    let welcome = NetWelcome {
        vault_id: VaultId([1; 32]),
        members: vec![document(2), document(3)],
        historical: vec![document(4)],
        denied: BTreeSet::from([PeerId([5; 32])]),
    };
    let decoded = encoding::decode_net_welcome(&encoding::encode_net_welcome(&welcome)?)?;
    assert!(decoded == welcome);

    let offer = FlushOffer {
        challenge: FlushChallenge([4; 32]),
        frame_count: 5,
        historical: Vec::new(),
    };
    assert_eq!(
        encoding::encode_flush_offer(&offer)?,
        [
            4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
            4, 4, 4, 0, 0, 0, 5,
        ]
    );
    assert!(encoding::decode_flush_offer(&encoding::encode_flush_offer(&offer)?)? == offer);
    let historical_offer = FlushOffer {
        historical: vec![document(6), document(7)],
        ..offer.clone()
    };
    assert!(
        encoding::decode_flush_offer(&encoding::encode_flush_offer(&historical_offer)?)?
            == historical_offer
    );
    assert!(encoding::decode_flush_offer(&[0; 35]).is_err());

    let controls = [
        NetControl::JoinAccepted {
            vault_id: welcome.vault_id,
            members: welcome.members,
            historical: welcome.historical,
            denied: welcome.denied,
        },
        NetControl::FlushEnd { digest: [6; 32] },
        NetControl::FlushApplied { digest: [7; 32] },
        NetControl::Ready,
        NetControl::Heartbeat,
    ];
    for control in controls {
        let decoded = encoding::decode_net_control(&encoding::encode_net_control(&control)?)?;
        assert!(decoded == control);
    }
    assert!(encoding::decode_net_control(&[0]).is_err());
    assert!(encoding::decode_net_control(&[4, 0]).is_err());
    Ok(())
}

#[test]
fn vault_metadata_round_trip_rejects_duplicate_members() -> Result<()> {
    let metadata = VaultMetadata {
        vault_id: VaultId([1; 32]),
        join_code: JoinCode([2; 16]),
        issued_at: 3,
        members: vec![PeerId([4; 32]), PeerId([5; 32])],
        denied: BTreeSet::from([PeerId([6; 32])]),
    };
    let decoded = encoding::decode_vault_metadata(&encoding::encode_vault_metadata(&metadata)?)?;
    assert_eq!(decoded.vault_id, metadata.vault_id);
    assert_eq!(decoded.join_code, metadata.join_code);
    assert_eq!(decoded.issued_at, metadata.issued_at);
    assert_eq!(decoded.members, metadata.members);

    let duplicate = VaultMetadata {
        members: vec![PeerId([4; 32]), PeerId([4; 32])],
        ..metadata
    };
    assert!(
        encoding::decode_vault_metadata(&encoding::encode_vault_metadata(&duplicate)?).is_err()
    );
    Ok(())
}
