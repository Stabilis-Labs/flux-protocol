#![allow(deprecated)]

//! # Flux Stability Pools Blueprint
//!
//! This blueprint defines the `StabilityPools` component, which plays a crucial role in maintaining
//! the health and stability of the Flux protocol. It manages pools of fUSD deposited by users,
//! which act as the primary source of liquidity for liquidating undercollateralized CDPs.
//!
//! ## Functionality
//! - **Pooling:** Users contribute fUSD to stability pools specific to each accepted collateral type.
//!   In return, they receive pool tokens representing their share.
//! - **Liquidations:** When a CDP becomes undercollateralized, this component uses the fUSD from the
//!   corresponding collateral's stability pool to pay off the CDP's debt.
//! - **Collateral Distribution:** The liquidated collateral (potentially including a liquidation bonus)
//!   is then added to the stability pool, effectively distributing it pro-rata among the pool contributors.
//! - **Redemptions:** This component also facilitates fUSD redemptions. It coordinates with the core `Flux`
//!   component to determine the optimal redemption route across different collaterals and executes the redemptions.
//! - **Interest Charging:** Periodically triggers interest charging on CDPs via the core `Flux` component.
//! - **Reward Distribution:** Manages the distribution of rewards (e.g., liquidation profits, interest income)
//!   among stability pool contributors, liquidity providers (future feature?), and a designated payout component.
//! - **Panic Mode:** Implements a panic mode mechanism using centralized stablecoins to handle liquidations
//!   when stability pools lack sufficient fUSD.
//!
//! ## Interaction with Other Components
//! - **`Flux` (Core):** Calls methods for liquidation (`liquidate_cdp`, `check_liquidate_cdp`), redemption
//!   (`optimal_batch_redemption`, `get_optimal_redemption_route`), interest charging (`charge_interest`),
//!   and retrieving collateral information (`get_collateral_infos`). Requires controller badge authorization.
//! - **`Oracle`:** Fetches collateral prices required for liquidations, contributions, and redemptions.
//! - **`Proxy`:** Typically acts as the intermediary for user and admin actions directed at this component.
//! - **`Payout Component`:** Receives a share of protocol fees/rewards for further distribution or protocol use.
//! - **`TwoResourcePool` (Radix Pool Blueprint):** Uses instances of this blueprint to manage the liquidity
//!   (collateral/fUSD) within each stability pool.

use crate::flux_component::flux_component::*;
use crate::shared_structs::*;
use crate::events::*;
use scrypto::prelude::*;

pub type Unit = ();

#[blueprint]
#[types(NonFungibleLocalId, Instant, Unit, Hash, ResourceAddress, Vault)]
#[events(
    StabilityPoolContributionEvent,
    StabilityPoolWithdrawalEvent,
    StabilityPoolBuyEvent,
    PanicModeChangeEvent,
    PanicModeLiquidationEvent,
)]
mod stability_pools {
    enable_method_auth! {
        roles {
            flux => updatable_by: [];
            airdropper => updatable_by: [flux];
        },
        methods {
            receive_badges => PUBLIC;
            contribute_to_pool => PUBLIC;
            withdraw_from_pool => PUBLIC;
            buy_collateral_from_pool => PUBLIC;
            charge_interest => PUBLIC;
            liquidate => PUBLIC;
            redemptions => PUBLIC;
            get_stability_pool_infos => PUBLIC;
            check_and_initiate_panic_mode => PUBLIC;
            panic_mode_liquidate => PUBLIC;
            check_panic_mode_status => PUBLIC;
            set_oracle => restrict_to: [flux];
            send_badges => restrict_to: [flux];
            new_pool => restrict_to: [flux];
            edit_pool => restrict_to: [flux];
            take_liquidity_rewards => restrict_to: [flux, airdropper];
            set_parameters => restrict_to: [flux];
            set_centralized_stablecoin => restrict_to: [flux];
            set_panic_mode_parameters => restrict_to: [flux];
            set_allow_multiple_actions => restrict_to: [flux];
            claim_payout_rewards => restrict_to: [flux];
        }
    }

    /// Manages stability pools for various collateral types within the Flux protocol.
    /// Facilitates liquidations, redemptions, and reward distribution.
    struct StabilityPools {
        /// Vault holding controller badges needed to authorize calls to the `Flux` component.
        badge_vault: FungibleVault,
        /// Global reference to the oracle component used for fetching collateral prices.
        oracle: Global<AnyComponent>,
        /// Global reference to the core `Flux` component.
        flux: Global<Flux>,
        /// The name of the method to call on the oracle for batch price requests.
        oracle_batch_method_name: String,
        /// The name of the method to call on the oracle for single price requests.
        oracle_single_method_name: String,
        /// Stores information about each stability pool, keyed by the collateral's `ResourceAddress`.
        stability_pools: HashMap<ResourceAddress, StabilityPoolInfo>,
        /// Vault holding fUSD accumulated for payout to the `payout_component`.
        payout_vault: Vault,
        /// The `ResourceAddress` of the fUSD token.
        fusd_address: ResourceAddress,
        /// Default parameters applied to stability pools unless overridden individually.
        parameters: StabilityPoolParameters,
        /// `ResourceManager` for the CDP NFTs, used to fetch CDP data during liquidations.
        cdp_resource_manager: ResourceManager,
        /// List of all collateral resource addresses for which pools exist.
        collaterals: Vec<ResourceAddress>,
        /// The `ComponentAddress` of this `StabilityPools` component.
        component_address: ComponentAddress,
        /// Tracks transaction hashes to prevent certain duplicate actions within the same transaction
        /// if `allow_multiple_actions` is false.
        transactions: KeyValueStore<Hash, ()>,
        /// Stores the state related to the protocol's panic mode.
        panic_mode: PanicModeInfo,
        /// Flag to allow/disallow multiple pool contributions or interest charges in a single transaction.
        allow_multiple_actions: bool,
    }

