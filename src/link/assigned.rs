use crate::utils::EndPoint;

pub struct AssignedLink {
    pub local: EndPoint,
    pub remote: EndPoint,
    pub solve: Box<dyn FnOnce() -> Result<(), super::ResumeTaskError> + 'static>,
}
