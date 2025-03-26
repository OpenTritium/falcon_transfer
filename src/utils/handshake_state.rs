use bincode::{Decode, Encode};

#[derive(Debug, Clone, Encode,Decode)]
pub enum HandshakeState {
    Hello(Vec<u8>),
    Exchange(Vec<u8>),
    Full(Vec<u8>),
}
