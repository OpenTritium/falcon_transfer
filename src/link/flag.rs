bitflags::bitflags! {
    pub struct BondStateFlag:u8 {
        const DISCOVED = 0; // 全0 表示仅仅才发现
        const HELLO = 1;
        const EXCHANGE = Self::HELLO.bits() << 1;
        const FULL = Self::EXCHANGE.bits() << 1;
        // 上面三个状态只能存在一个，且仅有full能与tranfer共存
        const TRANSFER = Self::FULL.bits() << 1;
    }
}