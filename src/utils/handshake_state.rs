#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum HandshakeState {
    Hello(Vec<u8>),
    Exchange(Vec<u8>),
    Full(Vec<u8>),
}
