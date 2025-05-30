//! # Oracle Blueprint
//! Component verifying collateral prices.

use scrypto::prelude::*;

#[derive(ScryptoSbor, Clone)]
pub struct PriceMessage {
    pub market_id: String,
    pub price: Decimal,
    pub nonce: u64,
    pub created_at: u64,
}

#[blueprint]
mod oracle {
    enable_method_auth! {
        methods {
            check_price_input => PUBLIC;
            check_price_inputs => PUBLIC;
            update_lsu_multiplier => PUBLIC;
            add_morpher_identifier => restrict_to: [OWNER];
            set_max_price_age => restrict_to: [OWNER];
            set_max_lsu_multiplier_age => restrict_to: [OWNER];
        }
    }

    const LSU_POOL: Global<LsuPool> = global_component!(
        LsuPool,
        //"component_rdx1cppy08xgra5tv5melsjtj79c0ngvrlmzl8hhs7vwtzknp9xxs63mfp" //mainnet
        "component_tdx_2_1cpdf8dsfslstthlvaa75kp652epw3pjn967dmf9kqhhzlger60mdn5" //stokenet dummy lsupool
    );

    extern_blueprint! {
        //"package_sim1pkgxxxxxxxxxpackgexxxxxxxxx000726633226xxxxxxxxxlk8hc9", //simulator package, uncomment to run tests
        "package_tdx_2_1phrthm8neequrhdg8jxvvwd8xazccuaa8u3ufyemysade0ckv88an2", //stokenet morpher package
        //"package_rdx1p5xvvessslnpnfam9weyzldlxr7q06gen2t3d3waa0x760g7jwxhkd", //mainnet morpher package
        MorpherOracle {
            fn check_price_input(&self, message: String, signature: String) -> PriceMessage;
        }

        // oracle address for stokenet: component_tdx_2_1cpt6kp3mqkds5uy858mqedwfglhsw25lhey59ev45ayce4yfsghf90
        // oracle address for mainnet: component_rdx1cp07hrz378zfugcf6h8f9usct4zqx7rdgjhxjwphkzxyv9h7l2q04s
    }

    extern_blueprint! {
        //"package_rdx1pkfrtmv980h85c9nvhxa7c9y0z4vxzt25c3gdzywz5l52g5t0hdeey", //mainnet lsu pool
        "package_tdx_2_1ph6p4hk03a6p8f9mqzsfs9595jug6gmv2gxteggjtep3wv52t8g2ds",
        LsuPool {
            fn get_dex_valuation_xrd(&self) -> Decimal;
            fn get_liquidity_token_total_supply(&self) -> Decimal;
        }

        // lsu lp address: resource_rdx1thksg5ng70g9mmy9ne7wz0sc7auzrrwy7fmgcxzel2gvp8pj0xxfmf
    }

    struct Oracle {
        morpher_identifiers: HashMap<ResourceAddress, String>,
        oracle_address: ComponentAddress,
        max_price_age: i64,
        lsu_lp_address: ResourceAddress,
        lsu_multiplier: Decimal,
        last_lsu_multiplier_update: Instant,
        max_lsu_multiplier_age: i64,
    }