    impl StabilityPools {
        /// Instantiates the `StabilityPools` component.
        ///
        /// Initializes the component state, including setting up default parameters and linking
        /// to essential external components like the oracle, payout component, and core `Flux` component.
        ///
        /// # Arguments
        /// * `controller_badge`: A `Bucket` containing controller badges required for authorization.
        ///                      One badge (amount 1.0) will be consumed to instantiate the PayoutComponent.
        /// * `payout_payment_token_address`: The `ResourceAddress` of the token required to claim rewards from the PayoutComponent.
        /// * `payout_initial_required_amount`: The initial `Decimal` amount of the payment token required.
        /// * `fusd_address`: The `ResourceAddress` of the fUSD token.
        /// * `oracle_address`: The `ComponentAddress` of the price oracle.
        /// * `flux_address`: The `ComponentAddress` of the core `Flux` component.
        /// * `dapp_def_address`: The `GlobalAddress` of the DApp Definition account for metadata.
        /// * `cdp_resource_address`: The `ResourceAddress` of the CDP NFT resource manager.
        /// * `initial_centralized_stablecoin`: The `ResourceAddress` of the initial stablecoin used for panic mode.
        ///
        /// # Returns
        /// * `Global<StabilityPools>`: A global reference to the newly instantiated component.
        pub fn instantiate(
            controller_badge: Bucket,
            fusd_address: ResourceAddress,
            oracle_address: ComponentAddress,
            flux_address: ComponentAddress,
            dapp_def_address: GlobalAddress,
            cdp_resource_address: ResourceAddress,
            initial_centralized_stablecoin: ResourceAddress,
            manual_liquidity_airdropper_access_rule: AccessRule,
        ) -> Global<StabilityPools> {
            let (address_reservation, component_address) =
                Runtime::allocate_component_address(StabilityPools::blueprint_id());

            let flux: Global<Flux> = Global::from(flux_address);

            let badge_address = controller_badge.resource_address();

            let owner_role = OwnerRole::Fixed(rule!(require_amount(dec!("0.75"), badge_address)));

            let parameters = StabilityPoolParameters {
                default_payout_split: dec!(0.1),
                default_liquidity_rewards_split: dec!(0.25),
                default_stability_pool_split: dec!(0.65),
                default_pool_buy_price_modifier: dec!(0.99),
                pool_contribution_flat_fee: Decimal::ZERO,
                pool_contribution_percentage_fee: Decimal::ZERO,
                liquidator_percentage_fee_share_of_profit: dec!(0.01),
                liquidator_flat_fee_share: Decimal::ZERO,
                lowest_interest_history_length: 10,
                lowest_interest_interval: 30,
                panic_mode_wait_period: 3, // use 1440 for prod
                panic_mode_cooldown_period: 3, // use 1440 for prod
            };

            let centralized_stablecoin_vaults = <scrypto::component::KeyValueStore<_, _> as stability_pools::stability_pools::StabilityPoolsKeyValueStore>::new_with_registered_type();
            centralized_stablecoin_vaults.insert(initial_centralized_stablecoin, Vault::new(initial_centralized_stablecoin));

            Self {
                badge_vault: FungibleVault::with_bucket(controller_badge.as_fungible()),
                flux,
                oracle: Global::from(oracle_address),
                oracle_batch_method_name: "check_price_inputs".to_string(),
                oracle_single_method_name: "check_price_input".to_string(),
                stability_pools: HashMap::new(),
                payout_vault: Vault::new(fusd_address),
                fusd_address,
                parameters,
                cdp_resource_manager: ResourceManager::from(cdp_resource_address),
                collaterals: vec![],
                component_address,
                transactions: <scrypto::component::KeyValueStore<_, _> as stability_pools::stability_pools::StabilityPoolsKeyValueStore>::new_with_registered_type(),
                panic_mode: PanicModeInfo {
                    is_active: false,
                    last_liquidation_time: None,
                    pending_cdps: <scrypto::component::KeyValueStore<_, _> as stability_pools::stability_pools::StabilityPoolsKeyValueStore>::new_with_registered_type(),
                    centralized_stablecoin_vaults,
                    current_centralized_stablecoin: initial_centralized_stablecoin,
                },
                allow_multiple_actions: false,
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .roles(roles! {
                flux => OWNER;
                airdropper => manual_liquidity_airdropper_access_rule;
            })
            .with_address(address_reservation)
            .metadata(metadata! {
                init {
                    "name" => "Flux Protocol StabilityPools".to_string(), updatable;
                    "description" => "A stability pools component for the Flux Protocol".to_string(), updatable;
                    "info_url" => Url::of("https://flux.ilikeitstable.com"), updatable;
                    "icon_url" => Url::of("https://flux.ilikeitstable.com/flux-logo.png"), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                }
            })
            .globalize()
        }

        /// Allows the component to receive controller badges sent from other authorized components (e.g., the Proxy).
        ///
        /// # Arguments
        /// * `badge_bucket`: A `Bucket` containing fungible controller badges.
        pub fn receive_badges(&mut self, badge_bucket: Bucket) {
            self.badge_vault.put(badge_bucket.as_fungible());
        }

        /// Sets the address and method names for interacting with the price oracle component.
        ///
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `oracle_address`: The `ComponentAddress` of the new oracle.
        /// * `single_method_name`: The method name on the oracle for fetching a single price.
        /// * `batch_method_name`: The method name on the oracle for fetching multiple prices.
        pub fn set_oracle(
            &mut self,
            oracle_address: ComponentAddress,
            single_method_name: String,
            batch_method_name: String,
        ) {
            self.oracle = Global::from(oracle_address);
            self.oracle_single_method_name = single_method_name;
            self.oracle_batch_method_name = batch_method_name;
        }

        /// Sends controller badges from this component's internal vault to another component.
        ///
        /// Requires OWNER authorization (controller badge).
        /// Assumes the receiving component has a `receive_badges` method.
        ///
        /// # Arguments
        /// * `amount`: The `Decimal` amount of badges to send.
        /// * `receiver_address`: The `ComponentAddress` of the component to receive the badges.
        pub fn send_badges(&mut self, amount: Decimal, receiver_address: ComponentAddress) {
            let receiver: Global<AnyComponent> = Global::from(receiver_address);
            let badge_bucket: Bucket = self.badge_vault.take(amount).into();
            receiver.call_raw("receive_badges", scrypto_args!(badge_bucket))
        }

        /// Creates and initializes a new stability pool for a specific collateral type.
        ///
        /// Instantiates a `TwoResourcePool` blueprint instance to manage the collateral/fUSD pool,
        /// sets up metadata for the pool unit token, and stores the pool information.
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the collateral for the new pool.
        /// * `payout_split`: Optional `Decimal` override for the share of interest/fees going to the payout component.
        /// * `liquidity_rewards_split`: Optional `Decimal` override for the share going to liquidity rewards (within the pool).
        /// * `stability_pool_split`: Optional `Decimal` override for the share remaining in the stability pool itself.
        /// * `allow_pool_buys`: `bool` - Whether users are allowed to directly buy collateral from this pool using fUSD.
        /// * `pool_buy_price_modifier`: Optional `Decimal` price modifier applied when buying collateral directly from the pool.
        /// * `pool_name`: The name for the pool unit token metadata.
        /// * `pool_description`: The description for the pool unit token metadata.
        /// * `pool_icon_url`: The icon URL for the pool unit token metadata.
        /// * `pool_token_symbol`: The symbol for the pool unit token metadata.
        /// * `pool_dapp_definition`: The DApp definition address to link in the pool unit token metadata.
        ///
        /// # Panics
        /// * If a pool for the given `collateral` already exists.
        pub fn new_pool(
            &mut self,
            collateral: ResourceAddress,
            payout_split: Option<Decimal>,
            liquidity_rewards_split: Option<Decimal>,
            stability_pool_split: Option<Decimal>,
            allow_pool_buys: bool,
            pool_buy_price_modifier: Option<Decimal>,
            pool_name: String,
            pool_description: String,
            pool_icon_url: Url,
            pool_token_symbol: String,
            pool_dapp_definition: GlobalAddress,
        ) -> ResourceAddress{
            let pool_component = Blueprint::<TwoResourcePool>::instantiate(
                OwnerRole::Fixed(rule!(require_amount(
                    dec!("0.75"),
                    self.badge_vault.resource_address(),
                ))),
                rule!(require(global_caller(self.component_address)) || 
                    require_amount(
                        dec!("0.75"),
                        self.badge_vault.resource_address(),
                    )
                ),
                (collateral, self.fusd_address),
                None,
            );

            let pool_unit_global_address: GlobalAddress =
                pool_component.get_metadata("pool_unit").unwrap().unwrap();
            let pool_unit_resource_address =
                ResourceAddress::try_from(pool_unit_global_address).unwrap();
            let pool_unit_manager = ResourceManager::from(pool_unit_resource_address);

            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                pool_unit_manager.set_metadata("name", pool_name);
                pool_unit_manager.set_metadata("description", pool_description);
                pool_unit_manager.set_metadata("symbol", pool_token_symbol);
                pool_unit_manager.set_metadata("icon_url", pool_icon_url);
                pool_unit_manager.set_metadata("dapp_definitions", pool_dapp_definition);
            });

