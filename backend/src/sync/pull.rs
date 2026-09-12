use crate::error::Result;
use crate::protocol::pull::PullRequest;

/// Pull orchestration is deferred; requests are pairwise GCM control packets.
pub trait PullCoordinator {
    fn request(&self, request: &PullRequest) -> Result<()>;
}

pub struct PendingPullCoordinator;

impl PullCoordinator for PendingPullCoordinator {
    fn request(&self, _: &PullRequest) -> Result<()> {
        Err(crate::error::Error::NotImplemented("pull orchestration"))
    }
}
