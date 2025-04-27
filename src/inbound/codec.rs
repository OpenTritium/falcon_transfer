


use super::Msg;
use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use tracing::warn;

const PROTOCOL_VERSION:u8 = 0;

#[derive(Default)]
pub struct MsgCodec;

impl MsgCodec {
    const HDR_LEN: usize = size_of::<u16>() + size_of::<u8>();
    const MSG_MAX_LEN: usize = u16::MAX as usize;
}

impl Encoder<Msg> for MsgCodec {
    type Error = anyhow::Error;
    fn encode(&mut self, item: Msg, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let msg_buf = bincode::encode_to_vec(item, bincode::config::standard())?;
        let msg_len = msg_buf.len();
        dst.extend(
            ((msg_len + Self::HDR_LEN) as u16) // udp 包长
                .to_be_bytes()
                .iter()
                .copied()
                .chain([PROTOCOL_VERSION].iter().copied())
                .chain(msg_buf),
        );
        Ok(())
    }
}

impl Decoder for MsgCodec {
    type Item = Msg;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < MsgCodec::HDR_LEN {
            // 消息头未接收完
            return Ok(None);
        }
        let msg_len = u16::from_be_bytes([src[0], src[1]]) as usize;
        let protocol_version = src[2];
        if msg_len > Self::MSG_MAX_LEN {
            // 消息长度异常
            warn!("Illegal message header, clearing buffer.");
            src.clear();
            return Ok(None);
        }
        if src.len() < msg_len {
            // 消息长度大于当前缓冲区，请求扩容，等消息完整再取出
            src.reserve(msg_len - src.len());
            return Ok(None);
        }
        if protocol_version != PROTOCOL_VERSION {
            // 协议版本不对，忽略此条消息
            src.advance(msg_len);
            return Ok(None);
        }
        let (msg, _) = bincode::decode_from_slice::<Msg, _>(
            &src.split_to(msg_len)[Self::HDR_LEN..], // 截断消息长度前的部分并去除消息头
            bincode::config::standard(),
        )?;
        Ok(Some(msg))
    }
}