            self.collaterals.push(collateral);

            self.stability_pools.insert(
                collateral,
                StabilityPoolInfo {
                    collateral,
                    payout_split,
                    liquidity_rewards_split,
                    stability_pool_split,
                    allow_pool_buys,
                    pool_buy_price_modifier,
                    liquidity_rewards: Vault::new(self.fusd_address),
                    pool: pool_component,
                    latest_lowest_interests: vec![],
                    last_lowest_interests_update: Clock::current_time_rounded_to_seconds(),
                },
            );

            pool_unit_resource_address
        }

        /// Edits the parameters of an existing stability pool.
        ///
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the collateral whose pool parameters are being edited.
        /// * `payout_split`: New optional `Decimal` override for the payout split.
        /// * `liquidity_rewards_split`: New optional `Decimal` override for the liquidity rewards split.
        /// * `stability_pool_split`: New optional `Decimal` override for the stability pool split.
        /// * `allow_pool_buys`: New `bool` value for allowing direct pool buys.
        /// * `pool_buy_price_modifier`: New optional `Decimal` override for the buy price modifier.
        ///
        /// # Panics
        /// * If no pool exists for the given `collateral` address.
        pub fn edit_pool(
            &mut self,
            collateral: ResourceAddress,
            payout_split: Option<Decimal>,
            liquidity_rewards_split: Option<Decimal>,
            stability_pool_split: Option<Decimal>,
            allow_pool_buys: bool,
            pool_buy_price_modifier: Option<Decimal>,
        ) {
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .payout_split = payout_split;
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .liquidity_rewards_split = liquidity_rewards_split;
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .stability_pool_split = stability_pool_split;
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .allow_pool_buys = allow_pool_buys;
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .pool_buy_price_modifier = pool_buy_price_modifier;
        }

        /// Contributes fUSD to a specific collateral's stability pool.
        ///
        /// Takes a user's fUSD contribution, potentially takes fees, calculates the required collateral
        /// amount to maintain the pool's balance based on the current price and a buy-in modifier,
        /// buys that collateral from the pool using a portion of the contribution, and then contributes
        /// the bought collateral and remaining fUSD to the underlying `TwoResourcePool`.
        /// Returns the pool units representing the user's share and any leftover tokens if not deposited.
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the collateral whose pool the user is contributing to.
        /// * `contribution`: A `Bucket` containing the fUSD contribution.
        /// * `deposit_leftover`: If `true`, attempts to deposit any minor leftover collateral/fUSD from pool operations back into the pool.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        ///
        /// # Returns
        /// * `(Bucket, Option<FungibleBucket>, Option<Bucket>)`: A tuple containing:
        ///     1. Pool unit tokens (`Bucket`) representing the contribution.
        ///     2. Optional leftover pool tokens (`Option<FungibleBucket>`) if `deposit_leftover` is `false` and the pool returned leftovers.
        ///     3. Optional leftover fUSD (`Option<Bucket>`) if `deposit_leftover` is `false` and there was fUSD remaining after internal collateral buying.
        ///
        /// # Panics
        /// * If `allow_multiple_actions` is false and this action (or `charge_interest`) has already occurred in the same transaction.
        /// * If the `contribution` bucket is not fUSD.
        /// * If the oracle call fails or provides an invalid price.
        /// * If no pool exists for the given `collateral`.
        pub fn contribute_to_pool(
            &mut self,
            collateral: ResourceAddress,
            mut contribution: Bucket,
            deposit_leftover: bool,
            message: String,
            signature: String,
        ) -> (Bucket, Option<FungibleBucket>, Option<Bucket>) {
            self.create_hash();
            let fusd_input = contribution.amount();

            assert!(
                contribution.resource_address() == self.fusd_address,
                "Invalid input."
            );

            let collateral_price: Decimal = self.oracle.call_raw(
                &self.oracle_single_method_name,
                scrypto_args!(collateral, message.clone(), signature.clone()),
            );

            if self.parameters.pool_contribution_percentage_fee > Decimal::ZERO {
                self.payout_vault.put(contribution.take(
                    fusd_input * self.parameters.pool_contribution_percentage_fee,
                ));
            }

            if self.parameters.pool_contribution_flat_fee > Decimal::ZERO {
                self.payout_vault
                    .put(contribution.take(self.parameters.pool_contribution_flat_fee));
            }

            let vault_amounts = self
                .stability_pools
                .get(&collateral)
                .unwrap()
                .pool
                .get_vault_amounts();

            let pool_fusd_value = *vault_amounts.get(&self.fusd_address).unwrap();
            let pool_collateral_value = *vault_amounts.get(&collateral).unwrap() * collateral_price;
            let buy_in_modifier = self
                .stability_pools
                .get(&collateral)
                .unwrap()
                .pool_buy_price_modifier
                .unwrap_or(self.parameters.default_pool_buy_price_modifier);

            if pool_collateral_value > Decimal::ZERO {
                let fusd_to_buy_collateral_with: Decimal =
                    (fusd_input * pool_collateral_value * buy_in_modifier)
                        / (pool_fusd_value + fusd_input + pool_collateral_value * buy_in_modifier);

                let (bought_collateral, leftover_fusd) = self.buy_collateral_from_pool_internal(
                    collateral,
                    contribution.take(fusd_to_buy_collateral_with),
                    collateral_price,
                );

                let (pool_units, leftover) = self
                    .stability_pools
                    .get_mut(&collateral)
                    .unwrap()
                    .pool
                    .contribute((bought_collateral.as_fungible(), contribution.as_fungible()));

                if deposit_leftover {
                    if let Some(leftover_bucket) = leftover {
                        self.stability_pools
                            .get_mut(&collateral)
                            .unwrap()
                            .pool
                            .protected_deposit(leftover_bucket);
                    }

                    self.stability_pools
                        .get_mut(&collateral)
                        .unwrap()
                        .pool
                        .protected_deposit(leftover_fusd.as_fungible());

                    // Emit contribution event
                    Runtime::emit_event(StabilityPoolContributionEvent {
                        collateral,
                        contribution_amount: fusd_input,
                        pool_tokens_received: pool_units.amount(),
                    });

                    return (pool_units.into(), None, None);
                }

                // Emit contribution event
                Runtime::emit_event(StabilityPoolContributionEvent {
                    collateral,
                    contribution_amount: fusd_input,
                    pool_tokens_received: pool_units.amount(),
                });

                (pool_units.into(), leftover, Some(leftover_fusd))
            } else {
                let (pool_units, leftover) = self.stability_pools
                    .get_mut(&collateral)
                    .unwrap()
                    .pool
                    .contribute((FungibleBucket::new(collateral), contribution.as_fungible()));

                // Emit contribution event
                Runtime::emit_event(StabilityPoolContributionEvent {
                    collateral,
                    contribution_amount: fusd_input,
                    pool_tokens_received: pool_units.amount(),
                });

                (pool_units.into(), leftover, None)
            }
        }

