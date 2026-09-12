use crate::{
    error::{Error, Result},
    ids::ChunkId,
};

pub const MAX_PULL_CHUNK_IDS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    chunk_ids: Vec<ChunkId>,
}

impl PullRequest {
    pub fn new(chunk_ids: Vec<ChunkId>) -> Result<Self> {
        if chunk_ids.len() > MAX_PULL_CHUNK_IDS {
            return Err(Error::InvalidInput(
                "pull request exceeds the 32 chunk identifier cap",
            ));
        }
        Ok(Self { chunk_ids })
    }

    pub fn chunk_ids(&self) -> &[ChunkId] {
        &self.chunk_ids
    }
}

#[cfg(test)]
mod tests {
    use super::{PullRequest, MAX_PULL_CHUNK_IDS};
    use crate::ids::ChunkId;

    #[test]
    fn accepts_at_most_32_chunk_ids() {
        assert!(PullRequest::new(vec![ChunkId([0; 32]); MAX_PULL_CHUNK_IDS]).is_ok());
    }

    #[test]
    fn rejects_more_than_32_chunk_ids() {
        assert!(PullRequest::new(vec![ChunkId([0; 32]); MAX_PULL_CHUNK_IDS + 1]).is_err());
    }
}
