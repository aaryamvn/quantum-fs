use crate::{
    ids::{ChunkId, FileId},
    Error, Result,
};

pub const HAVE_MAGIC: &[u8; 12] = b"qfs/v1/have/";
pub const MAX_HAVE_CHUNK_IDS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HaveQuery {
    pub file_id: FileId,
    chunk_ids: Vec<ChunkId>,
}

impl HaveQuery {
    pub fn new(file_id: FileId, chunk_ids: Vec<ChunkId>) -> Result<Self> {
        require_count(chunk_ids.len())?;
        Ok(Self { file_id, chunk_ids })
    }

    pub fn chunk_ids(&self) -> &[ChunkId] {
        &self.chunk_ids
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HaveReply {
    pub file_id: FileId,
    pub chunk_ids: Vec<ChunkId>,
    pub have_bitset: Vec<u8>,
}

impl HaveReply {
    pub fn new(query: &HaveQuery, have_bitset: Vec<u8>) -> Result<Self> {
        let reply = Self {
            file_id: query.file_id,
            chunk_ids: query.chunk_ids.clone(),
            have_bitset,
        };
        reply.validate(query)?;
        Ok(reply)
    }

    pub fn validate(&self, query: &HaveQuery) -> Result<()> {
        require_count(self.chunk_ids.len())?;
        if self.file_id != query.file_id || self.chunk_ids != query.chunk_ids {
            return Err(Error::AuthenticationFailed);
        }
        if self.have_bitset.len() != self.chunk_ids.len().div_ceil(8) {
            return Err(Error::InvalidInput("have reply bitset length mismatch"));
        }
        Ok(())
    }

    pub fn has(&self, index: usize) -> bool {
        index < self.chunk_ids.len()
            && self
                .have_bitset
                .get(index / 8)
                .is_some_and(|byte| byte & (1 << (index % 8)) != 0)
    }
}

pub(crate) fn require_count(count: usize) -> Result<()> {
    if !(1..=MAX_HAVE_CHUNK_IDS).contains(&count) {
        return Err(Error::InvalidInput(
            "have query requires between 1 and 32 chunk identifiers",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_bounds_and_reply_correlation_are_strict() -> Result<()> {
        let file = FileId([1; 32]);
        assert!(HaveQuery::new(file, Vec::new()).is_err());
        assert!(HaveQuery::new(file, vec![ChunkId([0; 32]); 33]).is_err());
        let query = HaveQuery::new(file, vec![ChunkId([2; 32]), ChunkId([3; 32])])?;
        let reply = HaveReply::new(&query, vec![0b10])?;
        assert!(!reply.has(0));
        assert!(reply.has(1));
        assert!(!reply.has(2));

        let mut wrong = reply.clone();
        wrong.file_id = FileId([4; 32]);
        assert!(wrong.validate(&query).is_err());
        let mut wrong = reply.clone();
        wrong.chunk_ids.swap(0, 1);
        assert!(wrong.validate(&query).is_err());
        let mut wrong = reply;
        wrong.have_bitset.push(0);
        assert!(wrong.validate(&query).is_err());
        Ok(())
    }
}