        /// Withdraws a user's contribution (collateral and fUSD) from a stability pool.
        ///
        /// Redeems the provided pool unit tokens from the underlying `TwoResourcePool`
        /// and returns the corresponding pro-rata share of collateral and fUSD held by the pool.
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the collateral whose pool the user is withdrawing from.
        /// * `tokens`: A `Bucket` containing the pool unit tokens to be redeemed.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing the withdrawn collateral and fUSD buckets.
        ///
        /// # Panics
        /// * If no pool exists for the given `collateral`.
        /// * If the `tokens` bucket resource address does not match the pool's unit token address.
        pub fn withdraw_from_pool(
            &mut self,
            collateral: ResourceAddress,
            tokens: Bucket,
        ) -> (Bucket, Bucket) {
            let input_amount = tokens.amount();

            let (bucket1, bucket2) = self
                .stability_pools
                .get_mut(&collateral)
                .unwrap()
                .pool
                .redeem(tokens.as_fungible());

            let fusd_amount = bucket2.amount();
            let collateral_amount = bucket1.amount();

            // Emit withdrawal event
            Runtime::emit_event(StabilityPoolWithdrawalEvent {
                collateral,
                pool_tokens_burned: input_amount,
                fusd_received: fusd_amount,
                collateral_received: collateral_amount,
            });

            (bucket1.into(), bucket2.into())
        }

        /// Allows a user to buy collateral directly from a stability pool using fUSD.
        ///
        /// This is only allowed if the pool's `allow_pool_buys` flag is set to `true`.
        /// Applies the pool's `pool_buy_price_modifier` (or the default one) to the oracle price.
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the collateral being bought.
        /// * `fusd`: A `Bucket` containing the fUSD payment.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing:
        ///     1. The `Bucket` of bought collateral.
        ///     2. The `Bucket` of remaining fUSD payment (if any).
        ///
        /// # Panics
        /// * If the specified `collateral` pool does not allow direct buys (`allow_pool_buys == false`).
        /// * If the oracle call fails.
        /// * If no pool exists for the given `collateral`.
        pub fn buy_collateral_from_pool(
            &mut self,
            collateral: ResourceAddress,
            fusd: Bucket,
            message: String,
            signature: String,
        ) -> (Bucket, Bucket) {
            let collateral_price: Decimal = self.oracle.call_raw(
                &self.oracle_single_method_name,
                scrypto_args!(collateral, message.clone(), signature.clone()),
            );

            let fusd_amount = fusd.amount();

            assert!(
                self.stability_pools
                    .get(&collateral)
                    .unwrap()
                    .allow_pool_buys,
                "Buying from the pool not allowed."
            );
            let (collateral_bucket, leftover_fusd) = self.buy_collateral_from_pool_internal(collateral, fusd, collateral_price);

            // Emit buy event
            Runtime::emit_event(StabilityPoolBuyEvent {
                collateral,
                fusd_paid: fusd_amount - leftover_fusd.amount(),
                collateral_received: collateral_bucket.amount(),
                effective_price: (fusd_amount - leftover_fusd.amount()) / collateral_bucket.amount(),
            });

            (collateral_bucket, leftover_fusd)
        }

