use super::LinkResumeTaskError;
use crate::addr::EndPoint;

type SolveClosure =
    Box<dyn FnOnce() -> Result<(), super::LinkResumeTaskError> + 'static + Send + Sync>;

pub struct AssignedLink {
    local: EndPoint,
    remote: EndPoint,
    solve: SolveClosure,
}

impl AssignedLink {
    pub fn local(&self) -> &EndPoint {
        &self.local
    }

    pub fn remote(&self) -> &EndPoint {
        &self.remote
    }

    pub fn solve(self) -> Result<(), LinkResumeTaskError> {
        (self.solve)()
    }

    pub fn new(local: EndPoint, remote: EndPoint, solve: SolveClosure) -> Self {
        Self {
            local,
            remote,
            solve,
        }
    }
}
