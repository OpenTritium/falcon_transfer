use anyhow::{Context, Result};
use snow::Builder;
use snow::StatelessTransportState;

static SECRET: &[u8; 32] = b"i don't care for fidget spinners";
pub fn handshake() -> Result<()> {
    static PATTERN: &'static str = "Noise_XX_25519_AESGCM_BLAKE2b";
    let mut initiator = snow::Builder::new(PATTERN.parse()?)
        .local_private_key([].as_slice())
        .build_initiator()?;
    let mut responder = snow::Builder::new(PATTERN.parse()?)
        .local_private_key([].as_slice())
        .build_responder()?;

    let (mut read_buf, mut first_msg, mut second_msg) = ([0u8; 1024], [0u8; 1024], [0u8; 1024]);

    // -> e
    let len = initiator.write_message(&[], &mut first_msg)?;
    println!("发起者握手协议密文{:?}", first_msg);

    responder.read_message(&first_msg[..len], &mut read_buf)?;
    println!("接收者读消息明文：{:?}", read_buf);
    // <- e, ee, s, se
    let len = responder.write_message(&[], &mut second_msg)?;
    println!("回应消息密文{:?}", second_msg);

    initiator.read_message(&second_msg[..len], &mut read_buf)?;
    println!("回应消息明文{:?}", read_buf);
    // -> s, es
    let len = initiator.write_message(&[], &mut first_msg)?;
    println!("发起者握手协议密文{:?}", first_msg);

    responder.read_message(&first_msg[..len], &mut read_buf)?;
    println!("接收者读消息明文：{:?}", read_buf);

    // NN handshake complete, transition into transport mode.
    let initiator = initiator.into_stateless_transport_mode()?;
    let responder = responder.into_stateless_transport_mode()?;
    Ok(())
}

// double handshake
// 事件枚举
// 主动握手被动握手

// 注入事件
// 注入消息发送器
fn ActiveAuth() -> Result<StatelessTransportState> {
    const PATTERN: &str = "Noise_XX_25519_AESGCM_BLAKE2b";
    let builder = Builder::new(PATTERN.parse()?);
    let local_key = builder.generate_keypair()?;
    let handshake = builder
        .local_private_key(&local_key.private)
        .build_initiator()?;
    handshake.into_stateless_transport_mode().context("")
}