        /// Allows an authorized user (OWNER) to withdraw accumulated liquidity rewards for a specific pool.
        ///
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` identifying the pool whose rewards are being withdrawn.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the withdrawn fUSD rewards.
        ///
        /// # Panics
        /// * If no pool exists for the given `collateral`.
        pub fn take_liquidity_rewards(&mut self, collateral: ResourceAddress) -> Bucket {
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .liquidity_rewards
                .take_all()
        }

        /// Triggers the charging of accrued interest on CDPs for a specific collateral type.
        ///
        /// Delegates the call to the core `Flux` component's `charge_interest` method.
        /// The collected interest (fUSD) is then split according to the pool's configured or default
        /// `payout_split`, `liquidity_rewards_split`, and `stability_pool_split` ratios.
        /// Also updates the record of the lowest interest rate seen for this collateral.
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the collateral whose CDPs should be charged interest.
        /// * `start_interest`: Optional start interest rate for the range to charge (passed to `Flux`).
        /// * `end_interest`: Optional end interest rate for the range to charge (passed to `Flux`).
        ///
        /// # Panics
        /// * If `allow_multiple_actions` is false and this action (or `contribute_to_pool`) has already occurred in the same transaction.
        /// * If the underlying `Flux::charge_interest` call fails.
        /// * If no pool exists for the given `collateral`.
        pub fn charge_interest(
            &mut self,
            collateral: ResourceAddress,
            start_interest: Option<Decimal>,
            end_interest: Option<Decimal>,
        ) {
            self.check_hash();

            let (mut fusd, lowest_interest): (Bucket, Decimal) =
                self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                    self.flux.charge_interest(
                        collateral,
                        start_interest,
                        end_interest,
                        self.get_highest_lowest_interest(collateral),
                    )
                });

            let should_update_lowest_interest_history = {
                let pool = self.stability_pools.get(&collateral).unwrap();
                let last_update_time = pool.last_lowest_interests_update
                    .add_minutes(self.parameters.lowest_interest_interval)
                    .unwrap();
                
                let current_lowest_interest = pool.latest_lowest_interests
                    .last()
                    .unwrap_or(&Decimal::ZERO);

                Clock::current_time_is_strictly_after(last_update_time, TimePrecision::Second) || 
                lowest_interest > *current_lowest_interest
            };

            if should_update_lowest_interest_history {
                self.stability_pools
                    .get_mut(&collateral)
                    .unwrap()
                    .latest_lowest_interests
                    .push(lowest_interest);

                if self
                    .stability_pools
                    .get_mut(&collateral)
                    .unwrap()
                    .latest_lowest_interests
                    .len()
                    > self.parameters.lowest_interest_history_length as usize
                {
                    self.stability_pools
                        .get_mut(&collateral)
                        .unwrap()
                        .latest_lowest_interests
                        .remove(0);
                }
            }

            let fusd_amount = fusd.amount();

            let payout_split = self
                .stability_pools
                .get(&collateral)
                .unwrap()
                .payout_split
                .unwrap_or(self.parameters.default_payout_split);
            let liquidity_rewards_split = self
                .stability_pools
                .get(&collateral)
                .unwrap()
                .liquidity_rewards_split
                .unwrap_or(self.parameters.default_liquidity_rewards_split);
            let stability_pool_split = self
                .stability_pools
                .get(&collateral)
                .unwrap()
                .stability_pool_split
                .unwrap_or(self.parameters.default_stability_pool_split);

            let split_weight = payout_split + liquidity_rewards_split + stability_pool_split;

            self.payout_vault
                .put(fusd.take(fusd_amount * payout_split / split_weight));
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .liquidity_rewards
                .put(fusd.take(fusd_amount * liquidity_rewards_split / split_weight));
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .pool
                .protected_deposit(fusd.as_fungible());
        }

        /// Initiates the liquidation of an undercollateralized CDP.
        ///
        /// Fetches the collateral price from the oracle, determines the amount of fUSD available in the
        /// stability pool, withdraws it, and calls the core `Flux` component's `liquidate_cdp` method.
        /// The returned collateral (debt coverage + profit) is processed: the profit portion (after deducting
        /// a potential liquidator fee share) is deposited back into the stability pool, and the liquidator fee share
        /// (if applicable) is returned to the caller.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to liquidate.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the liquidator's share of the profit (collateral), if any.
        ///
        /// # Panics
        /// * If the oracle call fails.
        /// * If no pool exists for the CDP's collateral type.
        /// * If the underlying `Flux::liquidate_cdp` call fails (e.g., CDP not liquidatable, insufficient pool fUSD for full liquidation initially provided).
        pub fn liquidate(
            &mut self,
            cdp_id: NonFungibleLocalId,
            message: String,
            signature: String,
        ) -> Bucket {
            let cdp_data: Cdp = self.cdp_resource_manager.get_non_fungible_data(&cdp_id);
            let collateral = cdp_data.collateral_address;

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_single_method_name,
                scrypto_args!(collateral, message, signature),
            );

            let fusd_amount_available: Decimal = *self
                .stability_pools
                .get(&collateral)
                .unwrap()
                .pool
                .get_vault_amounts()
                .get(&self.fusd_address)
                .unwrap();

            let payment = self
                .stability_pools
                .get_mut(&collateral)
                .unwrap()
                .pool
                .protected_withdraw(
                    self.fusd_address,
                    fusd_amount_available,
                    WithdrawStrategy::Rounded(RoundingMode::ToNegativeInfinity),
                );

            let (mut payout, collateral_equal_to_debt, leftover_payment) =
                self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                    self.flux.liquidate_cdp(payment.into(), cdp_id, Some(price))
                });


            let mut profit = payout.amount() - collateral_equal_to_debt;

            let mut liquidator_fee = Bucket::new(payout.resource_address());

            if profit > Decimal::ZERO {
                if self.parameters.liquidator_flat_fee_share > Decimal::ZERO {
                    liquidator_fee.put(
                        payout.take(
                            profit.min(self.parameters.liquidator_flat_fee_share),
                        ),
                    );

                    profit -= liquidator_fee.amount();
                }

                if self.parameters.liquidator_percentage_fee_share_of_profit > Decimal::ZERO {
                    liquidator_fee.put(
                        payout.take(profit.min(
                            self.parameters.liquidator_percentage_fee_share_of_profit * profit,
                        )),
                    );
                }
            }

            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .pool
                .protected_deposit(leftover_payment.as_fungible());

            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .pool
                .protected_deposit(payout.as_fungible());

            liquidator_fee
        }

        /// Performs fUSD redemptions against collateral held in the Flux protocol.
        ///
        /// First checks if any centralized stablecoin is available from panic mode redemptions.
        /// If so, it redeems using the stablecoin first.
        /// Otherwise, or if the fUSD payment is not fully covered by stablecoins, it proceeds
        /// to perform optimal batch redemptions against regular collateral pools by calling
        /// the core `Flux` component.
        ///
        /// # Arguments
        /// * `fusd`: A `Bucket` containing the fUSD to be redeemed.
        /// * `oracle_info`: A `Vec` containing tuples of `(ResourceAddress, String, String)` needed by the oracle
        ///                  to verify prices for all potential collateral types involved in the optimal redemption.
        /// * `max_redemptions`: The maximum number of individual CDP redemptions to perform across all collateral types.
        ///
        /// # Returns
        /// * `(Vec<(ResourceAddress, Bucket)>, Bucket)`: A tuple containing:
        ///     1. A `Vec` where each tuple holds the `ResourceAddress` of a redeemed collateral (or stablecoin)
        ///        and a `Bucket` containing the corresponding payout.
        ///     2. The `Bucket` containing any remaining fUSD from the initial `payment` bucket.
        ///
        /// # Panics
        /// * If the oracle call for batch prices fails.
        /// * If the underlying `Flux::optimal_batch_redemption` call fails.
        pub fn redemptions(
            &mut self,
            mut fusd: Bucket,
            oracle_info: Vec<(ResourceAddress, String, String)>,
            max_redemptions: u64,
        ) -> (Vec<(ResourceAddress, Bucket)>, Bucket) {
            // First check if there's any centralized stablecoin to redeem
            let current_stablecoin = self.panic_mode.current_centralized_stablecoin;
            
            // Create a temporary variable to store the stablecoin if we need it
            let stablecoin_redemption = {
                if let Some(mut stablecoin_vault) = self.panic_mode.centralized_stablecoin_vaults.get_mut(&current_stablecoin) {
                    if !stablecoin_vault.is_empty() {
                        let stablecoin_amount = stablecoin_vault.amount().min(fusd.amount());
                        if stablecoin_amount > Decimal::ZERO {
                            let stablecoin = stablecoin_vault.take(stablecoin_amount);
                            let fusd_payment = fusd.take(stablecoin_amount);
                            
                            // Burn the fUSD
                            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                                fusd_payment.burn();
                            });

                            // Return Some if we have a stablecoin redemption
                            Some((stablecoin, fusd.is_empty()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }; // End of block scope, stablecoin_vault is dropped here

            // Handle the stablecoin redemption if it occurred
            if let Some((stablecoin, is_empty)) = stablecoin_redemption {
                if is_empty {
                    return (vec![(current_stablecoin, stablecoin)], fusd);
                }

                // Otherwise continue with normal redemptions and add stablecoin to results
                let (mut results, remaining_fusd) = self.perform_normal_redemptions(fusd, oracle_info, max_redemptions);
                results.push((current_stablecoin, stablecoin));
                return (results, remaining_fusd);
            }

            // If no stablecoin redemption occurred, proceed with normal redemptions
            self.perform_normal_redemptions(fusd, oracle_info, max_redemptions)
        }

        // Helper method to perform normal redemptions
        fn perform_normal_redemptions(
            &mut self,
            fusd: Bucket,
            oracle_info: Vec<(ResourceAddress, String, String)>,
            max_redemptions: u64,
        ) -> (Vec<(ResourceAddress, Bucket)>, Bucket) {
            // Get prices with pre-allocated vector
            let prices: Vec<(ResourceAddress, Decimal)> = self
                .oracle
                .call_raw(&self.oracle_batch_method_name, scrypto_args!(oracle_info));

            let fusd_amount = fusd.amount();

            // Get collateral infos
            let collateral_infos_vec: Vec<CollateralInfoReturn> =
                self.flux.get_collateral_infos(self.collaterals.clone());

            // Pre-allocate vectors with known capacity
            let mut optimal_redemption_input = Vec::with_capacity(collateral_infos_vec.len());
            let mut total_debt_difference = Decimal::ZERO;

            // Single pass through collateral infos to build input and calculate total
            for collateral_info in collateral_infos_vec {
                let vault_amounts = self
                    .stability_pools
                    .get(&collateral_info.resource_address)
                    .unwrap()
                    .pool
                    .get_vault_amounts();

                let pool_amount = vault_amounts.get(&self.fusd_address).unwrap();

                let individual_difference = collateral_info.total_debt - *pool_amount;

                if individual_difference > Decimal::ZERO {
                    // Find price in single pass without additional allocations
                    let price = prices
                        .iter()
                        .find(|&&(addr, _)| addr == collateral_info.resource_address)
                        .map(|&(_, price)| price)
                        .unwrap_or_else(|| panic!("No price info found."));

                    total_debt_difference += individual_difference;

                    // Store info for later processing
                    optimal_redemption_input.push((
                        collateral_info.resource_address,
                        individual_difference,
                        Some(price),
                    ));
                }
            }

            assert!(
                total_debt_difference > Decimal::ZERO,
                "All fUSD is used within Stability Pools"
            );

            // Adjust amounts in place
            for (_, amount, _) in &mut optimal_redemption_input {
                *amount = *amount / total_debt_difference * fusd_amount;
            }

            // Perform batch redemption
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.optimal_batch_redemption(
                    fusd,
                    optimal_redemption_input,
                    None,
                    max_redemptions,
                )
            })
        }
        
        //getters
        
        /// Retrieves detailed information about specified stability pools, or all pools if none are specified.
        ///
        /// # Arguments
        /// * `resource_addresses`: An optional `Vec<ResourceAddress>` specifying which collateral pools to get info for.
        ///                       If `None`, returns info for all existing pools.
        ///
        /// # Returns
        /// * `Vec<StabilityPoolInfoReturn>`: A vector containing the information for each requested pool.
        pub fn get_stability_pool_infos(&mut self, resource_addresses: Option<Vec<ResourceAddress>>) -> Vec<StabilityPoolInfoReturn> {
            let mut info_return_vec: Vec<StabilityPoolInfoReturn> = vec![];
            
            let addresses_to_check = resource_addresses.as_ref().map(|v| v.as_slice()).unwrap_or_default();
            let should_check_addresses = resource_addresses.is_some();

            for (collateral, stability_pool) in &self.stability_pools {
                if !should_check_addresses || addresses_to_check.contains(collateral) {
                    let amounts_binding = self.stability_pools.get(collateral).unwrap().pool.get_vault_amounts();
                    let collateral_amount = amounts_binding.get(collateral).unwrap_or(&Decimal::ZERO);
                    let fusd_amount = amounts_binding.get(&self.fusd_address).unwrap_or(&Decimal::ZERO);

                    let info_return = StabilityPoolInfoReturn {
                        collateral: *collateral,
                        payout_split: stability_pool.payout_split,
                        liquidity_rewards_split: stability_pool.liquidity_rewards_split,
                        stability_pool_split: stability_pool.stability_pool_split,
                        allow_pool_buys: stability_pool.allow_pool_buys,
                        pool_buy_price_modifier: stability_pool.pool_buy_price_modifier,
                        liquidity_rewards: stability_pool.liquidity_rewards.amount(),
                        pool: stability_pool.pool,
                        collateral_amount: *collateral_amount,
                        fusd_amount: *fusd_amount,
                        latest_lowest_interests: stability_pool.latest_lowest_interests.clone(),
                        last_lowest_interests_update: stability_pool.last_lowest_interests_update,
                    };

                    info_return_vec.push(info_return);
                }
            }

            info_return_vec
        }

        //helpers

        /// Creates a transaction hash entry if `allow_multiple_actions` is false
        /// Used to prevent contributing to a pool and immediately charging interest
        ///
        /// # Panics
        /// * If `allow_multiple_actions` is `false` and an entry for the current transaction hash already exists.
        fn create_hash(&mut self) {
            let transaction_hash = Runtime::transaction_hash();
            self.transactions.insert(transaction_hash, ());
        }

        fn check_hash(&mut self) {
            let transaction_hash = Runtime::transaction_hash();
            
            if !self.allow_multiple_actions {
                assert!(
                    self.transactions.get(&transaction_hash).is_none(),
                    "Trying to execute two actions that aren't allowed in one transaction!"
                );
            }
        }

        /// Internal helper function to handle buying collateral from a pool's underlying `TwoResourcePool` component.
        /// Calculates the amount to buy based on the provided fUSD, price, and buy modifier.
        /// Withdraws collateral from the pool and deposits the fUSD payment.
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the collateral being bought (and identifying the pool).
        /// * `fusd`: A `Bucket` containing the fUSD payment.
        /// * `collateral_price`: The current `Decimal` price of the collateral.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing the bought collateral and any remaining fUSD.
        fn buy_collateral_from_pool_internal(
            &mut self,
            collateral: ResourceAddress,
            mut fusd: Bucket,
            collateral_price: Decimal,
        ) -> (Bucket, Bucket) {
            let buy_modifier = self
                .stability_pools
                .get(&collateral)
                .unwrap()
                .pool_buy_price_modifier
                .unwrap_or(self.parameters.default_pool_buy_price_modifier);

            let can_buy = fusd.amount() / (collateral_price * buy_modifier);
            let amount_binding = self.stability_pools.get(&collateral).unwrap().pool.get_vault_amounts();
            let max_available = amount_binding.get(&collateral).unwrap_or(&Decimal::ZERO);
            let can_buy_fraction = *max_available / can_buy;

            let payment_bucket = if can_buy_fraction >= Decimal::ONE {
                fusd.take(fusd.amount())
            } else {
                fusd.take(fusd.amount() * can_buy_fraction)
            };

            let buy = self
                .stability_pools
                .get_mut(&collateral)
                .unwrap()
                .pool
                .protected_withdraw(
                    collateral,
                    can_buy.min(*max_available),
                    WithdrawStrategy::Rounded(RoundingMode::ToNegativeInfinity),
                );
            self.stability_pools
                .get_mut(&collateral)
                .unwrap()
                .pool
                .protected_deposit(payment_bucket.as_fungible());

            (buy.into(), fusd)
        }

        /// Retrieves the highest interest rate recorded in the `latest_lowest_interests` history for a given collateral pool.
        /// Used when charging interest to determine the rate applied to irredeemable (-420 interest) CDPs.
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` identifying the pool.
        ///
        /// # Returns
        /// * `Decimal`: The highest interest rate found in the history, or `Decimal::ZERO` if the history is empty.
        fn get_highest_lowest_interest(&self, collateral: ResourceAddress) -> Decimal {
            *self
                .stability_pools
                .get(&collateral)
                .unwrap()
                .latest_lowest_interests
                .iter()
                .max()
                .unwrap_or(&Decimal::ZERO)
        }

