use bincode::{Decode, Encode};
use nanoid::nanoid;
use std::{
    fmt::Display,
    ops::{Deref, Not},
    str::FromStr,
};

#[derive(Debug, thiserror::Error)]
pub enum UidError {
    #[error("Invalid Uid: {0}")]
    Invalid(String),
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Encode,Decode)]
pub struct Uid(String);

impl Uid {
    const ID_LEN: usize = 32;
    pub fn random() -> Self {
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
    fn generate() {
        let uid = Uid::random();
        assert_eq!(uid.len(), Uid::ID_LEN);
    }

    #[test]
    fn valid() {
        let uid = Uid::from_str(Uid::random().as_str());
        assert!(uid.is_ok());
    }

    #[test]
    #[should_panic]
    fn short_str_into_uid() {
        Uid::from_str("r3u98hh3w").unwrap();
    }

    #[test]
    #[should_panic]
    fn str_with_invalid_char_into_uid() {
        Uid::from_str(" ").unwrap();
    }
}
