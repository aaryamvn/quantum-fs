use core::fmt;

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub [u8; 32]);

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

opaque_id!(PeerId);
opaque_id!(FileId);
opaque_id!(ChunkId);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seq(pub u64);

#[cfg(test)]
mod tests {
    use super::PeerId;

    #[test]
    fn peer_ids_use_unsigned_byte_ordering() {
        let low = PeerId([0x7f; 32]);
        let high = PeerId([0x80; 32]);
        assert!(low < high);

        let mut prefix = [0u8; 32];
        prefix[31] = 1;
        assert!(PeerId([0u8; 32]) < PeerId(prefix));
    }

    #[test]
    fn identifiers_have_redacted_debug_output() {
        assert_eq!(format!("{:?}", PeerId([0xab; 32])), "PeerId(<redacted>)");
    }
}