        /// Sets the currently active centralized stablecoin used for panic mode liquidations and redemptions.
        /// Creates a vault for the stablecoin if it doesn't exist yet.
        ///
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `stablecoin`: The `ResourceAddress` of the new centralized stablecoin.
        pub fn set_centralized_stablecoin(&mut self, stablecoin: ResourceAddress) {
            // Create new vault for the stablecoin if it doesn't exist
            if !self.panic_mode.centralized_stablecoin_vaults.get(&stablecoin).is_some() {
                self.panic_mode.centralized_stablecoin_vaults.insert(
                    stablecoin,
                    Vault::new(stablecoin)
                );
            }
            
            self.panic_mode.current_centralized_stablecoin = stablecoin;
        }

        /// Checks if a CDP is liquidatable and if the stability pool has insufficient fUSD to cover the debt.
        /// If both conditions are met and the CDP is not already pending panic mode, it marks the CDP
        /// as pending with a wait period.
        /// If the CDP is already pending and the wait period has passed (but not exceeded by double), it activates panic mode.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to check.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        ///
        /// # Panics
        /// * If the oracle call fails.
        /// * If the `Flux::check_liquidate_cdp` call fails.
        /// * If the CDP is liquidatable but the stability pool *does* have enough fUSD.
        pub fn check_and_initiate_panic_mode(&mut self, cdp_id: NonFungibleLocalId, message: String, signature: String) {
            let cdp_data: Cdp = self.cdp_resource_manager.get_non_fungible_data(&cdp_id);
            let collateral = cdp_data.collateral_address;

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_single_method_name,
                scrypto_args!(collateral, message, signature),
            );

