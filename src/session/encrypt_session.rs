use anyhow::{Context, Result, anyhow};
use bytes::BytesMut;
enum State {
    Initiator(snow::HandshakeState),
    Responder(snow::HandshakeState),
    Transport(snow::TransportState),
}
pub struct EncryptSession {
    // 包ge 1个recv,sender
    pub state: State,
    buf: BytesMut,
}

const PATTERN: &str = "Noise_XX_25519_AESGCM_BLAKE2b";

// 注册链路不是你的责任
impl EncryptSession {
    fn try_initiate() -> Result<Self> {
        Ok(Self {
            state: State::Initiator(
                snow::Builder::new(PATTERN.parse().unwrap())
                    .local_private_key([].as_slice())
                    .build_initiator()?,
            ),
            buf: BytesMut::with_capacity(1024),
        })
    }

    fn try_response() -> Result<Self> {
        Ok(Self {
            state: State::Initiator(
                snow::Builder::new(PATTERN.parse().unwrap())
                    .local_private_key([].as_slice())
                    .build_responder()?,
            ),
            buf: BytesMut::with_capacity(1024),
        })
    }

    fn get_handshake(&mut self) -> Option<&mut snow::HandshakeState> {
        use State::*;
        match &mut self.state {
            Initiator(handshake_state) | Responder(handshake_state) => Some(handshake_state),
            Transport(_) => None,
        }
    }

    pub fn hello(&mut self) -> Result<()> {
        let mut initiator = Self::try_initiate()?;
        let hs = initiator
            .get_handshake()
            .ok_or(anyhow!("handshake has finished"))?;
        // -> e,ee
        let sz = hs.write_message(&[], &mut self.buf)?;
        // sender 发送
        Ok(())
    }

    pub fn exchange(&mut self, msg: Vec<u8>) -> Result<()> {
        let mut responder = Self::try_response()?;
        let hs = responder
            .get_handshake()
            .ok_or(anyhow!("handshake has finished"))?;
        // <- e,ee
        hs.read_message(&msg, &mut self.buf)?;
        // -> e,ee,s,se
        let sz = hs.write_message(&[], &mut self.buf)?;
        // sender 发送
        Ok(())
    }

    pub fn full(mut self, msg: Vec<u8>) -> Result<Self> {
        use State::*;
        match &mut self.state {
            Initiator(hs) => {
                // <- e,ee,s,se
                hs.read_message(&msg, &mut self.buf)?;
                // -> s,es
                let sz = hs.write_message(&[], &mut self.buf)?;

                // sender 一下
                self.into_transport()
            }
            Responder(hs) => {
                // <- s,es
                hs.read_message(&msg, &mut self.buf)?;
                self.into_transport()
            }
            Transport(_) => Err(anyhow!("alread handshaked")),
        }
    }

    pub fn into_transport(mut self) -> Result<Self> {
        use State::*;
        let transport = match self.state {
            Initiator(hs) | Responder(hs) => hs.into_transport_mode().with_context(|| anyhow!("")),
            Transport(_) => Err(anyhow!("")),
        }?;
        self.state = Transport(transport);
        Ok(self)
    }
}
