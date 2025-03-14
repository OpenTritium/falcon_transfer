use serde::{Deserialize, Serialize};
use std::{
    fmt::Display,
    ops::{Deref, Not},
    str::FromStr,
};
use nanoid::nanoid;

#[derive(Debug, thiserror::Error)]
pub enum UidError {
    #[error("Invalid Uid{0}")]
    Invalid(String),
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct Uid(String);

impl Uid {
    const ID_LEN: usize = 32;
    pub fn new() -> Self {
        #[allow(unused_braces)]
        Self(nanoid!({ Self::ID_LEN }))
    }
}

impl FromStr for Uid {
    type Err = UidError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != Self::ID_LEN || s.chars().all(|c| nanoid::alphabet::SAFE.contains(&c)).not() {
            return Err(UidError::Invalid(s.to_string()));
        }
        Ok(Uid(s.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_new() {
        let uid = Uid::new();
        assert_eq!(uid.len(), Uid::ID_LEN);
    }

    #[test]
    fn valid() {
        let uid = Uid::from_str(Uid::new().as_str());
        assert!(uid.is_ok());
    }

    #[test]
    #[should_panic]
    fn short_string() {
        Uid::from_str("r3u98hh3w").unwrap();
    }
    #[test]
    #[should_panic]
    fn invalid_char() {
        Uid::from_str(" ").unwrap();
    }
}
