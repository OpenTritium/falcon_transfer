use serde::{Deserialize, Serialize};
use std::{fmt::Display, ops::Deref};

#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct Uid(String);

impl Uid {
    pub fn new() -> Self {
        Self(nanoid::nanoid!())
    }
}
//todo 针对string校验
impl From<String> for Uid {
    fn from(s: String) -> Self {
        Uid(s)
    }
}

impl Deref for Uid {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
