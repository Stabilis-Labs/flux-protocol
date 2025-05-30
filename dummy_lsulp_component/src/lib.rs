use scrypto::prelude::*;

#[blueprint]
mod dummy_lsulp_component {
    pub struct LsuPool {}

    impl LsuPool {
        pub fn instantiate_token_pool() -> Global<LsuPool> {
            Self {}
                .instantiate()
                .prepare_to_globalize(OwnerRole::None)
                .globalize()
        }

        pub fn get_dex_valuation_xrd(&self) -> Decimal {
            dec!(11000)
        }

        pub fn get_liquidity_token_total_supply(&self) -> Decimal {
            dec!(10000)
        }
    }
}
