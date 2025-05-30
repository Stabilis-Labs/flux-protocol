//! # Dummy Oracle Blueprint
//! Component for testing price verification without external dependencies.

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
            set_price => restrict_to: [OWNER];
            add_morpher_identifier => restrict_to: [OWNER];
            set_max_price_age => restrict_to: [OWNER];
        }
    }

    struct Oracle {
        morpher_identifiers: HashMap<ResourceAddress, String>,
        prices: HashMap<String, Decimal>,
        max_price_age: i64,
    }

    impl Oracle {
        pub fn instantiate_oracle(xrd_address: ResourceAddress, lsulp_address: ResourceAddress) -> Global<Oracle> {
            let mut morpher_identifiers: HashMap<ResourceAddress, String> = HashMap::new();
            let mut prices: HashMap<String, Decimal> = HashMap::new();

            // Add default XRD price
            morpher_identifiers.insert(xrd_address, "XRD".to_string());
            morpher_identifiers.insert(lsulp_address, "LSULP".to_string());
            prices.insert("LSULP".to_string(), dec!(2)); // Default LSULP price
            prices.insert("XRD".to_string(), dec!(1)); // Default XRD price

            Self {
                morpher_identifiers,
                prices,
                max_price_age: 120,
            }
            .instantiate()
            .prepare_to_globalize(OwnerRole::None)
            .metadata(metadata! {
                init {
                    "name" => "Ersatz Dummy Oracle".to_string(), updatable;
                    "description" => "A dummy oracle used for testing Ersatz".to_string(), updatable;
                    "info_url" => Url::of("https://ilikeitstable.com"), updatable;
                }
            })
            .globalize()
        }

        pub fn check_price_input(
            &self,
            collateral: ResourceAddress,
            _message: String,
            _signature: String,
        ) -> Decimal {
            let market_id = self.morpher_identifiers
                .get(&collateral)
                .expect("Collateral not supported.");
            
            self.prices
                .get(market_id)
                .cloned()
                .expect("Price not set for this market")
        }

        pub fn check_price_inputs(
            &self,
            collaterals: Vec<(ResourceAddress, String, String)>,
        ) -> Vec<(ResourceAddress, Decimal)> {
            collaterals
                .into_iter()
                .map(|(collateral, _, _)| {
                    let price = self.check_price_input(collateral, String::new(), String::new());
                    (collateral, price)
                })
                .collect()
        }

        pub fn set_price(
            &mut self,
            market_id: String,
            price: Decimal,
        ) {
            self.prices.insert(market_id, price);
        }

        pub fn add_morpher_identifier(
            &mut self,
            resource_address: ResourceAddress,
            market_id: String,
        ) {
            self.morpher_identifiers.insert(resource_address, market_id.clone());
            // Initialize price if not set
            if !self.prices.contains_key(&market_id) {
                self.prices.insert(market_id, Decimal::ONE);
            }
        }

        pub fn set_max_price_age(&mut self, new_max_age: i64) {
            self.max_price_age = new_max_age;
        }
    }
}

#[derive(ScryptoSbor, Clone)]
pub struct PriceEntry {
    pub price: Decimal,
    pub changed_at: u64,
    pub identifier: String,
}