    impl Oracle {
        pub fn instantiate_oracle(
            owner_role: OwnerRole,
            oracle_address: ComponentAddress,
            dapp_def_address: GlobalAddress,
            lsu_lp_address: ResourceAddress,
        ) -> Global<Oracle> {
            let mut morpher_identifiers: HashMap<ResourceAddress, String> = HashMap::new();

            morpher_identifiers.insert(XRD, "GATEIO:XRD_USDT".to_string());
            morpher_identifiers.insert(lsu_lp_address, "GATEIO:XRD_USDT".to_string());

            Self {
                morpher_identifiers,
                oracle_address,
                max_price_age: 120,
                lsu_lp_address,
                lsu_multiplier: LSU_POOL.get_dex_valuation_xrd()
                    / LSU_POOL.get_liquidity_token_total_supply(),
                last_lsu_multiplier_update: Clock::current_time_rounded_to_seconds(),
                max_lsu_multiplier_age: 1,
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .metadata(metadata! {
                init {
                    "name" => "Ersatz Oracle".to_string(), updatable;
                    "description" => "An oracle used to keep track of collateral prices for Ersatz".to_string(), updatable;
                    "info_url" => Url::of("https://ilikeitstable.com"), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                }
            })
            .globalize()
        }

        pub fn check_price_input(
            &mut self,
            collateral: ResourceAddress,
            message: String,
            signature: String,
        ) -> Decimal {
            let morpher_oracle = Global::<MorpherOracle>::from(self.oracle_address);
            let price_message = morpher_oracle.check_price_input(message, signature);
            self.check_message_validity(collateral, price_message.clone());

            if collateral == self.lsu_lp_address {
                self.check_for_lsu_multiplier_update();
                return price_message.price * self.lsu_multiplier;
            } else {
                return price_message.price;
            }
        }

        pub fn check_price_inputs(
            &mut self,
            collaterals: Vec<(ResourceAddress, String, String)>,
        ) -> Vec<(ResourceAddress, Decimal)> {
            let morpher_oracle = Global::<MorpherOracle>::from(self.oracle_address);
            let mut price_return: Vec<(ResourceAddress, Decimal)> = vec![];
            let mut xrd_price: Option<Decimal> = None;

            for (collateral, message, signature) in collaterals {
                if collateral == XRD {
                    if xrd_price.is_none() {
                        let price_message = morpher_oracle.check_price_input(message, signature);
                        self.check_message_validity(collateral, price_message.clone());
                        xrd_price = Some(price_message.price);
                        price_return.push((collateral, price_message.price));
                    } else {
                        price_return.push((collateral, xrd_price.unwrap()));
                    }
                } else if collateral == self.lsu_lp_address {
                    self.check_for_lsu_multiplier_update();
                    if xrd_price.is_none() {
                        let price_message = morpher_oracle.check_price_input(message, signature);
                        self.check_message_validity(collateral, price_message.clone());
                        xrd_price = Some(price_message.price);
                        price_return.push((collateral, price_message.price * self.lsu_multiplier));
                    } else {
                        price_return.push((collateral, xrd_price.unwrap() * self.lsu_multiplier));
                    }
                } else {
                    let price_message = morpher_oracle.check_price_input(message, signature);
                    self.check_message_validity(collateral, price_message.clone());
                    price_return.push((collateral, price_message.price));
                }
            }

            price_return
        }

        pub fn add_morpher_identifier(
            &mut self,
            resource_address: ResourceAddress,
            market_id: String,
        ) {
            self.morpher_identifiers.insert(resource_address, market_id);
        }

        pub fn set_max_price_age(&mut self, new_max_age: i64) {
            self.max_price_age = new_max_age;
        }

        pub fn set_max_lsu_multiplier_age(&mut self, new_max_age: i64) {
            self.max_lsu_multiplier_age = new_max_age;
        }

        pub fn update_lsu_multiplier(&mut self) {
            let lsu_lp_supply = ResourceManager::from_address(self.lsu_lp_address).total_supply().unwrap();
            self.lsu_multiplier =
                LSU_POOL.get_dex_valuation_xrd() / lsu_lp_supply;
                
            self.last_lsu_multiplier_update = Clock::current_time_rounded_to_seconds();
        }

        fn check_for_lsu_multiplier_update(&mut self) {
            if Clock::current_time_is_strictly_after(
                self.last_lsu_multiplier_update
                    .add_days(self.max_lsu_multiplier_age)
                    .unwrap(),
                TimePrecision::Second,
            ) {
                self.update_lsu_multiplier();
            }
        }

        fn check_message_validity(&self, collateral: ResourceAddress, message: PriceMessage) {
            assert_eq!(
                *self
                    .morpher_identifiers
                    .get(&collateral)
                    .expect("Collateral not supported."),
                message.market_id
            );
            assert!(
                (message.created_at as i64 + self.max_price_age)
                    > Clock::current_time_rounded_to_seconds().seconds_since_unix_epoch
            )
        }
    }
}

#[derive(ScryptoSbor, Clone)]
pub struct PriceEntry {
    pub price: Decimal,
    pub changed_at: u64,
    pub identifier: String,
}