            let (liquidatable, required_fusd, collateral_address) = self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.check_liquidate_cdp(cdp_id.clone(), Some(price))
            });

            assert!(liquidatable, "CDP not liquidatable");

            // Check if stability pool has enough fUSD
            let pool_fusd = *self.stability_pools
                .get(&collateral_address)
                .unwrap()
                .pool
                .get_vault_amounts()
                .get(&self.fusd_address)
                .unwrap();

            assert!(pool_fusd < required_fusd, "No need for panic mode, enough fUSD available");

            // If CDP not already pending, add it with 1 day wait period
            if !self.panic_mode.pending_cdps.get(&cdp_id).is_some() {
                self.panic_mode.pending_cdps.insert(
                    cdp_id.clone(),
                    Clock::current_time_rounded_to_seconds()
                        .add_minutes(self.parameters.panic_mode_wait_period)
                        .unwrap()
                );
                Runtime::emit_event(PanicModeChangeEvent {
                    cdp_id,
                    activation_time: Clock::current_time_rounded_to_seconds(),
                    change: PanicModeEvent::Initiation,
                });
                return;
            }

            let current_time = Clock::current_time_rounded_to_seconds();
            let mut too_late = false;

            // If CDP is pending, check if it's within the valid window
            if let Some(wait_until) = self.panic_mode.pending_cdps.get(&cdp_id) {
                let final_deadline = wait_until.add_minutes(self.parameters.panic_mode_wait_period).unwrap();
                
                if Clock::current_time_is_strictly_after(*wait_until, TimePrecision::Second) {
                    if Clock::current_time_is_strictly_before(final_deadline, TimePrecision::Second) {
                        // Activate panic mode
                        self.panic_mode.is_active = true;
                        self.panic_mode.last_liquidation_time = Some(current_time);
                        Runtime::emit_event(PanicModeChangeEvent {
                            cdp_id,
                            activation_time: Clock::current_time_rounded_to_seconds(),
                            change: PanicModeEvent::Activation,
                        });
                        return;
                    } else {
                        too_late = true;
                    }
                } else {
                    panic!("Wait period has not yet ended");
                }
            }

            if too_late {
                self.panic_mode.pending_cdps.remove(&cdp_id);
                self.panic_mode.pending_cdps.insert(
                    cdp_id.clone(),
                    current_time.add_minutes(self.parameters.panic_mode_wait_period).unwrap()
                );
                Runtime::emit_event(PanicModeChangeEvent {
                    cdp_id,
                    activation_time: Clock::current_time_rounded_to_seconds(),
                    change: PanicModeEvent::TooLateActivation,
                });
            }
        }

        /// Performs a liquidation using a centralized stablecoin payment when panic mode is active.
        ///
        /// Verifies panic mode status, stablecoin payment, and CDP liquidatability.
        /// Mints fUSD 1:1 for the stablecoin payment, stores the stablecoin, and calls the core
        /// `Flux::liquidate_cdp` method using the minted fUSD.
        /// Updates the last panic mode liquidation time.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to liquidate.
        /// * `stablecoin_payment`: A `Bucket` containing the payment in the currently active centralized stablecoin.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing the liquidation payout (collateral) and any leftover fUSD (from the 1:1 mint).
        ///
        /// # Panics
        /// * If panic mode is not active.
        /// * If the `stablecoin_payment` resource address doesn't match the active stablecoin.
        /// * If the `stablecoin_payment` amount is less than the CDP's required fUSD debt.
        /// * If the oracle call fails.
        /// * If the `Flux::check_liquidate_cdp` or `Flux::liquidate_cdp` calls fail.
        /// * If the stability pool actually *does* have enough fUSD (normal liquidation should be used).
        pub fn panic_mode_liquidate(
            &mut self,
            cdp_id: NonFungibleLocalId,
            stablecoin_payment: Bucket,
            message: String,
            signature: String,
        ) -> (Bucket, Bucket) {
            let cdp_data: Cdp = self.cdp_resource_manager.get_non_fungible_data(&cdp_id);
            let collateral = cdp_data.collateral_address;

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_single_method_name,
                scrypto_args!(collateral, message, signature),
            );

            assert!(self.check_panic_mode_status(), "Panic mode not active");

            // Verify stablecoin payment
            assert!(
                stablecoin_payment.resource_address() == self.panic_mode.current_centralized_stablecoin,
                "Invalid stablecoin payment"
            );

            // Check if CDP is liquidatable
            let (liquidatable, required_fusd, collateral_address) = self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.check_liquidate_cdp(cdp_id.clone(), Some(price))
            });

            assert!(stablecoin_payment.amount() >= required_fusd, "Stablecoin payment must be greater than or equal to fUSD debt");

            assert!(liquidatable, "CDP not liquidatable");

            // Check if stability pool has enough fUSD - if so, must use normal liquidation
            let pool_fusd = *self.stability_pools
                .get(&collateral_address)
                .unwrap()
                .pool
                .get_vault_amounts()
                .get(&self.fusd_address)
                .unwrap();

            assert!(
                pool_fusd < required_fusd,
                "Must use normal liquidation - enough fUSD in stability pool"
            );

            // Convert stablecoin to fUSD 1:1
            let fusd = self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.free_fusd(stablecoin_payment.amount())
            });

            // Store stablecoin
            self.panic_mode.centralized_stablecoin_vaults
                .get_mut(&self.panic_mode.current_centralized_stablecoin)
                .unwrap()
                .put(stablecoin_payment);

            // Perform liquidation
            let (payout, leftover) = self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                let (payout, _, leftover_fusd) = self.flux.liquidate_cdp(fusd, cdp_id.clone(), Some(price));
                
                let leftover = self.panic_mode.centralized_stablecoin_vaults
                    .get_mut(&self.panic_mode.current_centralized_stablecoin)
                    .unwrap()
                    .take(leftover_fusd.amount());

                leftover_fusd.burn();

                (payout, leftover)
            });

            // Update last liquidation time
            self.panic_mode.last_liquidation_time = Some(Clock::current_time_rounded_to_seconds());

            // Emit panic mode liquidation event
            Runtime::emit_event(PanicModeLiquidationEvent {
                cdp_id,
                stablecoin_paid: required_fusd,
                collateral_received: payout.amount(),
            });

            (payout, leftover)
        }

        /// Checks the current status of panic mode.
        /// If active, it checks if the cooldown period since the last panic liquidation has passed.
        /// If the cooldown has passed, it deactivates panic mode.
        ///
        /// # Returns
        /// * `bool`: `true` if panic mode is currently active, `false` otherwise.
        pub fn check_panic_mode_status(&mut self) -> bool{
            if self.panic_mode.is_active {
                if let Some(last_liquidation) = self.panic_mode.last_liquidation_time {
                    if Clock::current_time_is_strictly_after(
                        last_liquidation.add_minutes(self.parameters.panic_mode_cooldown_period).unwrap(),
                        TimePrecision::Second
                    ) {
                        // Exit panic mode if no liquidations for a day
                        self.panic_mode.is_active = false;
                        self.panic_mode.last_liquidation_time = None;
                        return false;
                    }
                    return true;
                }
            }
            false
        }

        /// Sets the time periods (in minutes) related to panic mode activation and cooldown.
        ///
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `wait_period`: The duration (minutes) a CDP must be pending liquidation before panic mode can be activated.
        /// * `cooldown_period`: The duration (minutes) after the last panic mode liquidation before panic mode automatically deactivates.
        ///
        /// # Panics
        /// * If `wait_period` or `cooldown_period` are not positive.
        pub fn set_panic_mode_parameters(
            &mut self,
            wait_period: i64,
            cooldown_period: i64,
            lowest_interest_history_length: u64,
            lowest_interest_interval: i64,

        ) {
            assert!(wait_period > 0, "Wait period must be positive");
            assert!(cooldown_period > 0, "Cooldown period must be positive");
            
            self.parameters.panic_mode_wait_period = wait_period;
            self.parameters.panic_mode_cooldown_period = cooldown_period;
            self.parameters.lowest_interest_history_length = lowest_interest_history_length;
            self.parameters.lowest_interest_interval = lowest_interest_interval;
        }

        /// Sets whether to allow multiple contribution/interest charging actions within a single transaction.
        ///
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `allow`: `bool` - `true` to allow multiple actions, `false` to restrict to one per transaction.
        pub fn set_allow_multiple_actions(&mut self, allow: bool) {
            self.allow_multiple_actions = allow;
        }

        /// Allows the PayoutComponent (or owner) to claim the accumulated fUSD rewards.
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing all the fUSD from the `payout_vault`.
        pub fn claim_payout_rewards(&mut self) -> Bucket {
            self.payout_vault.take_all()
        }

        /// Sets the default parameters for stability pools.
        ///
        /// These parameters are used for new pools unless explicitly overridden during pool creation
        /// or editing.
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `default_payout_split`: Default share of rewards sent to the payout component.
        /// * `default_liquidity_rewards_split`: Default share reinvested as liquidity rewards.
        /// * `default_stability_pool_split`: Default share remaining in the stability pool.
        /// * `default_pool_buy_price_modifier`: Default modifier applied when buying from a pool.
        /// * `pool_contribution_flat_fee`: Flat fUSD fee charged on pool contributions.
        /// * `pool_contribution_percentage_fee`: Percentage fee charged on pool contributions.
        /// * `lowest_interest_history_length`: Number of lowest interest rate samples to keep for history.
        pub fn set_parameters(
            &mut self,
            default_payout_split: Decimal,
            default_liquidity_rewards_split: Decimal,
            default_stability_pool_split: Decimal,
            default_pool_buy_price_modifier: Decimal,
            pool_contribution_flat_fee: Decimal,
            pool_contribution_percentage_fee: Decimal,
        ) {
            self.parameters.default_payout_split = default_payout_split;
            self.parameters.default_liquidity_rewards_split = default_liquidity_rewards_split;
            self.parameters.default_stability_pool_split = default_stability_pool_split;
            self.parameters.default_pool_buy_price_modifier = default_pool_buy_price_modifier;
            self.parameters.pool_contribution_flat_fee = pool_contribution_flat_fee;
            self.parameters.pool_contribution_percentage_fee = pool_contribution_percentage_fee;
        }
    }
}

