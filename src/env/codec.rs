use crate::env::global_config;
use crate::utils::Msg;
use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use tracing::warn;

#[derive(Default)]
pub struct MsgCodec;

impl MsgCodec {
    const HEADER_LEN: usize = size_of::<u16>() + size_of::<u8>();
    const MSG_MAX_LEN: usize = 9999; // todo
}

impl Encoder<Msg> for MsgCodec {
    type Error = anyhow::Error;
    fn encode(&mut self, item: Msg, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut msg_buf = vec![]; // todo 内存分配优化
        let msg_len = bincode::encode_into_slice(item, &mut msg_buf, bincode::config::standard())?;
        dst.extend(
            ((msg_len + Self::HEADER_LEN) as u16)
                .to_be_bytes()
                .iter()
                .copied()
                .chain([global_config().protocol_version].iter().copied())
                .chain(msg_buf),
        );
        Ok(())
    }
}

impl Decoder for MsgCodec {
    type Item = Msg;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < MsgCodec::HEADER_LEN {
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
        if protocol_version != global_config().protocol_version {
            // 协议版本不对，忽略此条消息
            src.advance(msg_len);
            return Ok(None);
        }
        let (msg, _) = bincode::decode_from_slice::<Msg, _>(
            &src.split_to(msg_len)[Self::HEADER_LEN..],
            bincode::config::standard(),
        )?;
        Ok(Some(msg))
    }
}
// 支持到事件的直接解码
