use super::Msg;
use anyhow::anyhow;
use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use tracing::warn;

const PROTOCOL_VERSION: u8 = 0;

#[derive(Default)]
pub struct MsgCodec;

impl MsgCodec {
    const HDR_LEN: usize = size_of::<u16>() + size_of::<u8>();
    const MSG_MAX_LEN: u16 = u16::MAX;
}

impl Encoder<Msg> for MsgCodec {
    type Error = anyhow::Error;
    fn encode(&mut self, item: Msg, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let msg_buf = bincode::encode_to_vec(item, bincode::config::standard())?;
        let total_len = msg_buf
            .len()
            .checked_add(Self::HDR_LEN)
            .ok_or_else(|| anyhow!("Length overflow usize"))?;
        let total_len: u16 = total_len
            .try_into()
            .map_err(|_| anyhow!("Length overflow u16"))?;
        dst.extend(
            total_len // udp 包长
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Uid;
    use bytes::{BufMut, BytesMut};

    // 辅助函数：构造编码后的完整报文
    fn build_encoded_message(msg: &Msg, protocol_version: u8) -> BytesMut {
        let msg_buf = bincode::encode_to_vec(msg, bincode::config::standard()).unwrap();
        let total_len = msg_buf.len() + MsgCodec::HDR_LEN;

        let mut bytes = BytesMut::new();
        bytes.put_u16(total_len as u16);
        bytes.put_u8(protocol_version);
        bytes.extend_from_slice(&msg_buf);
        bytes
    }

    #[test]
    fn test_encoder_success() {
        let mut codec = MsgCodec;
        let msg = Msg::Task {
            host: Uid::random(),
            cipher: b"114514".to_vec(),
        };
        let mut buffer = BytesMut::new();

        let encoded_msg = build_encoded_message(&msg, PROTOCOL_VERSION);
        codec.encode(msg, &mut buffer).unwrap();

        assert_eq!(buffer, encoded_msg);
    }

    #[test]
    fn test_decoder_complete_message() {
        let mut codec = MsgCodec;
        let msg = Msg::Task {
            host: Uid::random(),
            cipher: b"114514".to_vec(),
        };
        let mut bytes = build_encoded_message(&msg, PROTOCOL_VERSION);

        let result = codec.decode(&mut bytes).unwrap().unwrap();
        assert_eq!(result, msg);
    }

    #[test]
    fn test_decoder_incomplete_header() {
        let mut codec = MsgCodec;
        let mut bytes = BytesMut::from([0x00, 0x00].as_slice()); // 仅2字节（不足3字节头）

        assert!(codec.decode(&mut bytes).unwrap().is_none());
    }

    #[test]
    fn test_decoder_invalid_protocol_version() {
        let mut codec = MsgCodec;
        let msg = Msg::Task {
            host: Uid::random(),
            cipher: b"114514".to_vec(),
        };
        let mut bytes = build_encoded_message(&msg, PROTOCOL_VERSION + 1); // 错误协议版本

        let result = codec.decode(&mut bytes).unwrap();
        assert!(result.is_none());
        assert!(bytes.is_empty()); // 错误版本的消息应被跳过
    }

    #[test]
    fn test_decoder_partial_body() {
        let mut codec = MsgCodec;
        let msg = Msg::Task {
            host: Uid::random(),
            cipher: b"114514".to_vec(),
        };
        let mut full_bytes = build_encoded_message(&msg, PROTOCOL_VERSION);

        // 先发送头+1字节数据（不足消息体）
        let mut bytes = full_bytes.split_to(MsgCodec::HDR_LEN + 1);
        assert!(codec.decode(&mut bytes).unwrap().is_none());

        // 补充剩余数据
        bytes.unsplit(full_bytes);
        let result = codec.decode(&mut bytes).unwrap();
        assert_eq!(result, Some(msg));
    }

    #[test]
    fn test_decoder_invalid_bincode_data() {
        let mut codec = MsgCodec;
        let mut bytes = BytesMut::new();
        bytes.put_u16(5 + MsgCodec::HDR_LEN as u16); // 总长度5+3=8
        bytes.put_u8(PROTOCOL_VERSION);
        bytes.put_slice(b"INVALID"); // 无效的bincode数据（5字节）

        let result = codec.decode(&mut bytes);
        assert!(result.is_err()); // 应返回反序列化错误
    }

    #[test]
    fn test_multiple_messages_in_stream() {
        let mut codec = MsgCodec;
        let msg1 = Msg::Task {
            host: Uid::random(),
            cipher: b"114514".to_vec(),
        };
        let msg2 = Msg::Task {
            host: Uid::random(),
            cipher: b"114514".to_vec(),
        };

        // 构建包含两个消息的字节流
        let mut bytes = build_encoded_message(&msg1, PROTOCOL_VERSION);
        bytes.unsplit(build_encoded_message(&msg2, PROTOCOL_VERSION));

        // 解析第一个消息
        let result1 = codec.decode(&mut bytes).unwrap();
        assert_eq!(result1, Some(msg1));

        // 解析第二个消息
        let result2 = codec.decode(&mut bytes).unwrap();
        assert_eq!(result2, Some(msg2));

        assert!(bytes.is_empty()); // 缓冲区应无剩余数据
    }
}