/// Holds information specific to a single stability pool tied to a collateral type.
#[derive(ScryptoSbor)]
pub struct StabilityPoolInfo {
    /// The `ResourceAddress` of the collateral associated with this pool.
    pub collateral: ResourceAddress,
    /// Optional override for the split of rewards going to the payout component.
    pub payout_split: Option<Decimal>,
    /// Optional override for the split of rewards staying in the pool for liquidity providers.
    pub liquidity_rewards_split: Option<Decimal>,
    /// Optional override for the split of rewards used to increase the stability pool's assets directly.
    pub stability_pool_split: Option<Decimal>,
    /// Flag indicating if users can directly buy collateral from this pool using fUSD.
    pub allow_pool_buys: bool,
    /// Optional override for the price modifier applied when buying collateral directly from the pool.
    pub pool_buy_price_modifier: Option<Decimal>,
    /// Vault holding accumulated fUSD rewards designated for liquidity providers in this pool.
    pub liquidity_rewards: Vault,
    /// Global reference to the underlying `TwoResourcePool` component managing this pool's liquidity.
    pub pool: Global<TwoResourcePool>,
    /// A recent history of the lowest active interest rates observed for CDPs of this collateral type.
    pub latest_lowest_interests: Vec<Decimal>,
    /// Timestamp of the last time the `latest_lowest_interests` history was updated.
    pub last_lowest_interests_update: Instant,
}

/// A structure for returning stability pool information, including current asset amounts.
#[derive(ScryptoSbor, Clone)]
pub struct StabilityPoolInfoReturn {
    /// The `ResourceAddress` of the collateral associated with this pool.
    pub collateral: ResourceAddress,
    /// The configured payout split (optional override).
    pub payout_split: Option<Decimal>,
    /// The configured liquidity rewards split (optional override).
    pub liquidity_rewards_split: Option<Decimal>,
    /// The configured stability pool split (optional override).
    pub stability_pool_split: Option<Decimal>,
    /// Flag indicating if direct collateral buys are allowed.
    pub allow_pool_buys: bool,
    /// The configured buy price modifier (optional override).
    pub pool_buy_price_modifier: Option<Decimal>,
    /// The current amount of accumulated fUSD liquidity rewards.
    pub liquidity_rewards: Decimal,
    /// Global reference to the underlying `TwoResourcePool` component.
    pub pool: Global<TwoResourcePool>,
    /// The current amount of collateral held within the pool.
    pub collateral_amount: Decimal,
    /// The current amount of fUSD held within the pool.
    pub fusd_amount: Decimal,
    /// The recent history of lowest active interest rates.
    pub latest_lowest_interests: Vec<Decimal>,
    /// Timestamp of the last update to the interest history.
    pub last_lowest_interests_update: Instant,
}

/// Configurable parameters for the stability pools component.
#[derive(ScryptoSbor, Clone)]
pub struct StabilityPoolParameters {
    /// Default split ratio for rewards/fees sent to the external payout component.
    pub default_payout_split: Decimal,
    /// Default split ratio for rewards/fees reinvested into the pool's liquidity rewards vault.
    pub default_liquidity_rewards_split: Decimal,
    /// Default split ratio for rewards/fees remaining directly in the stability pool.
    pub default_stability_pool_split: Decimal,
    /// Default price modifier applied when buying collateral directly from a pool.
    pub default_pool_buy_price_modifier: Decimal,
    /// Flat fee (fUSD) charged on contributions to stability pools.
    pub pool_contribution_flat_fee: Decimal,
    /// Percentage fee charged on contributions to stability pools.
    pub pool_contribution_percentage_fee: Decimal,
    /// The percentage share of the liquidation profit (collateral bonus) given to the liquidator.
    pub liquidator_percentage_fee_share_of_profit: Decimal,
    /// A flat fee share (in collateral) given to the liquidator from the profit.
    pub liquidator_flat_fee_share: Decimal,
    /// The number of lowest interest rate samples to store in the history.
    pub lowest_interest_history_length: u64,
    /// The interval (in minutes) between updates to the lowest interest rate history.
    pub lowest_interest_interval: i64,
    /// The required wait period (minutes) after marking a CDP before panic mode can be activated for it.
    pub panic_mode_wait_period: i64,
    /// The cooldown period (minutes) after the last panic mode liquidation before panic mode deactivates.
    pub panic_mode_cooldown_period: i64,
}

/// Holds the state related to the panic mode functionality.
#[derive(ScryptoSbor)]
pub struct PanicModeInfo {
    /// Flag indicating if panic mode is currently active.
    pub is_active: bool,
    /// Timestamp of the last panic mode liquidation.
    pub last_liquidation_time: Option<Instant>,
    /// Stores CDPs that are pending potential panic mode activation, mapped to their activation eligibility time.
    pub pending_cdps: KeyValueStore<NonFungibleLocalId, Instant>,
    /// Vaults holding different types of centralized stablecoins, keyed by their `ResourceAddress`.
    pub centralized_stablecoin_vaults: KeyValueStore<ResourceAddress, Vault>,
    /// The `ResourceAddress` of the currently designated stablecoin for panic mode operations.
    pub current_centralized_stablecoin: ResourceAddress,
}
