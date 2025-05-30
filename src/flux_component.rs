#![allow(deprecated)]

//! # The Flux Core Logic Blueprint
//!
//! This blueprint defines the core component of the Flux protocol, responsible for managing
//! Collateralized Debt Positions (CDPs), minting/burning the fUSD stablecoin, handling liquidations,
//! redemptions, and interest calculations.
//!
//! ## Overview
//! Users interact with this component (typically via a proxy) to manage their loans:
//! - **Open a Loan:** Deposit accepted collateral, choose an interest rate, and mint fUSD.
//!   The loan must be overcollateralized based on the collateral's Minimum Collateral Ratio (MCR).
//! - **Manage Loan:** Add more collateral (`top_up_cdp`), remove collateral (`remove_collateral`),
//!   borrow more fUSD (`borrow_more`), partially repay fUSD (`partial_close_cdp`), or change the interest rate (`change_cdp_interest`).
//! - **Close Loan:** Repay the outstanding fUSD debt to retrieve all collateral (`close_cdp`).
//! - **Liquidation:** If a loan's collateral value falls below its MCR threshold relative to the debt,
//!   it can be liquidated by external actors (via the `StabilityPools` component). Liquidators repay the fUSD
//!   debt and receive collateral, potentially with a bonus.
//! - **Redemption:** Users can redeem fUSD for collateral directly from the system, targeting the riskiest loans
//!   (lowest collateral ratio) first. This mechanism helps maintain the fUSD peg. Redemptions incur a fee.
//!   Redemptions are also typically initiated via the `StabilityPools` component.
//! - **Interest:** Interest accrues on outstanding fUSD debt based on the chosen rate and is charged periodically
//!   (typically via the `StabilityPools` component).
//!
//! ## Key Concepts
//! - **fUSD:** The dollar-pegged stablecoin minted by the protocol.
//! - **CDP (Collateralized Debt Position):** Represents a user's loan, holding collateral and tracking debt.
//!   Managed as an NFT (`Cdp` struct).
//! - **Collateral:** Assets accepted by the protocol to back loans (e.g., XRD, other tokens).
//! - **MCR (Minimum Collateral Ratio):** The minimum ratio of collateral value to debt value required
//!   to avoid liquidation.
//! - **Interest Rate:** Variable rates chosen by borrowers, affecting borrowing costs and redemption order.
//! - **Pool Debt / Real Debt:** Internal accounting mechanisms to handle varying interest rates efficiently.
//!   `pool_debt` is a normalized value, while `real_debt` represents the actual fUSD owed after accounting for
//!   interest accrual and the debt multiplier.
//! - **Privileged Borrowers:** Special NFTs allowing certain benefits like opting out of redemptions or
//!   receiving liquidation notices.
//!
//! ## Interaction with Other Components
//! - **`Proxy`:** Usually the main entry point for users, handling oracle price feeds and authorization.
//! - **`StabilityPools`:** Manages liquidations and redemptions, acting as the primary source of liquidity
//!   for these operations and distributing rewards/losses.
//! - **`FlashLoans`:** Borrows fUSD directly from this component for flash loan operations.
//! - **Oracle:** Provides price feeds for collateral assets.

use crate::events::*;
use crate::shared_structs::*;
use scrypto::prelude::*;
use scrypto_avltree::AvlTree;

#[blueprint]
#[types(ResourceAddress, CollateralInfo, Decimal, AvlTree<Decimal, Vec<NonFungibleLocalId>>, Vec<NonFungibleLocalId>, NonFungibleLocalId, Instant, Cdp, PrivilegedBorrowerData)]
#[events(
    EventAddCollateral,
    EventAddPoolCollateral,
    EventNewCdp,
    EventUpdateCdp,
    EventRedeemCdp,
    EventMarkCdp,
    EventCloseCdp,
    EventLiquidateCdp,
    EventChangeCollateral,
    EventChargeInterest,
)]
mod flux_component {
    enable_method_auth! {
        methods {
            open_cdp => restrict_to: [OWNER];
            top_up_cdp => restrict_to: [OWNER];
            remove_collateral => restrict_to: [OWNER];
            close_cdp => restrict_to: [OWNER];
            borrow_more => restrict_to: [OWNER];
            change_cdp_interest => restrict_to: [OWNER];
            partial_close_cdp => restrict_to: [OWNER];
            retrieve_leftover_collateral => restrict_to: [OWNER];
            liquidate_cdp => restrict_to: [OWNER];
            change_collateral_price => restrict_to: [OWNER];
            edit_collateral => restrict_to: [OWNER];
            mint_controller_badge => restrict_to: [OWNER];
            set_stops => restrict_to: [OWNER];
            set_max_vector_length => restrict_to: [OWNER];
            set_minimum_mint => restrict_to: [OWNER];
            set_fines => restrict_to: [OWNER];
            set_interest_params => restrict_to: [OWNER];
            new_collateral => restrict_to: [OWNER];
            redemption => restrict_to: [OWNER];
            batch_redemption => restrict_to: [OWNER];
            batch_redemptions => restrict_to: [OWNER];
            optimal_batch_redemption => restrict_to: [OWNER];
            free_fusd => restrict_to: [OWNER];
            burn_fusd => restrict_to: [OWNER];
            burn_loan_receipt => restrict_to: [OWNER];
            charge_interest => restrict_to: [OWNER];
            set_redemption_parameters => restrict_to: [OWNER];
            tag_irredeemable => restrict_to: [OWNER];
            unmark => restrict_to: [OWNER];
            create_privileged_borrower => restrict_to: [OWNER];
            edit_privileged_borrower => restrict_to: [OWNER];
            link_cdp_to_privileged_borrower => restrict_to: [OWNER];
            unlink_cdp_from_privileged_borrower => restrict_to: [OWNER];
            get_lowest_interest => PUBLIC;
            get_optimal_redemption_route => PUBLIC;
            get_cdps_info => PUBLIC;
            get_privileged_borrower_info => PUBLIC;
            get_collateral_infos => PUBLIC;
            get_crs => PUBLIC;
            get_next_liquidations => PUBLIC;
            get_next_redemptions => PUBLIC;
            get_debt_in_front => PUBLIC;
            get_interest_infos => PUBLIC;
            get_total_debt => PUBLIC;
            get_marked_liquidation_date => PUBLIC;
            check_liquidate_cdp => PUBLIC;
            get_fusd_address => PUBLIC;
        }
    }
    struct Flux {
        /// Stores information about each accepted collateral type, keyed by the collateral's `ResourceAddress`.
        /// Includes MCR, price, vaults, debt tracking, and interest/ratio trees.
        collaterals: KeyValueStore<ResourceAddress, CollateralInfo>,
        /// A counter to generate unique IDs for each new Collateralized Debt Position (CDP).
        cdp_counter: u64,
        /// The `ResourceManager` for the CDP NFTs (`Cdp` struct).
        cdp_manager: ResourceManager,
        /// The `ResourceManager` for the fUSD fungible token.
        fusd_manager: ResourceManager,
        /// The `ResourceManager` for the controller badge fungible token, used for authorization.
        controller_badge_manager: ResourceManager,
        /// The total amount of fUSD currently in circulation.
        circulating_fusd: Decimal,
        /// Stores various configurable parameters of the protocol.
        parameters: ProtocolParameters,
        /// Timestamp of the last recorded redemption operation. Used for calculating redemption fees.
        last_redemption: Instant,
        /// The base rate used for calculating redemption fees. This decays over time and spikes with redemption volume.
        redemption_base_rate: Decimal,
        /// The `ResourceManager` for the privileged borrower NFTs (`PrivilegedBorrowerData` struct).
        privileged_borrower_manager: ResourceManager,
        /// A counter to generate unique IDs for each new privileged borrower NFT.
        privileged_borrower_counter: u64,
    }

    impl Flux {
        /// Instantiates the core `Flux` component and associated resources.
        ///
        /// This function sets up the fundamental building blocks of the protocol:
        /// the fUSD token, the CDP NFT, the controller badge for authorization,
        /// and the privileged borrower NFT.
        ///
        /// # Arguments
        /// * `dapp_def_address`: The `GlobalAddress` of the DApp Definition account for metadata linkage.
        ///
        /// # Returns
        /// A tuple containing:
        /// * `Global<Flux>`: A global reference to the newly instantiated `Flux` component.
        /// * `Bucket`: A bucket containing the initially minted controller badges (supply: 10).
        /// * `ResourceAddress`: The resource address of the Privileged Borrower NFT manager.
        /// * `ResourceAddress`: The resource address of the CDP NFT manager.
        /// * `ResourceAddress`: The resource address of the fUSD token manager.
        ///
        /// # Logic
        /// 1. **Initialize Parameters:** Sets default values for `ProtocolParameters`.
        /// 2. **Allocate Address:** Reserves a component address for `Flux`.
        /// 3. **Create Controller Badge:**
        ///    - Creates a fungible resource (`fusdCTRL`) managed by this component.
        ///    - Sets metadata (name, symbol).
        ///    - Mints an initial supply (10 units) required for authorization within the protocol ecosystem.
        /// 4. **Create fUSD Token Manager:**
        ///    - Creates the fungible resource manager for fUSD.
        ///    - Sets metadata (name, symbol, URLs).
        ///    - Defines mint/burn roles, allowing the component itself or holders of 0.75 controller badges
        ///      (e.g., FlashLoans, StabilityPools) to mint/burn.
        /// 5. **Create CDP NFT Manager:**
        ///    - Creates the non-fungible resource manager for CDPs (`fusdLOAN`).
        ///    - Sets metadata (name, symbol, description, URLs).
        ///    - Defines mint/burn/update roles similar to fUSD, requiring the component or 0.75 controller badges.
        /// 6. **Create Privileged Borrower NFT Manager:**
        ///    - Creates the non-fungible resource manager for privileged borrowers (`fusdPRIV`).
        ///    - Sets metadata (name, symbol, description, URLs).
        ///    - Defines mint/burn/update roles similar to fUSD/CDP.
        /// 7. **Instantiate State:** Creates the `Flux` struct instance with:
        ///    - Empty `collaterals` KVS.
        ///    - Initialized counters (`cdp_counter`, `privileged_borrower_counter`).
        ///    - Resource managers created above.
        ///    - Zero `circulating_fusd`.
        ///    - Default `parameters`.
        ///    - `last_redemption` set to the current time.
        ///    - Zero `redemption_base_rate`.
        /// 8. **Globalize Component:** Makes the component globally accessible, setting its owner role
        ///    to require 0.75 controller badges and linking component metadata (name, description, URL, DApp definition).
        pub fn instantiate(dapp_def_address: GlobalAddress) -> (Global<Flux>, Bucket, ResourceAddress, ResourceAddress, ResourceAddress) {
            let parameters = ProtocolParameters {
                minimum_mint: Decimal::ONE,
                max_vector_length: 250,
                liquidation_fine: dec!("0.10"),
                stop_liquidations: false,
                stop_openings: false,
                stop_closings: false,
                stop_redemption: false,
                max_interest: dec!(1),
                interest_interval: dec!("0.001"),
                days_of_extra_interest_fee: 7,
                feeless_interest_rate_change_cooldown: 7,
                redemption_halflife_k: dec!(0.999967910367636),
                redemption_spike_k: Decimal::ONE,
                minimum_redemption_fee: dec!(0.005),
                maximum_redemption_fee: dec!(0.05),
                irredeemable_tag_fee: Decimal::ONE,
                liquidation_notice_fee: Decimal::ONE,
            };

            let (address_reservation, component_address) =
                Runtime::allocate_component_address(Flux::blueprint_id());

            let controller_role: Bucket = ResourceBuilder::new_fungible(OwnerRole::Fixed(rule!(
                require(global_caller(component_address))
            )))
            .divisibility(DIVISIBILITY_MAXIMUM)
            .metadata(metadata! (
                init {
                    "name" => "controller badge flux", locked;
                    "symbol" => "fusdCTRL", locked;
                }
            ))
            .mint_roles(mint_roles!(
                minter => rule!(require(global_caller(component_address)));
                minter_updater => rule!(deny_all);
            ))
            .mint_initial_supply(30)
            .into();

            let controller_badge_manager: ResourceManager = controller_role.resource_manager();

            let fusd_manager: ResourceManager = ResourceBuilder::new_fungible(OwnerRole::Fixed(
                rule!(require(controller_role.resource_address())),
            ))
            .divisibility(DIVISIBILITY_MAXIMUM)
            .metadata(metadata! (
                init {
                    "name" => "fUSD", updatable;
                    "symbol" => "fUSD", updatable;
                    "info_url" => "https://flux.ilikeitstable.com", updatable;
                    "icon_url" => Url::of("https://flux.ilikeitstable.com/fusd-logo.png"), updatable;
                    "tags" => vec!["stablecoin", "defi", "usd"], updatable;
                    "dapp_definitions" => vec![dapp_def_address], updatable;
                }
            ))
            .mint_roles(mint_roles!(
                minter => rule!(require(global_caller(component_address))
                || require_amount(
                    dec!("0.75"),
                    controller_role.resource_address()
                ));
                minter_updater => rule!(require_amount(
                    dec!("0.75"),
                    controller_role.resource_address()
                ));
            ))
            .burn_roles(burn_roles!(
                burner => rule!(require(global_caller(component_address))
                || require_amount(
                    dec!("0.75"),
                    controller_role.resource_address()
                ));
                burner_updater => rule!(require_amount(
                    dec!("0.75"),
                    controller_role.resource_address()
                ));
            ))
            .create_with_no_initial_supply()
            .into();

            let cdp_manager: ResourceManager =
                ResourceBuilder::new_integer_non_fungible_with_registered_type::<Cdp>(OwnerRole::Fixed(rule!(
                    require_amount(dec!("0.75"), controller_role.resource_address())
                )))
                .metadata(metadata!(
                    init {
                        "name" => "Flux Generator", locked;
                        "symbol" => "fusdGEN", locked;
                        "description" => "A receipt for your fUSD loan.", locked;
                        "info_url" => "https://flux.ilikeitstable.com", updatable;
                        "icon_url" => Url::of("https://flux.ilikeitstable.com/flux-logo.png"), updatable;
                        "dapp_definitions" => vec![dapp_def_address], updatable;
                    }
                ))
                .non_fungible_data_update_roles(non_fungible_data_update_roles!(
                    non_fungible_data_updater => rule!(require(global_caller(component_address))
                        || require_amount(
                            dec!("0.75"),
                            controller_role.resource_address()
                        ));
                    non_fungible_data_updater_updater => rule!(require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                ))
                .mint_roles(mint_roles!(
                    minter => rule!(require(global_caller(component_address))
                    || require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                    minter_updater => rule!(require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                ))
                .burn_roles(burn_roles!(
                    burner => rule!(require(global_caller(component_address))
                    || require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                    burner_updater => rule!(require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                ))
                .create_with_no_initial_supply()
                .into();

            let privileged_borrower_manager: ResourceManager =
                ResourceBuilder::new_integer_non_fungible_with_registered_type::<PrivilegedBorrowerData>(OwnerRole::Fixed(rule!(
                    require_amount(dec!("0.75"), controller_role.resource_address())
                )))
                .metadata(metadata!(
                    init {
                        "name" => "Privileged Borrower", locked;
                        "symbol" => "fusdPRIV", locked;
                        "description" => "A privileged Flux borrower badge. If you have one if these, you're part of a mighty few!", locked;
                        "info_url" => "https://flux.ilikeitstable.com", updatable;
                        "icon_url" => Url::of("https://flux.ilikeitstable.com/flux-logo.png"), updatable;
                        "dapp_definitions" => vec![dapp_def_address], updatable;
                    }
                ))
                .non_fungible_data_update_roles(non_fungible_data_update_roles!(
                    non_fungible_data_updater => rule!(require(global_caller(component_address))
                        || require_amount(
                            dec!("0.75"),
                            controller_role.resource_address()
                        ));
                    non_fungible_data_updater_updater => rule!(require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                ))
                .mint_roles(mint_roles!(
                    minter => rule!(require(global_caller(component_address))
                    || require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                    minter_updater => rule!(require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                ))
                .burn_roles(burn_roles!(
                    burner => rule!(require(global_caller(component_address))
                    || require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                    burner_updater => rule!(require_amount(
                        dec!("0.75"),
                        controller_role.resource_address()
                    ));
                ))
                .create_with_no_initial_supply()
                .into();

            let flux = Self {
                collaterals: KeyValueStore::new_with_registered_type(),
                cdp_counter: 0,
                cdp_manager,
                fusd_manager,
                controller_badge_manager,
                circulating_fusd: Decimal::ZERO,
                parameters,
                last_redemption: Clock::current_time_rounded_to_seconds(),
                redemption_base_rate: Decimal::ZERO,
                privileged_borrower_manager,
                privileged_borrower_counter: 0,
            }
            .instantiate()
            .prepare_to_globalize(OwnerRole::Fixed(rule!(require_amount(
                dec!("0.75"),
                controller_role.resource_address()
            ))))
            .with_address(address_reservation)
            .metadata(metadata! {
                init {
                    "name" => "Flux Protocol Core Logic".to_string(), updatable;
                    "description" => "The core logic component for the Flux Protocol".to_string(), updatable;
                    "info_url" => Url::of("https://flux.ilikeitstable.com"), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                }
            })
            .globalize();

            (flux, controller_role, privileged_borrower_manager.address(), cdp_manager.address(), fusd_manager.address())
        }

        /// Opens a new Collateralized Debt Position (CDP), minting fUSD against deposited collateral.
        ///
        /// # Arguments
        /// * `collateral`: A `Bucket` containing the collateral tokens to be deposited.
        /// * `fusd_to_mint`: The `Decimal` amount of fUSD the user wishes to mint.
        /// * `interest`: The desired annual interest rate for the loan. Must be divisible by `parameters.interest_interval`.
        ///               A special value of `-420` can be used if linked to a `privileged_borrower` with `redemption_opt_out` set to true.
        /// * `privileged_borrower`: An optional `NonFungibleLocalId` of a `PrivilegedBorrowerData` NFT. If provided and valid,
        ///                          it may grant special conditions (like redemption opt-out if interest is -420).
        /// * `with_price`: An optional `Decimal` to override the oracle price for this specific transaction. Used for testing or specific scenarios.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing:
        ///     1. A `Bucket` of the newly minted fUSD tokens (minus any initial interest fee).
        ///     2. A `Bucket` containing the newly minted CDP NFT (`Cdp` struct) representing the loan.
        ///
        /// # Panics
        /// * If `stop_openings` parameter is true.
        /// * If the provided `collateral` type is not accepted.
        /// * If the requested `fusd_to_mint` is below `parameters.minimum_mint`.
        /// * If the chosen `interest` rate is not valid (not divisible by interval, outside allowed range, or -420 without valid privilege).
        /// * If the collateral value (based on `collateral_amount` and price) is insufficient to meet the MCR for the `fusd_to_mint` plus any initial fees.
        /// * If linking to a privileged borrower fails validation (e.g., trying to use interest -420 without redemption opt-out).
        ///
        /// # Logic
        /// 1. **Initialization:** Increments `cdp_counter`, gets collateral details.
        /// 2. **Price Update:** If `with_price` is provided, updates the collateral's stored price.
        /// 3. **Debt Calculation:** Calculates the initial `pool_debt` based on `fusd_to_mint` and the current debt multiplier for the chosen interest rate.
        /// 4. **Interest Fee:** Calculates an initial interest fee (`extra_interest_days_fee`) based on protocol parameters.
        ///    - This fee is skipped if using a privileged borrower with interest -420.
        ///    - Adds the fee (in pool units) to the `pool_debt`.
        /// 5. **Mint fUSD:** Calls `mint_fusd` helper to mint the required fUSD (for `pool_debt`), increments `circulating_fusd` and collateral's `total_debt`.
        /// 6. **Validation:**
        ///    - Checks if minted amount >= `minimum_mint`.
        ///    - Checks if openings are allowed (`!stop_openings`).
        ///    - Checks if collateral is accepted.
        ///    - Checks if interest rate is valid.
        /// 7. **Deposit Collateral:** Puts the `collateral` bucket into the appropriate vault via `put_collateral`.
        /// 8. **Calculate & Check CR:** Calculates the Collateral Ratio (CR = `collateral_amount / pool_debt`) using `get_and_check_cr`.
        ///    - This function also asserts that the CR is above the Liquidation CR (LCR), effectively checking the MCR.
        /// 9. **Store CR:** Inserts the calculated CR and the new `cdp_id` into the `ratios_by_interest` tree using `insert_cr`.
        /// 10. **Create CDP Data:** Creates the `Cdp` struct with all loan details.
        /// 11. **Mint CDP NFT:** Mints the actual CDP NFT using `cdp_manager`.
        /// 12. **Store Interest Fee:** Takes the `real_extra_debt` portion from the minted fUSD and stores it in the collateral's `uncharged_interest` vault.
        /// 13. **Emit Event:** Emits `EventNewCdp`.
        /// 14. **Return:** Returns the remaining minted fUSD (after fee deduction) and the CDP NFT bucket.
        pub fn open_cdp(
            &mut self,
            collateral: Bucket,
            fusd_to_mint: Decimal,
            interest: Decimal,
            privileged_borrower: Option<NonFungibleLocalId>,
            with_price: Option<Decimal>,
        ) -> (Bucket, Bucket) {
            self.cdp_counter += 1;
            let collateral_address = collateral.resource_address();
            let collateral_amount = collateral.amount();

            if let Some(price) = with_price {
                self.change_collateral_price(collateral_address, price);
            }

            if let Some(ref borrower) = privileged_borrower {
                self.link_cdp_to_privileged_borrower(borrower.clone(), NonFungibleLocalId::integer(self.cdp_counter), false);
            }

            let mut pool_debt = self.real_to_pool_debt(collateral_address, interest, fusd_to_mint);

            let (real_extra_debt, pool_extra_debt) = match privileged_borrower {
                Some(ref borrower) => {
                    assert!(
                        (Decimal::ZERO <= interest && interest < self.parameters.max_interest) || interest == dec!(-420),
                        "Chosen interest not within allowed range."
                    );
                    let privileged_data: PrivilegedBorrowerData = self
                        .privileged_borrower_manager
                        .get_non_fungible_data(borrower);
                    if interest == dec!(-420) {
                        if privileged_data.redemption_opt_out {
                            (Decimal::ZERO, Decimal::ZERO)
                        } else {
                            panic!("No privileged borrower for this loan");
                        }
                    } else {
                        self.extra_interest_days_fee(
                            collateral_address,
                            interest,
                            pool_debt,
                            self.parameters.days_of_extra_interest_fee,
                        )
                    }
                }
                None => {
                    assert!(
                        Decimal::ZERO <= interest && interest < self.parameters.max_interest,
                        "Chosen interest not within allowed range."
                    );
                    if interest == dec!(-420) {
                        panic!("No privileged borrower for this loan");
                    } else {
                        self.extra_interest_days_fee(
                            collateral_address,
                            interest,
                            pool_debt,
                            self.parameters.days_of_extra_interest_fee,
                        )
                    }
                }
            };

            pool_debt += pool_extra_debt;

            let mut fusd_tokens = self.mint_fusd(collateral_address, interest, pool_debt);
            self.add_debt_to_collateral(collateral_address, fusd_tokens.amount());

            assert!(
                fusd_tokens.amount() >= self.parameters.minimum_mint,
                "Minted fUSD is less than the minimum required amount."
            );
            assert!(
                !self.parameters.stop_openings,
                "Not allowed to open loans right now."
            );
            assert!(
                self.collaterals
                    .get(&collateral.resource_address())
                    .map(|c| c.accepted)
                    .unwrap_or(false),
                "This collateral is not accepted"
            );
            assert!(
                self.is_divisible_by(interest, self.parameters.interest_interval),
                "Chosen interest rate not permitted."
            );

            //+ amount on interest, collateral, put in vault
            self.put_collateral(collateral_address, collateral, interest);
            let cr = self.get_and_check_cr(
                collateral_address,
                interest,
                collateral_amount,
                pool_debt,
                None,
            );

            self.insert_cr(
                collateral_address,
                interest,
                cr,
                NonFungibleLocalId::integer(self.cdp_counter),
            );

            let cdp = Cdp {
                key_image_url: Url::of("https://flux.ilikeitstable.com/flux-generator.png"),
                collateral_address: collateral_address,
                collateral_amount: collateral_amount,
                interest,
                last_interest_change: Clock::current_time_rounded_to_seconds(),
                pool_debt: pool_debt,
                collateral_fusd_ratio: cr,
                status: CdpStatus::Healthy,
                privileged_borrower: privileged_borrower,
            };

            let cdp_receipt: NonFungibleBucket = self
                .cdp_manager
                .mint_non_fungible(&NonFungibleLocalId::integer(self.cdp_counter), cdp.clone())
                .as_non_fungible();

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .uncharged_interest
                .put(fusd_tokens.take(real_extra_debt));

            Runtime::emit_event(EventNewCdp {
                cdp: cdp.clone(),
                cdp_id: NonFungibleLocalId::integer(self.cdp_counter),
            });

            (fusd_tokens, cdp_receipt.into())
        }

        /// Closes a CDP by repaying the full outstanding fUSD debt.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to close.
        /// * `fusd_payment`: A `Bucket` containing fUSD sufficient to cover the outstanding debt.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing:
        ///     1. The `Bucket` of collateral originally deposited in the CDP.
        ///     2. The `Bucket` containing any excess fUSD from the `fusd_payment` after covering the debt.
        ///
        /// # Panics
        /// * If `stop_closings` parameter is true.
        /// * If the `cdp_id` corresponds to a CDP that is not currently in a `Healthy` or `Marked` state.
        /// * If the `fusd_payment` bucket does not contain fUSD managed by this component.
        /// * If the `fusd_payment` amount is less than the total calculated `fusd_debt`.
        ///
        /// # Logic
        /// 1. **Fetch Data:** Retrieves the `Cdp` data for the given `cdp_id`.
        /// 2. **Calculate Debt:** Calculates the current real fUSD debt using `pool_to_real_debt`.
        /// 3. **Validation:**
        ///    - Asserts the payment amount is sufficient.
        ///    - Asserts closings are allowed (`!stop_closings`).
        ///    - Asserts the CDP status is `Healthy` or `Marked`.
        ///    - Asserts the payment is the correct fUSD resource.
        /// 4. **Retrieve Collateral:** Takes all collateral associated with the CDP using `take_collateral`.
        /// 5. **Repay Debt:** Takes the exact `fusd_debt` amount from the `fusd_payment` bucket.
        /// 6. **Burn fUSD & Update State:** Calls `unmint_fusd` to burn the repaid fUSD and decrease `circulating_fusd` and collateral's `total_debt`.
        /// 7. **Remove CR:** Removes the CDP's entry from the `ratios_by_interest` tree using `remove_cr`.
        /// 8. **Update CDP NFT:** Updates the NFT data to reflect the closed state:
        ///    - Sets `status` to `CdpStatus::Closed`.
        ///    - Sets `collateral_amount`, `collateral_fusd_ratio`, and `pool_debt` to zero.
        /// 9. **Emit Event:** Emits `EventCloseCdp`.
        /// 10. **Cleanup:** Calls `clean_up_interest_info` to potentially remove empty interest rate entries.
        /// 11. **Return:** Returns the retrieved collateral bucket and the remaining fUSD payment bucket.
        pub fn close_cdp(
            &mut self,
            cdp_id: NonFungibleLocalId,
            mut fusd_payment: Bucket,
        ) -> (Bucket, Bucket) {
            let receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            let fusd_debt: Decimal = self.pool_to_real_debt(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.pool_debt,
            );

            assert!(
                fusd_payment.amount() >= fusd_debt,
                "not enough fUSD supplied to close completely"
            );
            assert!(
                !self.parameters.stop_closings,
                "Not allowed to close loans right now."
            );
            assert!(
                receipt_data.status == CdpStatus::Healthy || receipt_data.status == CdpStatus::Marked,
                "Loan not healthy or marked. Can't close right now. In case of liquidation, retrieve collateral."
            );
            assert!(
                fusd_payment.resource_address() == self.fusd_manager.address(),
                "Invalid fUSD payment."
            );

            let collateral: Bucket = self.take_collateral(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_amount,
            );

            self.unmint_fusd(
                receipt_data.collateral_address,
                receipt_data.interest,
                fusd_payment.take(fusd_debt),
            );

            self.remove_debt_from_collateral(receipt_data.collateral_address, fusd_debt);

            self.remove_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_fusd_ratio,
                cdp_id.clone(),
            );

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "status", CdpStatus::Closed);

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "collateral_amount", Decimal::ZERO);

            self.cdp_manager.update_non_fungible_data(
                &cdp_id,
                "collateral_fusd_ratio",
                Decimal::ZERO,
            );

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "pool_debt", Decimal::ZERO);

            if let Some(ref borrower) = receipt_data.privileged_borrower {
                self.unlink_cdp_from_privileged_borrower(borrower.clone(), cdp_id.clone());
            }

            Runtime::emit_event(EventCloseCdp { cdp_id: cdp_id });

            self.clean_up_interest_info(receipt_data.interest, receipt_data.collateral_address);

            (collateral, fusd_payment)
        }

        /// Allows the owner of a liquidated or redeemed CDP NFT to retrieve any remaining collateral.
        ///
        /// After a liquidation or redemption, some collateral might remain if the debt was covered
        /// before exhausting all deposited collateral. This method allows the original owner
        /// (holder of the CDP NFT) to claim this leftover amount.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP NFT whose collateral is to be retrieved.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the leftover collateral.
        ///
        /// # Panics
        /// * If the CDP status is not `Liquidated` or `Redeemed`.
        /// * If the `collateral_amount` field in the CDP data is zero (no collateral left).
        /// * If `stop_closings` parameter is true (retrieval is considered part of closing).
        ///
        /// # Logic
        /// 1. **Fetch Data:** Retrieves the `Cdp` data for the given `cdp_id`.
        /// 2. **Validation:**
        ///    - Asserts the status is `Liquidated` or `Redeemed`.
        ///    - Asserts `collateral_amount` is greater than zero.
        ///    - Asserts closings/retrievals are allowed (`!stop_closings`).
        /// 3. **Update CDP NFT:** Sets the `collateral_amount` in the NFT data to zero.
        /// 4. **Retrieve Collateral:** Calls `take_collateral_from_leftovers` to withdraw the recorded
        ///    `collateral_amount` from the specific collateral's leftovers vault.
        /// 5. **Return:** Returns the bucket of retrieved leftover collateral.
        pub fn retrieve_leftover_collateral(&mut self, cdp_id: NonFungibleLocalId) -> Bucket {
            let receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            assert!(
                receipt_data.status == CdpStatus::Liquidated
                    || receipt_data.status == CdpStatus::Redeemed,
                "Loan not liquidated or redeemed"
            );
            assert!(
                receipt_data.collateral_amount > Decimal::ZERO,
                "No collateral leftover"
            );
            assert!(
                !self.parameters.stop_closings,
                "Not allowed to close loans right now."
            );

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "collateral_amount", Decimal::ZERO);

            self.take_collateral_from_leftovers(
                receipt_data.collateral_address,
                receipt_data.collateral_amount,
            )
        }

        /// Adds more collateral to an existing healthy or marked CDP.
        ///
        /// This increases the CDP's collateralization ratio, making it safer from liquidation.
        /// If the CDP was marked for liquidation, adding collateral might bring it back to a healthy state.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to top up.
        /// * `collateral`: A `Bucket` containing the additional collateral tokens to deposit.
        ///                 Must be the same resource type as the CDP's existing collateral.
        /// * `with_price`: An optional `Decimal` to override the oracle price for this specific transaction.
        ///
        /// # Panics
        /// * If the CDP status is not `Healthy` or `Marked`.
        /// * If the resource address of the `collateral` bucket does not match the CDP's `collateral_address`.
        /// * If recalculating the CR with the new collateral amount still results in a ratio below the MCR threshold.
        ///
        /// # Logic
        /// 1. **Fetch Data:** Retrieves the `Cdp` data for the given `cdp_id`.
        /// 2. **Price Update:** If `with_price` is provided, updates the collateral's stored price.
        /// 3. **Calculate New Amount:** Calculates the `new_collateral_amount`.
        /// 4. **Validation:**
        ///    - Asserts the CDP status is `Healthy` or `Marked`.
        ///    - Asserts the deposited collateral resource matches the CDP's collateral.
        /// 5. **Update CR Tree (Remove Old):** Removes the CDP's old CR entry from the `ratios_by_interest` tree using `remove_cr`.
        /// 6. **Calculate & Check New CR:** Calculates the new CR based on the `new_collateral_amount` and existing `pool_debt`
        ///    using `get_and_check_cr`. This also validates that the new CR is above the LCR.
        ///    - If the CDP was `Marked`, `get_and_check_cr` will attempt to unmark it if the new CR is sufficient.
        /// 7. **Update CR Tree (Insert New):** Inserts the new CR and `cdp_id` into the `ratios_by_interest` tree using `insert_cr`.
        /// 8. **Deposit Collateral:** Puts the additional `collateral` into the appropriate vault using `put_collateral`.
        /// 9. **Update CDP NFT:** Updates the `collateral_fusd_ratio` and `collateral_amount` fields in the CDP NFT data.
        ///    If unmarked, the status is also updated back to `Healthy` (handled within `get_and_check_cr` -> `unmark_if_marked`).
        /// 10. **Emit Event:** Emits `EventUpdateCdp` with the updated CDP data.
        pub fn top_up_cdp(
            &mut self,
            cdp_id: NonFungibleLocalId,
            collateral: Bucket,
            with_price: Option<Decimal>,
        ) {
            let mut receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            if let Some(price) = with_price {
                self.change_collateral_price(receipt_data.collateral_address, price);
            }

            let new_collateral_amount = receipt_data.collateral_amount + collateral.amount();

            assert!(
                receipt_data.status == CdpStatus::Healthy
                    || receipt_data.status == CdpStatus::Marked,
                "Loan not healthy or marked."
            );
            assert!(
                receipt_data.collateral_address == collateral.resource_address(),
                "Incompatible token."
            );

            self.remove_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_fusd_ratio,
                cdp_id.clone(),
            );

            let cr = self.get_and_check_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                new_collateral_amount,
                receipt_data.pool_debt,
                Some(cdp_id.clone()),
            );

            self.insert_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                cr,
                cdp_id.clone(),
            );

            self.put_collateral(
                receipt_data.collateral_address,
                collateral,
                receipt_data.interest,
            );

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "collateral_fusd_ratio", cr);

            self.cdp_manager.update_non_fungible_data(
                &cdp_id,
                "collateral_amount",
                new_collateral_amount,
            );

            receipt_data.collateral_fusd_ratio = cr;
            receipt_data.collateral_amount = new_collateral_amount;
            receipt_data.status = CdpStatus::Healthy;

            Runtime::emit_event(EventUpdateCdp {
                cdp: receipt_data,
                cdp_id: cdp_id,
            });
        }

        /// Removes a specified amount of collateral from a healthy or marked CDP.
        ///
        /// This is only possible if the CDP remains sufficiently collateralized (above MCR)
        /// after the removal.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP from which to remove collateral.
        /// * `amount`: The `Decimal` amount of collateral to remove.
        /// * `with_price`: An optional `Decimal` to override the oracle price for this specific transaction.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the removed collateral tokens.
        ///
        /// # Panics
        /// * If the CDP status is not `Healthy` or `Marked`.
        /// * If `stop_closings` parameter is true (removing collateral is restricted).
        /// * If the `amount` to remove is greater than the CDP's current `collateral_amount`.
        /// * If removing the `amount` would cause the CDP's CR to fall below the MCR threshold.
        ///
        /// # Logic
        /// 1. **Fetch Data:** Retrieves the `Cdp` data for the given `cdp_id`.
        /// 2. **Price Update:** If `with_price` is provided, updates the collateral's stored price.
        /// 3. **Calculate New Amount:** Calculates `new_collateral_amount` after removal.
        /// 4. **Validation:**
        ///    - Asserts CDP status is `Healthy` or `Marked`.
        ///    - Asserts closings/removals are allowed (`!stop_closings`).
        ///    - Asserts `new_collateral_amount` is non-negative (implicitly checked by `get_and_check_cr`).
        /// 5. **Update CR Tree (Remove Old):** Removes the CDP's old CR entry using `remove_cr`.
        /// 6. **Calculate & Check New CR:** Calculates the new CR based on `new_collateral_amount` and existing `pool_debt`
        ///    using `get_and_check_cr`. This asserts the CDP remains above LCR after removal.
        ///    - If the CDP was `Marked`, `get_and_check_cr` attempts to unmark it if the new CR is sufficient.
        /// 7. **Update CR Tree (Insert New):** Inserts the new CR and `cdp_id` using `insert_cr`.
        /// 8. **Retrieve Collateral:** Takes the specified `amount` of collateral from the vault using `take_collateral`.
        /// 9. **Update CDP NFT:** Updates the `collateral_fusd_ratio` and `collateral_amount` fields in the CDP NFT data.
        ///    Status is potentially updated back to `Healthy` if it was `Marked` (handled in step 6).
        /// 10. **Emit Event:** Emits `EventUpdateCdp` with the updated CDP data.
        /// 11. **Return:** Returns the bucket containing the removed collateral.
        pub fn remove_collateral(
            &mut self,
            cdp_id: NonFungibleLocalId,
            amount: Decimal,
            with_price: Option<Decimal>,
        ) -> Bucket {
            let mut receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            if let Some(price) = with_price {
                self.change_collateral_price(receipt_data.collateral_address, price);
            }

            let new_collateral_amount = receipt_data.collateral_amount - amount;

            assert!(
                receipt_data.status == CdpStatus::Healthy
                    || receipt_data.status == CdpStatus::Marked,
                "Loan not healthy or marked."
            );

            assert!(
                !self.parameters.stop_closings,
                "Not allowed to close loans / remove collateral right now."
            );

            self.remove_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_fusd_ratio,
                cdp_id.clone(),
            );

            let cr = self.get_and_check_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                new_collateral_amount,
                receipt_data.pool_debt,
                Some(cdp_id.clone()),
            );

            self.insert_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                cr,
                cdp_id.clone(),
            );

            let removed_collateral: Bucket = self.take_collateral(
                receipt_data.collateral_address,
                receipt_data.interest,
                amount,
            );

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "collateral_fusd_ratio", cr);
            self.cdp_manager.update_non_fungible_data(
                &cdp_id,
                "collateral_amount",
                new_collateral_amount,
            );

            receipt_data.collateral_fusd_ratio = cr;
            receipt_data.collateral_amount = new_collateral_amount;
            receipt_data.status = CdpStatus::Healthy;

            Runtime::emit_event(EventUpdateCdp {
                cdp: receipt_data,
                cdp_id: cdp_id,
            });

            removed_collateral
        }

        /// Partially repays the fUSD debt of a CDP.
        ///
        /// If the repayment amount is greater than or equal to the outstanding debt,
        /// this method automatically calls `close_cdp` to fully close the position.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to partially repay.
        /// * `repayment`: A `Bucket` containing the fUSD tokens for repayment.
        ///
        /// # Returns
        /// * `(Option<Bucket>, Option<Bucket>)`: A tuple where:
        ///     - The first element is `Some(collateral_bucket)` if the CDP was fully closed, `None` otherwise.
        ///     - The second element is `Some(leftover_payment_bucket)` if the CDP was fully closed, `None` otherwise.
        ///     - If the CDP is only partially closed, both elements are `None`.
        ///
        /// # Panics
        /// * If `stop_closings` parameter is true.
        /// * If the `repayment` bucket does not contain the protocol's fUSD token.
        /// * If the CDP status is not `Healthy` or `Marked`.
        /// * If the remaining fUSD debt after partial repayment falls below `parameters.minimum_mint` (unless the CDP is fully closed).
        ///
        /// # Logic
        /// 1. **Fetch Data:** Retrieves the `Cdp` data.
        /// 2. **Validation:**
        ///    - Asserts closings/repayments are allowed (`!stop_closings`).
        ///    - Asserts `repayment` is fUSD.
        ///    - Asserts CDP status is `Healthy` or `Marked`.
        /// 3. **Calculate Repayment:** Converts the `repayment_amount` (real fUSD) to `pool_repayment` (pool units) using `real_to_pool_debt`.
        /// 4. **Calculate New Debt:** Determines the `new_pool_debt` after subtracting `pool_repayment`.
        /// 5. **Check for Full Closure:** If `new_pool_debt` is zero or negative, call `close_cdp` and return its result.
        /// 6. **Minimum Debt Check:** If not fully closing, asserts that the remaining real debt (`pool_to_real_debt(new_pool_debt)`) is still >= `minimum_mint`.
        /// 7. **Update CR Tree (Remove Old):** Removes the old CR entry using `remove_cr`.
        /// 8. **Calculate New CR:** Calculates the new CR based on existing `collateral_amount` and `new_pool_debt` using `get_cr`.
        /// 9. **Burn Repayment & Update State:** Calls `unmint_fusd` to burn the `repayment` fUSD and update `circulating_fusd` and collateral's `total_debt`.
        /// 10. **Update CR Tree (Insert New):** Inserts the new CR and `cdp_id` using `insert_cr`.
        /// 11. **Update CDP NFT:** Updates the `collateral_fusd_ratio` and `pool_debt` fields in the CDP NFT data.
        /// 12. **Emit Event:** Emits `EventUpdateCdp`.
        /// 13. **Return:** Returns `(None, None)` as the CDP was only partially closed.
        pub fn partial_close_cdp(
            &mut self,
            cdp_id: NonFungibleLocalId,
            repayment: Bucket,
        ) -> (Option<Bucket>, Option<Bucket>) {
            let mut receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            assert!(
                !self.parameters.stop_closings,
                "Not allowed to close loans / remove collateral right now."
            );

            assert!(
                repayment.resource_address() == self.fusd_manager.address(),
                "Invalid fUSD payment."
            );

            assert!(
                receipt_data.status == CdpStatus::Healthy
                    || receipt_data.status == CdpStatus::Marked,
                "Loan not healthy or marked."
            );

            let repayment_amount = repayment.amount();

            let pool_repayment = self.real_to_pool_debt(
                receipt_data.collateral_address,
                receipt_data.interest,
                repayment_amount,
            );
            let new_pool_debt = receipt_data.pool_debt - pool_repayment;

            if new_pool_debt < Decimal::ZERO {
                let (collateral, leftover_payment): (Bucket, Bucket) =
                    self.close_cdp(cdp_id, repayment);
                return (Some(collateral), Some(leftover_payment));
            }

            assert!(
                self.pool_to_real_debt(
                    receipt_data.collateral_address,
                    receipt_data.interest,
                    new_pool_debt
                ) >= self.parameters.minimum_mint,
                "Resulting borrowed fUSD needs to be above minimum mint."
            );

            self.remove_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_fusd_ratio,
                cdp_id.clone(),
            );

            let cr = self.get_cr(receipt_data.collateral_amount, new_pool_debt);

            self.unmint_fusd(
                receipt_data.collateral_address,
                receipt_data.interest,
                repayment,
            );

            self.remove_debt_from_collateral(receipt_data.collateral_address, repayment_amount);

            self.insert_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                cr,
                cdp_id.clone(),
            );

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "collateral_fusd_ratio", cr);

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "pool_debt", new_pool_debt);

            receipt_data.pool_debt = new_pool_debt;
            receipt_data.collateral_fusd_ratio = cr;

            Runtime::emit_event(EventUpdateCdp {
                cdp: receipt_data,
                cdp_id: cdp_id,
            });

            (None, None)
        }

        /// Borrows more fUSD against an existing CDP.
        ///
        /// This increases the CDP's debt and decreases its collateralization ratio.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to borrow against.
        /// * `amount`: The `Decimal` amount of additional fUSD to borrow.
        /// * `with_interest`: Internal flag (not user-settable via proxy) - whether to charge the initial interest fee.
        /// * `check_cr`: Internal flag (not user-settable via proxy) - whether to perform the CR check (can be skipped in specific internal calls like interest charging).
        /// * `with_price`: An optional `Decimal` to override the oracle price for this specific transaction.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the newly minted fUSD (minus initial interest fee if applicable).
        ///
        /// # Panics
        /// * If the CDP status is not `Healthy` or `Marked`.
        /// * If `stop_openings` parameter is true (borrowing more is restricted).
        /// * If the collateral type associated with the CDP is no longer accepted (`accepted == false`).
        /// * If `check_cr` is true and borrowing the additional `amount` causes the CDP's CR to fall below the MCR threshold.
        ///
        /// # Logic
        /// 1. **Fetch Data:** Retrieves the `Cdp` data.
        /// 2. **Price Update:** If `with_price` is provided, updates the collateral's stored price.
        /// 3. **Calculate Additional Debt:** Converts the requested `amount` (real fUSD) to `additional_pool_debt` (pool units).
        /// 4. **Calculate Interest Fee:** If `with_interest` is true and the loan is not irredeemable (interest != -420), calculates the `extra_interest_days_fee` on the additional borrowed amount.
        /// 5. **Calculate New Debt:** Determines the `new_pool_debt` by adding `additional_pool_debt` and any `pool_extra_debt` (interest fee) to the existing `pool_debt`.
        /// 6. **Validation:**
        ///    - Asserts CDP status is `Healthy` or `Marked`.
        ///    - Asserts openings/borrowing more is allowed (`!stop_openings`).
        ///    - Asserts the collateral type is still accepted.
        /// 7. **Update CR Tree (Remove Old):** Removes the old CR entry using `remove_cr`.
        /// 8. **Calculate & Check New CR:**
        ///    - If `check_cr` is true, calculates the new CR using `get_and_check_cr`, which also validates it against LCR and potentially unmarks the CDP.
        ///    - If `check_cr` is false, calculates the new CR using `get_cr` without validation (used internally).
        /// 9. **Update CR Tree (Insert New):** Inserts the new CR and `cdp_id` using `insert_cr`.
        /// 10. **Update CDP NFT:** Updates the `collateral_fusd_ratio` and `pool_debt` fields.
        /// 11. **Mint fUSD:** Calls `mint_fusd` to mint fUSD corresponding to the `additional_pool_debt` plus the `pool_extra_debt` (interest fee).
        /// 12. **Update Global State:** Increases `circulating_fusd` and the collateral's `total_debt`.
        /// 13. **Store Interest Fee:** If an interest fee was charged, takes the `real_extra_debt` portion from the minted tokens and puts it into the collateral's `uncharged_interest` vault.
        /// 14. **Emit Event:** Emits `EventUpdateCdp`.
        /// 15. **Return:** Returns the bucket of newly minted fUSD (after potentially removing the fee part).
        pub fn borrow_more(
            &mut self,
            cdp_id: NonFungibleLocalId,
            amount: Decimal,
            with_interest: bool, // can't choose this in proxy.rs, so only for internal use
            check_cr: bool, // can't choose this in proxy.rs, so only for internal use
            with_price: Option<Decimal>,
        ) -> Bucket {
            let mut receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            if let Some(price) = with_price {
                self.change_collateral_price(receipt_data.collateral_address, price);
            }

            let additional_pool_debt = self.real_to_pool_debt(
                receipt_data.collateral_address,
                receipt_data.interest,
                amount,
            );

            let (real_extra_debt, pool_extra_debt) = match receipt_data.interest {
                interest if interest == dec!(-420) => (Decimal::ZERO, Decimal::ZERO),
                _ => {
                    if with_interest {
                        self.extra_interest_days_fee(
                            receipt_data.collateral_address,
                            receipt_data.interest,
                            additional_pool_debt,
                            self.parameters.days_of_extra_interest_fee,
                        )
                    } else {
                        (Decimal::ZERO, Decimal::ZERO)
                    }
                }
            };

            let new_pool_debt = receipt_data.pool_debt + additional_pool_debt + pool_extra_debt;

            assert!(
                receipt_data.status == CdpStatus::Healthy
                    || receipt_data.status == CdpStatus::Marked,
                "Loan not healthy or marked."
            );

            assert!(
                !self.parameters.stop_openings,
                "Not allowed to open loans right now."
            );

            assert!(
                self.collaterals
                    .get(&receipt_data.collateral_address)
                    .map(|c| c.accepted)
                    .unwrap_or(false),
                "This collateral is not accepted"
            );

            self.remove_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_fusd_ratio,
                cdp_id.clone(),
            );

            let cr = if check_cr {
                self.get_and_check_cr(
                    receipt_data.collateral_address,
                    receipt_data.interest,
                    receipt_data.collateral_amount,
                    new_pool_debt,
                    Some(cdp_id.clone()),
                )
            } else {
                self.get_cr(receipt_data.collateral_amount, new_pool_debt)
            };

            self.insert_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                cr,
                cdp_id.clone(),
            );

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "collateral_fusd_ratio", cr);
            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "pool_debt", new_pool_debt);

            receipt_data.pool_debt = new_pool_debt;
            receipt_data.collateral_fusd_ratio = cr;
            receipt_data.status = CdpStatus::Healthy;

            let mut tokens = self.mint_fusd(
                receipt_data.collateral_address,
                receipt_data.interest,
                additional_pool_debt + pool_extra_debt,
            );

            self.add_debt_to_collateral(receipt_data.collateral_address, tokens.amount());

            self.collaterals
                .get_mut(&receipt_data.collateral_address)
                .unwrap()
                .uncharged_interest
                .put(tokens.take(real_extra_debt));

            Runtime::emit_event(EventUpdateCdp {
                cdp: receipt_data,
                cdp_id: cdp_id,
            });

            tokens
        }

        /// Changes the interest rate for an existing CDP.
        ///
        /// This involves moving the CDP's accounting information (debt, collateral amount)
        /// from the old interest rate's records to the new one. It also recalculates the
        /// pool debt based on the new rate's debt multiplier and potentially charges an
        /// interest fee if the change occurs within the cooldown period.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP whose interest rate is being changed.
        /// * `interest`: The new desired annual interest rate. Must be divisible by `parameters.interest_interval`.
        ///               A special value of `-420` requires a valid linked `borrower` NFT with redemption opt-out.
        /// * `borrower`: An optional `NonFungibleLocalId` of a linked `PrivilegedBorrowerData` NFT. Required and validated
        ///               if changing the interest rate to `-420`.
        /// * `with_price`: An optional `Decimal` to override the oracle price for this specific transaction.
        ///
        /// # Panics
        /// * If the `interest` rate is invalid (not divisible by interval, outside allowed range).
        /// * If attempting to set interest to `-420` without providing a valid linked `borrower` NFT that has `redemption_opt_out` set to true.
        /// * If the CDP status is not `Healthy`.
        /// * If, after potentially adding an interest change fee, the new CR falls below the MCR threshold.
        ///
        /// # Logic
        /// 1. **Validation:**
        ///    - Asserts the new `interest` rate is valid (format, range).
        ///    - If `interest` is -420, validates the provided `borrower` NFT and its `redemption_opt_out` status.
        /// 2. **Fetch Data:** Retrieves the `Cdp` data.
        /// 3. **Price Update:** If `with_price` is provided, updates the collateral's stored price.
        /// 4. **Calculate Current Debt:** Determines the current real fUSD debt.
        /// 5. **Calculate New Pool Debt:** Calculates the equivalent `pool_debt` for the current real debt but at the *new* interest rate.
        /// 6. **Interest Change Fee:**
        ///    - Checks if the change is within the `feeless_interest_rate_change_cooldown` period.
        ///    - If yes, and not changing to interest -420 (irredeemable), calculates an interest fee (`extra_interest_days_fee`) based on the new rate and `days_of_extra_interest_fee`.
        ///    - Adds the fee (pool units) to the `new_pool_debt`.
        ///    - Mints the corresponding real fUSD fee, adds it to `total_debt` and `circulating_fusd`, and stores it in `uncharged_interest`.
        /// 7. **Validation:** Asserts the CDP status is `Healthy`.
        /// 8. **Move Collateral:** Takes the collateral from the old interest rate's vault and puts it into the new interest rate's vault using `take_collateral` and `put_collateral`.
        /// 9. **Move Debt Accounting:** Calls `move_fusd_interest` to update the `real_debt` and `pool_debt` tracked within the `InterestInfo` structs for both the old and new interest rates.
        /// 10. **Update CR Tree (Remove Old):** Removes the old CR entry using `remove_cr`.
        /// 11. **Calculate & Check New CR:** Calculates the new CR using `get_and_check_cr` based on the potentially increased `pool_debt` (due to fee) and validates it against LCR.
        /// 12. **Update CR Tree (Insert New):** Inserts the new CR using `insert_cr` under the new interest rate.
        /// 13. **Update CDP NFT:** Updates the `interest`, `collateral_fusd_ratio`, `pool_debt`, and `last_interest_change` fields in the CDP NFT data.
        /// 14. **Cleanup:** Calls `clean_up_interest_info` for the old interest rate to potentially remove empty entries.
        /// 15. **Emit Event:** Emits `EventUpdateCdp`.
        pub fn change_cdp_interest(
            &mut self,
            cdp_id: NonFungibleLocalId,
            interest: Decimal,
            borrower: Option<NonFungibleLocalId>,
            with_price: Option<Decimal>,
        ) {
            assert!(
                self.is_divisible_by(interest, self.parameters.interest_interval),
                "Chosen interest rate not permitted."
            );

            assert!(
                (Decimal::ZERO <= interest && interest < self.parameters.max_interest) || interest == dec!(-420),
                "Chosen interest not within allowed range."
            );

            let mut receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            let redemption_opt_out = match interest {
                interest if interest == dec!(-420) => {
                    if let Some(borrower) = borrower {
                        let privileged_data: PrivilegedBorrowerData = self
                            .privileged_borrower_manager
                            .get_non_fungible_data(&borrower);
                        self.link_cdp_to_privileged_borrower(borrower, cdp_id.clone(), true);
                        assert!(
                            privileged_data.redemption_opt_out,
                            "Not privileged for redemption opt-out"
                        );
                        true
                    } else {
                        panic!("No privileged borrower for redemption opt-out");
                    }
                }
                _ => {
                    if let Some(ref borrower) = receipt_data.privileged_borrower {
                        self.unlink_cdp_from_privileged_borrower(borrower.clone(), cdp_id.clone());
                    }

                    false
                }
            };

            if let Some(price) = with_price {
                self.change_collateral_price(receipt_data.collateral_address, price);
            }

            let fusd_debt: Decimal = self.pool_to_real_debt(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.pool_debt,
            );

            let mut pool_debt =
                self.real_to_pool_debt(receipt_data.collateral_address, interest, fusd_debt);

            if Clock::current_time_is_strictly_before(
                receipt_data
                    .last_interest_change
                    .add_days(self.parameters.feeless_interest_rate_change_cooldown as i64)
                    .unwrap(),
                TimePrecision::Second,
            ) && !redemption_opt_out
            {
                let (_real_extra_debt, pool_extra_debt) = self.extra_interest_days_fee(
                    receipt_data.collateral_address,
                    interest,
                    pool_debt,
                    self.parameters.days_of_extra_interest_fee,
                );

                pool_debt += pool_extra_debt;

                let extra_fusd =
                    self.mint_fusd(receipt_data.collateral_address, interest, pool_extra_debt);

                self.add_debt_to_collateral(receipt_data.collateral_address, extra_fusd.amount());

                self.collaterals
                    .get_mut(&receipt_data.collateral_address)
                    .unwrap()
                    .uncharged_interest
                    .put(extra_fusd);
            }

            assert!(
                receipt_data.status == CdpStatus::Healthy,
                "Loan not healthy."
            );

            let collateral: Bucket = self.take_collateral(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_amount,
            );

            self.put_collateral(receipt_data.collateral_address, collateral, interest);

            self.move_fusd_interest(
                receipt_data.collateral_address,
                receipt_data.interest,
                interest,
                fusd_debt,
            );

            self.remove_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_fusd_ratio,
                cdp_id.clone(),
            );

            let cr = self.get_and_check_cr(
                receipt_data.collateral_address,
                interest,
                receipt_data.collateral_amount,
                pool_debt,
                Some(cdp_id.clone()),
            );

            self.insert_cr(
                receipt_data.collateral_address,
                interest,
                cr,
                cdp_id.clone(),
            );

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "interest", interest);

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "collateral_fusd_ratio", cr);

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "pool_debt", pool_debt);

            self.cdp_manager.update_non_fungible_data(
                &cdp_id,
                "last_interest_change",
                Clock::current_time_rounded_to_seconds(),
            );

            self.clean_up_interest_info(receipt_data.interest, receipt_data.collateral_address);

            receipt_data.interest = interest;
            receipt_data.pool_debt = pool_debt;
            receipt_data.collateral_fusd_ratio = cr;

            Runtime::emit_event(EventUpdateCdp {
                cdp: receipt_data,
                cdp_id: cdp_id,
            });
        }

        /// Marks a CDP associated with a privileged borrower (who hasn't opted out of redemption) as irredeemable.
        ///
        /// This involves changing its interest rate to the lowest available standard rate for the collateral
        /// and borrowing a small fee (`irredeemable_tag_fee`) against the CDP.
        /// This effectively makes the CDP subject to normal redemption rules.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to tag.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the fUSD borrowed as the fee.
        ///
        /// # Panics
        /// * If the CDP status is not `Healthy`.
        /// * If the CDP's interest rate is not `-420` (i.e., not a privileged CDP).
        /// * If the CDP is linked to a privileged borrower who *has* opted out of redemption.
        pub fn tag_irredeemable(&mut self, cdp_id: NonFungibleLocalId) -> Bucket {
            let receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);
            assert!(
                receipt_data.status == CdpStatus::Healthy,
                "Loan not healthy"
            );
            assert!(receipt_data.interest == dec!(-420), "Loan not privileged");

            if let Some(privileged_borrower) = receipt_data.privileged_borrower {
                let privileged_data: PrivilegedBorrowerData = self
                    .privileged_borrower_manager
                    .get_non_fungible_data(&privileged_borrower);
                assert!(
                    !privileged_data.redemption_opt_out,
                    "Loan privileged to opt out of redemption"
                );
                self.unlink_cdp_from_privileged_borrower(privileged_borrower, cdp_id.clone());
            }

            // Change interest rate to the lowest standard rate for this collateral
            self.change_cdp_interest(
                cdp_id.clone(),
                self.get_lowest_interest(receipt_data.collateral_address),
                None,
                None,
            );

            // Borrow the fee amount. check_cr is false because we just set the interest rate correctly.
            self.borrow_more(cdp_id, self.parameters.irredeemable_tag_fee, false, false, None)
        }

        /// Attempts to change the status of a `Marked` CDP back to `Healthy`.
        ///
        /// This is typically called after adding collateral or if the collateral price increases,
        /// potentially making the CDP safe again.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the `Marked` CDP.
        /// * `with_price`: An optional `Decimal` to override the oracle price for this specific transaction.
        ///
        /// # Panics
        /// * If the CDP status is not `Marked`.
        /// * If, even after a potential price update, the CDP's CR is still below the MCR threshold.
        ///
        /// # Logic
        /// 1. **Fetch Data:** Retrieves the `Cdp` data.
        /// 2. **Validation:** Asserts the status is `Marked`.
        /// 3. **Price Update:** If `with_price` is provided, updates the collateral's stored price.
        /// 4. **Check CR:** Calls `get_and_check_cr`. This function calculates the current CR and asserts it's above the LCR.
        ///    Crucially, `get_and_check_cr` internally calls `unmark_if_marked` if the check passes, which removes the CDP
        ///    from the `marked_cdps` list and updates the CDP NFT status back to `Healthy`.
        pub fn unmark(&mut self, cdp_id: NonFungibleLocalId, with_price: Option<Decimal>) {
            let mut receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);
            assert!(receipt_data.status == CdpStatus::Marked, "Loan not marked");

            if let Some(price) = with_price {
                self.change_collateral_price(receipt_data.collateral_address, price);
            }

            // get_and_check_cr handles the actual unmarking logic via unmark_if_marked
            // if the CR is now sufficient.
            self.get_and_check_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_amount,
                receipt_data.pool_debt,
                Some(cdp_id.clone()), // Pass the ID so get_and_check_cr knows to potentially unmark
            );

            receipt_data.status = CdpStatus::Healthy;

            Runtime::emit_event(EventUpdateCdp {
                cdp: receipt_data,
                cdp_id: cdp_id,
            });
        }

        /// Performs a redemption operation, exchanging fUSD for collateral from the riskiest CDPs.
        ///
        /// Redemptions allow users to swap fUSD for collateral at face value (minus a fee),
        /// targeting the CDPs with the lowest collateral ratios first (starting from the lowest interest rate).
        /// This helps maintain the fUSD peg by creating demand for fUSD when it trades below peg.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral type to redeem against.
        /// * `payment`: A `Bucket` containing the fUSD to be redeemed.
        /// * `percentage_to_take`: An optional `Decimal` specifying the fraction of the redeemed value (in collateral terms)
        ///                         that the redeemer receives. If `None`, the dynamic redemption fee is calculated and applied.
        ///                         A value of `1.0` implies no fee (or 100% take for the redeemer).
        /// * `with_price`: An optional `Decimal` to override the oracle price for this specific transaction.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing:
        ///     1. The `Bucket` of collateral received by the redeemer.
        ///     2. The `Bucket` containing any leftover fUSD from the `payment` if not all was used (e.g., if all eligible CDPs were redeemed).
        ///
        /// # Panics
        /// * If `stop_redemption` parameter is true.
        /// * If the `payment` bucket does not contain the protocol's fUSD token.
        /// * If there are no redeemable CDPs (interest rate >= 0 or -420 without opt-out) for the specified `collateral_address`.
        /// * If a partial redemption is attempted on a CDP where CR <= 100% (must redeem fully in this case).
        ///
        /// # Logic
        /// 1. **Price Update & Validation:** Updates price if `with_price` provided. Asserts redemptions are allowed and payment is fUSD.
        /// 2. **Find Target CDP:**
        ///    - Identifies the lowest interest rate tier (`start_interest`) containing redeemable CDPs (interest >= 0, or -420 if not opted out).
        ///    - Finds the CDP (`cdp_id`) within that tier that has the lowest collateral ratio (first entry in the `collateral_ratios` tree).
        ///    - If no redeemable CDP exists, returns empty collateral and the original payment.
        /// 3. **Fetch & Prep CDP:** Retrieves the `Cdp` data for the target `cdp_id`. Removes its old CR entry using `remove_cr`.
        /// 4. **Calculate Ratios & Fees:**
        ///    - Gets current Collateral Ratio (CR) and Liquidation CR (LCR).
        ///    - Calculates `cr_percentage` (CR relative to MCR, essentially `CR / LCR * MCR`).
        ///    - Determines the amount of `pool_debt` to remove based on the `payment` amount (`pool_amount_to_remove`).
        /// 5. **Determine Redemption Scope:**
        ///    - Calculates the `percentage_to_redeem` of the target CDP's debt.
        ///    - Calculates the actual `payment_amount` (real fUSD) to take from the input `payment` bucket.
        ///    - Calculates the `new_pool_debt` remaining in the CDP after redemption.
        ///    - If the input `payment` covers more than the CDP's debt, sets `percentage_to_redeem` to 1, `payment_amount` to the CDP's full real debt, and `new_pool_debt` to 0.
        /// 6. **Calculate Dynamic Fee (if applicable):**
        ///    - If `percentage_to_take` was `None`:
        ///        - Calculates the decay factor based on time since `last_redemption`.
        ///        - Calculates the spike factor based on the redeemed amount relative to `circulating_fusd`.
        ///        - Updates `redemption_base_rate`.
        ///        - Calculates the effective fee based on the updated base rate, min/max fee parameters.
        ///        - Sets `percentage_to_take` to `1.0 - fee`.
        /// 7. **Calculate Collateral Payout:** Determines the `new_collateral_amount` remaining in the CDP after the redeemer takes their share.
        ///    `collateral_to_take = collateral_amount * percentage_to_redeem * percentage_to_take / cr_percentage`
        ///    `new_collateral_amount = collateral_amount - collateral_to_take` (clamped at zero).
        /// 8. **Validate Full Redemption (if CR <= 100%):** Asserts that if `cr_percentage <= 1`, the entire CDP must be redeemed (`percentage_to_redeem == 1`).
        /// 9. **Burn fUSD & Update State:** Calls `unmint_fusd` to burn the `payment_amount` and update debt totals.
        /// 10. **Retrieve & Distribute Collateral:** Takes all collateral from the CDP's vault.
        /// 11. **Update CDP NFT (Partial/Full):** Updates the CDP's `collateral_amount` and `pool_debt`.
        /// 12. **Handle Partial Redemption:**
        ///     - If `percentage_to_redeem < 1` (CDP remains open):
        ///         - Puts the `new_collateral_amount` back into the CDP's vault.
        ///         - Calculates the `new_cr`.
        ///         - Inserts the `new_cr` back into the CR tree.
        ///         - Updates the CDP NFT's `collateral_fusd_ratio`.
        ///         - Emits `EventRedeemCdp`.
        /// 13. **Handle Full Redemption:**
        ///     - If `percentage_to_redeem == 1` (CDP is closed):
        ///         - Puts the `new_collateral_amount` (which might be > 0 due to `percentage_to_take` < 1) into the `leftovers` vault.
        ///         - Updates the CDP NFT's `status` to `Redeemed`.
        ///         - Emits `EventRedeemCdp`
        /// 14. **Cleanup & Return:** Calls `clean_up_interest_info`. Returns the collateral bucket for the redeemer and the leftover fUSD payment bucket.
        pub fn redemption(
            &mut self,
            collateral_address: ResourceAddress,
            mut payment: Bucket,
            mut percentage_to_take: Option<Decimal>,
            with_price: Option<Decimal>,
        ) -> (Bucket, Bucket) {
            if let Some(price) = with_price {
                self.change_collateral_price(collateral_address, price);
            }

            assert!(
                !self.parameters.stop_redemption,
                "Not allowed to redeem loans right now."
            );

            assert!(
                payment.resource_address() == self.fusd_manager.address(),
                "Invalid fUSD payment."
            );

            let mut start_interest = Decimal::ZERO;

            if self
                .collaterals
                .get(&collateral_address)
                .unwrap()
                .interests
                .range(Decimal::ZERO..)
                .next()
                .is_none()
            {
                if self
                    .collaterals
                    .get(&collateral_address)
                    .unwrap()
                    .interests
                    .range(dec!(-420)..)
                    .next()
                    .is_none()
                {
                    return (Bucket::new(collateral_address), payment);
                } else {
                    start_interest = dec!(-420);
                }
            }

            let cdp_id = {
                let mut collateral = self.collaterals.get_mut(&collateral_address).unwrap();

                let (first_interest, _, _) =
                    collateral.interests.range(start_interest..).next().unwrap();

                let collateral_ratios = collateral
                    .ratios_by_interest
                    .get_mut(&first_interest)
                    .unwrap();

                let (_, cdp_ids, _) = collateral_ratios.range(Decimal::ZERO..).next().unwrap();

                cdp_ids[0].clone()
            };

            let mut receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            self.remove_cr(
                collateral_address,
                receipt_data.interest,
                receipt_data.collateral_fusd_ratio,
                cdp_id.clone(),
            );

            let cr: Decimal = self.get_cr(receipt_data.collateral_amount, receipt_data.pool_debt);
            let lcr: Decimal = self.get_lcr(collateral_address, receipt_data.interest);

            let mcr = self.collaterals.get(&collateral_address).unwrap().mcr;
            let cr_percentage: Decimal = mcr * cr / lcr;

            let pool_amount_to_remove =
                self.real_to_pool_debt(collateral_address, receipt_data.interest, payment.amount());

            let (percentage_to_redeem, payment_amount, new_pool_debt) =
                if pool_amount_to_remove > receipt_data.pool_debt {
                    (
                        Decimal::ONE,
                        self.pool_to_real_debt(
                            collateral_address,
                            receipt_data.interest,
                            receipt_data.pool_debt,
                        ),
                        Decimal::ZERO,
                    )
                } else {
                    (
                        (pool_amount_to_remove / receipt_data.pool_debt),
                        payment.amount(),
                        receipt_data.pool_debt - pool_amount_to_remove,
                    )
                };

            if percentage_to_take.is_none() {
                let protocol_redeemed_fraction = payment_amount / self.circulating_fusd;
                let time_since_last_redemption = Clock::current_time_rounded_to_seconds()
                    .seconds_since_unix_epoch
                    - self.last_redemption.seconds_since_unix_epoch;
                let current_base_rate = self.redemption_base_rate
                    * self
                        .parameters
                        .redemption_halflife_k
                        .checked_powi(time_since_last_redemption)
                        .unwrap();
                let new_base_rate = current_base_rate
                    + self.parameters.redemption_spike_k * protocol_redeemed_fraction;
                let new_base_rate_to_use = current_base_rate
                    + dec!("0.5") * (self.parameters.redemption_spike_k * protocol_redeemed_fraction);
                self.redemption_base_rate = new_base_rate;

                percentage_to_take = Some(
                    Decimal::ONE
                        - self
                            .parameters
                            .maximum_redemption_fee
                            .min(new_base_rate_to_use + self.parameters.minimum_redemption_fee),
                );
            }

            let new_collateral_amount = (receipt_data.collateral_amount
                - (receipt_data.collateral_amount
                    * percentage_to_redeem
                    * percentage_to_take.unwrap()
                    / cr_percentage))
                .max(Decimal::ZERO);

            assert!(
                cr_percentage > Decimal::ONE || percentage_to_redeem == Decimal::ONE,
                "CR < 100%. Entire loan must be liquidated",
            );

            self.unmint_fusd(
                collateral_address,
                receipt_data.interest,
                payment.take(payment_amount),
            );

            self.remove_debt_from_collateral(collateral_address, payment_amount);

            let mut collateral_payment = self.take_collateral(
                collateral_address,
                receipt_data.interest,
                receipt_data.collateral_amount,
            );

            self.cdp_manager.update_non_fungible_data(
                &cdp_id,
                "collateral_amount",
                new_collateral_amount,
            );
            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "pool_debt", new_pool_debt);

            if percentage_to_redeem < Decimal::ONE {
                self.put_collateral(
                    collateral_address,
                    collateral_payment.take(new_collateral_amount),
                    receipt_data.interest,
                );

                let new_cr = self.get_cr(new_collateral_amount, new_pool_debt);
                self.insert_cr(
                    collateral_address,
                    receipt_data.interest,
                    new_cr,
                    cdp_id.clone(),
                );

                self.cdp_manager
                    .update_non_fungible_data(&cdp_id, "collateral_fusd_ratio", new_cr);

                receipt_data.collateral_fusd_ratio = new_cr;
                receipt_data.collateral_amount = new_collateral_amount;
                receipt_data.pool_debt = new_pool_debt;

                self.clean_up_interest_info(receipt_data.interest, receipt_data.collateral_address);

                Runtime::emit_event(EventRedeemCdp {
                    cdp: receipt_data,
                    cdp_id: cdp_id,
                    fully_redeemed: false,
                });
            } else {
                self.put_collateral_in_leftovers(
                    collateral_address,
                    collateral_payment.take(new_collateral_amount),
                );

                self.cdp_manager
                    .update_non_fungible_data(&cdp_id, "status", CdpStatus::Redeemed);

                if let Some(ref borrower) = receipt_data.privileged_borrower {
                    self.unlink_cdp_from_privileged_borrower(borrower.clone(), cdp_id.clone());
                }

                self.clean_up_interest_info(receipt_data.interest, receipt_data.collateral_address);

                receipt_data.collateral_amount = new_collateral_amount;
                Runtime::emit_event(EventRedeemCdp {
                    cdp: receipt_data,
                    cdp_id: cdp_id,
                    fully_redeemed: true,
                });
            }

            self.last_redemption = Clock::current_time_rounded_to_seconds();

            (collateral_payment, payment)
        }

        /// Performs multiple redemption operations against a single collateral type in sequence.
        ///
        /// This repeatedly calls the `redemption` method until either the payment bucket is empty
        /// or the maximum number of redemptions is reached.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral type to redeem against.
        /// * `payment`: A `Bucket` containing the fUSD to be redeemed.
        /// * `percentage_to_take`: An optional `Decimal` specifying the fraction for the redeemer (passed to each `redemption` call).
        ///                         If `None`, dynamic fees are calculated in each `redemption` call.
        /// * `max_redemptions`: The maximum number of individual `redemption` calls to perform.
        /// * `with_price`: An optional `Decimal` to override the oracle price for this specific transaction (passed to the first `redemption` call).
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing:
        ///     1. A `Bucket` aggregating all collateral received from the individual redemptions.
        ///     2. The `Bucket` containing any remaining fUSD from the initial `payment` bucket.
        pub fn batch_redemption(
            &mut self,
            collateral_address: ResourceAddress,
            mut payment: Bucket,
            percentage_to_take: Option<Decimal>,
            max_redemptions: u64,
            with_price: Option<Decimal>,
        ) -> (Bucket, Bucket) {
            if let Some(price) = with_price {
                self.change_collateral_price(collateral_address, price);
            }

            let mut total_payout = Bucket::new(collateral_address);

            for _ in 0..max_redemptions {
                if payment.is_empty() {
                    break;
                }

                // Take the full remaining payment for the next redemption attempt.
                // The redemption method itself will only take what it needs.
                let current_payment = payment.take(payment.amount());
                let (individual_payout, remaining_payment) = self.redemption(
                    collateral_address,
                    current_payment,
                    percentage_to_take,
                    None, // Price only needs to be set once at the start
                );

                payment.put(remaining_payment);
                total_payout.put(individual_payout);
            }

            (total_payout, payment)
        }

        /// Calculates an optimal distribution of redemptions across multiple collateral types
        /// to achieve a target redemption amount or utilize a maximum number of redemption steps.
        ///
        /// This method simulates redemptions iteratively, always picking the collateral type
        /// that is currently furthest below its target redemption amount (proportionally) until
        /// the `max_redemptions` limit is hit. It then scales down the results to ensure no
        /// collateral type exceeds its proportional target based on the *least* fulfilled target.
        ///
        /// # Arguments
        /// * `collaterals`: A `Vec` of tuples `(ResourceAddress, Decimal, Option<Decimal>)`:
        ///     - `ResourceAddress`: The collateral to consider.
        ///     - `Decimal`: The target fUSD amount to redeem against this collateral.
        ///     - `Option<Decimal>`: An optional price override for this collateral during calculation.
        /// * `max_redemptions`: The maximum total number of individual redemption steps allowed across all collaterals.
        ///
        /// # Returns
        /// * `(Vec<(ResourceAddress, u64, Decimal)>, Decimal)`: A tuple containing:
        ///     - A `Vec` representing the optimized plan. Each tuple contains:
        ///         - `ResourceAddress`: The collateral address.
        ///         - `u64`: The calculated number of redemptions to perform against this collateral.
        ///         - `Decimal`: The calculated total fUSD amount to be redeemed against this collateral according to the plan.
        ///     - `Decimal`: The total fUSD amount used across all collaterals in the final plan.
        ///
        /// # Note
        /// This method only *calculates* the route; it does not perform any actual redemptions.
        /// The result should be used as input for the `optimal_batch_redemption` method.
        /// The commented-out alternative implementation explores a different approach but was deemed potentially less efficient.
        pub fn get_optimal_redemption_route(
            &mut self,
            collaterals: Vec<(ResourceAddress, Decimal, Option<Decimal>)>,
            max_redemptions: u64,
        ) -> (Vec<(ResourceAddress, u64, Decimal)>, Decimal) {
            // Pre-allocate with exact capacity
            let mut redemption_info = Vec::with_capacity(collaterals.len());
            let mut total_fusd_used = Decimal::ZERO;
            let mut remaining_redemptions = max_redemptions;

            // Store all redemptions in a single Vec to minimize allocations
            let mut all_redemptions = Vec::with_capacity(max_redemptions as usize * 2);
            let mut states = Vec::with_capacity(collaterals.len());

            // Initialize state
            for (address, target, price) in collaterals {
                if let Some(p) = price {
                    self.change_collateral_price(address, p);
                }

                let redemptions_start_idx = all_redemptions.len();
                if let Some(redemptions) = self.get_next_redemptions(address, max_redemptions) {
                    all_redemptions.extend(redemptions);
                }

                states.push(CollateralState {
                    address,
                    target,
                    current_amount: Decimal::ZERO,
                    redemption_count: 0,
                    redemptions_start_idx,
                });
            }

            // Main redemption loop
            while remaining_redemptions > 0 {
                let mut lowest_idx = None;
                let mut lowest_fraction = Decimal::ONE;

                // Find lowest fraction with simple scan
                for (i, state) in states.iter().enumerate() {
                    let redemptions_end = if i + 1 < states.len() {
                        states[i + 1].redemptions_start_idx
                    } else {
                        all_redemptions.len()
                    };

                    // Check if we have more redemptions available
                    if state.redemptions_start_idx + (state.redemption_count as usize)
                        < redemptions_end
                        && state.current_amount < state.target
                    {
                        let fraction = state.current_amount / state.target;
                        if fraction < lowest_fraction {
                            lowest_fraction = fraction;
                            lowest_idx = Some(i);
                        }
                    }
                }

                // Add redemption to lowest fraction if found
                if let Some(i) = lowest_idx {
                    let state = &mut states[i];
                    let redemption_idx =
                        state.redemptions_start_idx + state.redemption_count as usize;
                    if all_redemptions[redemption_idx] + state.current_amount > state.target {
                        state.current_amount = state.target;
                    } else {
                        state.current_amount += all_redemptions[redemption_idx];
                    }
                    state.redemption_count += 1;
                    remaining_redemptions -= 1;
                } else {
                    break;
                }
            }

            // Find minimum fraction
            let min_fraction = states
                .iter()
                .filter(|state| state.target != Decimal::ZERO)
                .map(|state| state.current_amount / state.target)
                .min()
                .unwrap_or(Decimal::ONE);

            // Build final result
            for state in states {
                if state.target == Decimal::ZERO {
                    continue;
                }

                let target_amount = state.target * min_fraction;
                let final_amount = if state.current_amount > target_amount {
                    target_amount
                } else {
                    state.current_amount
                };

                redemption_info.push((state.address, state.redemption_count, final_amount));
                total_fusd_used += final_amount;
            }

            (redemption_info, total_fusd_used)
        }

        /// Executes a batch redemption operation based on a pre-calculated optimal route.
        ///
        /// This method takes the output from `get_optimal_redemption_route` and performs
        /// the actual `batch_redemption` calls for each collateral type specified in the route.
        /// It also calculates and applies the dynamic redemption fee based on the total fUSD used
        /// if no explicit `percentage_to_take` is provided.
        ///
        /// # Arguments
        /// * `payment`: A `Bucket` containing the fUSD to be used for redemptions.
        /// * `collaterals`: A `Vec` of tuples `(ResourceAddress, Decimal, Option<Decimal>)` defining the target amounts and potential price overrides for each collateral.
        ///                Used *only* to recalculate the optimal route internally.
        /// * `percentage_to_take`: An optional `Decimal` specifying the fraction for the redeemer. If `None`, the dynamic fee is calculated based on the total planned `fusd_used`.
        /// * `max_redemptions`: The maximum total number of individual redemption steps allowed across all collaterals.
        ///
        /// # Returns
        /// * `(Vec<(ResourceAddress, Bucket)>, Bucket)`: A tuple containing:
        ///     - A `Vec` where each tuple holds the `ResourceAddress` of a redeemed collateral and a `Bucket` containing the payout for that collateral.
        ///     - The `Bucket` containing any remaining fUSD from the initial `payment` bucket.
        ///
        /// # Logic
        /// 1. **Calculate Route:** Calls `get_optimal_redemption_route` using the provided `collaterals` and `max_redemptions` to determine the plan (`optimal_route`) and total `fusd_used`.
        /// 2. **Calculate Dynamic Fee (if needed):**
        ///    - If `percentage_to_take` is `None`:
        ///        - Calculates the dynamic redemption fee based on `fusd_used` and time since last redemption, similar to the single `redemption` method.
        ///        - Sets `percentage_to_take` to `1.0 - fee`.
        /// 3. **Execute Batch Redemptions:**
        ///    - Iterates through the `optimal_route`.
        ///    - For each `(collateral, num_redemptions, redeemed_amount)`:
        ///        - Skips if `num_redemptions` or `redeemed_amount` is zero.
        ///        - Takes the minimum of `redeemed_amount` and the available `payment.amount()`.
        ///        - Calls `batch_redemption` for the specific `collateral` with the calculated payment portion, the (potentially dynamic) `percentage_to_take`, and `num_redemptions`.
        ///        - Puts any leftover fUSD from the `batch_redemption` call back into the main `payment` bucket.
        ///        - Stores the resulting collateral payout bucket.
        /// 4. **Return Results:** Returns the collected collateral payout buckets (one per collateral type) and the final remaining fUSD payment bucket.
        pub fn optimal_batch_redemption(
            &mut self,
            mut payment: Bucket,
            collaterals: Vec<(ResourceAddress, Decimal, Option<Decimal>)>,
            mut percentage_to_take: Option<Decimal>,
            max_redemptions: u64,
        ) -> (Vec<(ResourceAddress, Bucket)>, Bucket) {
            // Get the optimal route
            let (optimal_route, fusd_used) =
                self.get_optimal_redemption_route(collaterals, max_redemptions);

            if percentage_to_take.is_none() {
                let protocol_redeemed_fraction = fusd_used / self.circulating_fusd;
                let time_since_last_redemption = Clock::current_time_rounded_to_seconds()
                    .seconds_since_unix_epoch
                    - self.last_redemption.seconds_since_unix_epoch;
                let current_base_rate = self.redemption_base_rate
                    * self
                        .parameters
                        .redemption_halflife_k
                        .checked_powi(time_since_last_redemption)
                        .unwrap();
                let new_base_rate = current_base_rate
                    + self.parameters.redemption_spike_k * protocol_redeemed_fraction;
                let new_base_rate_to_use = current_base_rate
                    + dec!("0.5") * (self.parameters.redemption_spike_k * protocol_redeemed_fraction);
                self.redemption_base_rate = new_base_rate;

                percentage_to_take = Some(
                    Decimal::ONE
                        - self
                            .parameters
                            .maximum_redemption_fee
                            .min(new_base_rate_to_use + self.parameters.minimum_redemption_fee),
                );
            }

            // Prepare the vector for batch_redemptions
            let mut batch_inputs = Vec::with_capacity(optimal_route.len());
            for (collateral, num_redemptions, redeemed_amount) in &optimal_route {
                if *num_redemptions == 0 || *redeemed_amount == Decimal::ZERO {
                    continue;
                }
                // Take the amount for this collateral from the payment bucket
                let amount_for_collateral = if payment.amount() >= *redeemed_amount {
                    *redeemed_amount
                } else {
                    payment.amount()
                };
                let collateral_payment = payment.take(amount_for_collateral);
                batch_inputs.push((
                    *collateral,
                    collateral_payment,
                    percentage_to_take,
                    *num_redemptions,
                    None,
                ));
            }

            let results = self.batch_redemptions(batch_inputs);
            let mut output = Vec::with_capacity(results.len());
            for (collateral, payout, leftover_fusd) in results {
                payment.put(leftover_fusd);
                output.push((collateral, payout));
            }

            (output, payment)
        }

        /// Performs multiple batch redemptions across different collateral types in a single call.
        ///
        /// # Arguments
        /// * `redemptions`: A vector of tuples, each containing:
        ///     - `ResourceAddress`: The collateral address.
        ///     - `Bucket`: The payment bucket for this collateral.
        ///     - `Option<Decimal>`: The percentage to take (fee logic).
        ///     - `u64`: The max number of redemptions for this collateral.
        ///     - `Option<Decimal>`: Optional price override.
        ///
        /// # Returns
        /// * `Vec<(ResourceAddress, Bucket, Bucket)>`: For each collateral, a tuple of (collateral_address, payout_bucket, leftover_fusd_bucket)
        pub fn batch_redemptions(
            &mut self,
            redemptions: Vec<(
                ResourceAddress,
                Bucket,
                Option<Decimal>,
                u64,
                Option<Decimal>,
            )>,
        ) -> Vec<(ResourceAddress, Bucket, Bucket)> {
            let mut results = Vec::with_capacity(redemptions.len());
            for (collateral_address, payment, percentage_to_take, max_redemptions, with_price) in redemptions {
                let (payout, leftover_fusd) = self.batch_redemption(
                    collateral_address,
                    payment,
                    percentage_to_take,
                    max_redemptions,
                    with_price,
                );
                results.push((collateral_address, payout, leftover_fusd));
            }
            results
        }

        /// Liquidate a loan / CDP
        ///
        /// # Input
        /// - `payment`: The fUSD tokens to pay back
        /// - `cdp_id`: The NonFungibleLocalId of the to be liquidated loan / CDP
        ///
        /// # Output, depends on outcome:
        /// 1: liquidation successful
        /// - The fine (bonus) paid out in collateral
        /// - The amount of collateral to cover the paid back fUSD
        /// - The leftover payment fUSD
        ///
        /// # Logic
        /// - Get the CDP to be liquidated:
        /// - Liquidate
        pub fn liquidate_cdp(
            &mut self,
            mut payment: Bucket,
            cdp_id: NonFungibleLocalId,
            with_price: Option<Decimal>,
        ) -> (Bucket, Decimal, Bucket) {
            assert!(
                payment.resource_address() == self.fusd_manager.address(),
                "Invalid fUSD payment."
            );

            let receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            if let Some(price) = with_price {
                self.change_collateral_price(receipt_data.collateral_address, price);
            }

            assert!(
                !self.parameters.stop_liquidations,
                "Not allowed to liquidate loans right now."
            );

            assert!(
                receipt_data.status == CdpStatus::Healthy
                    || receipt_data.status == CdpStatus::Marked,
                "Loan not healthy"
            );

            if receipt_data.status == CdpStatus::Marked {
                assert!(
                    Clock::current_time_is_strictly_after(
                        *self
                            .collaterals
                            .get_mut(&receipt_data.collateral_address)
                            .unwrap()
                            .marked_cdps
                            .get(&cdp_id)
                            .unwrap(),
                        TimePrecision::Second,
                    ),
                    "Liquidation notice not yet reached"
                );
            }

            let cr: Decimal = self.get_cr(receipt_data.collateral_amount, receipt_data.pool_debt);
            let lcr: Decimal = self.get_lcr(receipt_data.collateral_address, receipt_data.interest);
            let mcr: Decimal = self
                .collaterals
                .get(&receipt_data.collateral_address)
                .unwrap()
                .mcr;
            let real_debt: Decimal = self.pool_to_real_debt(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.pool_debt,
            );

            assert!(cr < lcr, "Cannot liquidate, CR not under MCR");

            if let Some(ref borrower) = receipt_data.privileged_borrower {
                if receipt_data.status == CdpStatus::Healthy {
                    let privileged_data: PrivilegedBorrowerData = self
                        .privileged_borrower_manager
                        .get_non_fungible_data(borrower);
                    if privileged_data.liquidation_notice.is_some() {
                        let liquidation_notice = privileged_data.liquidation_notice.unwrap();
                        self.cdp_manager
                            .update_non_fungible_data(&cdp_id, "status", CdpStatus::Marked);
                        self.collaterals
                            .get_mut(&receipt_data.collateral_address)
                            .unwrap()
                            .marked_cdps
                            .insert(
                                cdp_id.clone(),
                                Clock::current_time_rounded_to_seconds()
                                    .add_minutes(liquidation_notice)
                                    .unwrap(),
                            );
                        payment.put(self.borrow_more(
                            cdp_id.clone(),
                            self.parameters.liquidation_notice_fee,
                            false,
                            false,
                            None,
                        ));

                        Runtime::emit_event(EventMarkCdp {
                            cdp_id: cdp_id,
                        });

                        return (
                            Bucket::new(receipt_data.collateral_address),
                            Decimal::ZERO,
                            payment,
                        );
                    }
                } else {
                    self.unlink_cdp_from_privileged_borrower(borrower.clone(), cdp_id.clone());
                }
            }

            if receipt_data.status == CdpStatus::Marked {
                let liquidation_time = *self
                    .collaterals
                    .get_mut(&receipt_data.collateral_address)
                    .unwrap()
                    .marked_cdps
                    .get(&cdp_id)
                    .unwrap();
                assert!(
                    Clock::current_time_is_strictly_after(liquidation_time, TimePrecision::Second),
                    "Liquidation notice not yet reached"
                );
            }

            assert!(
                real_debt <= payment.amount(),
                "Not enough fUSD to liquidate."
            );

            self.remove_cr(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_fusd_ratio,
                cdp_id.clone(),
            );

            let repayment: Bucket = payment.take(real_debt);

            self.unmint_fusd(
                receipt_data.collateral_address,
                receipt_data.interest,
                repayment,
            );

            self.remove_debt_from_collateral(receipt_data.collateral_address, real_debt);

            let cr_percentage: Decimal = mcr * cr / lcr;
            let collateral_equal_to_debt = receipt_data.collateral_amount / cr_percentage;
            let max_profit = self.parameters.liquidation_fine * collateral_equal_to_debt;

            let mut collateral = self.take_collateral(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.collateral_amount,
            );

            let payout = collateral.take(
                receipt_data
                    .collateral_amount
                    .min(max_profit + collateral_equal_to_debt),
            );
            let leftover_collateral: Decimal = collateral.amount();
            self.put_collateral_in_leftovers(receipt_data.collateral_address, collateral);

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "status", CdpStatus::Liquidated);

            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "pool_debt", Decimal::ZERO);

            self.cdp_manager.update_non_fungible_data(
                &cdp_id,
                "collateral_amount",
                leftover_collateral,
            );

            Runtime::emit_event(EventLiquidateCdp { cdp_id });

            self.clean_up_interest_info(receipt_data.interest, receipt_data.collateral_address);

            (payout, collateral_equal_to_debt, payment)
        }

        /// Checks if a CDP is currently eligible for liquidation.
        ///
        /// This method verifies several conditions:
        /// - Liquidations must be globally enabled (`!stop_liquidations`).
        /// - The CDP status must be `Healthy` or `Marked`.
        /// - If `Marked`, the liquidation notice period must have expired.
        /// - The CDP's Collateral Ratio (CR) must be below its Liquidation CR (LCR), calculated using the `with_price`.
        /// - If the CDP is `Healthy` and linked to a privileged borrower with a `liquidation_notice`, liquidation is not allowed (it needs to be marked first).
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to check.
        /// * `with_price`: An optional `Decimal` price override for the collateral to use in CR calculation.
        ///               If `None`, the currently stored price is used.
        ///
        /// # Returns
        /// * `(bool, Decimal, ResourceAddress)`: A tuple containing:
        ///     - `bool`: `true` if the CDP can be liquidated right now, `false` otherwise.
        ///     - `Decimal`: The current real fUSD debt of the CDP.
        ///     - `ResourceAddress`: The resource address of the CDP's collateral.
        ///
        /// # Panics
        /// * If `cdp_id` is invalid.
        /// * If the CDP status is not `Healthy` or `Marked`.
        /// * If `with_price` is `None` and the collateral address is invalid (should not happen if `cdp_id` is valid).
        pub fn check_liquidate_cdp(
            &mut self,
            cdp_id: NonFungibleLocalId,
            with_price: Option<Decimal>,
        ) -> (bool, Decimal, ResourceAddress) {
            let mut liquidation_allowed = true;
            if self.parameters.stop_liquidations {
                liquidation_allowed = false;
            }

            let receipt_data: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);

            assert!(
                receipt_data.status == CdpStatus::Healthy
                    || receipt_data.status == CdpStatus::Marked,
                "Loan not healthy or marked."
            );

            if let Some(price) = with_price {
                self.change_collateral_price(receipt_data.collateral_address, price);
            }

            let cr: Decimal = self.get_cr(receipt_data.collateral_amount, receipt_data.pool_debt);
            let lcr: Decimal = self.get_lcr(receipt_data.collateral_address, receipt_data.interest);

            if cr >= lcr {
                liquidation_allowed = false;
            }

            if receipt_data.status == CdpStatus::Marked {
                if !Clock::current_time_is_strictly_after(
                    *self
                        .collaterals
                        .get_mut(&receipt_data.collateral_address)
                        .unwrap()
                        .marked_cdps
                        .get(&cdp_id)
                        .unwrap(),
                    TimePrecision::Second,
                ) {
                    liquidation_allowed = false;
                }
            }

            let real_debt: Decimal = self.pool_to_real_debt(
                receipt_data.collateral_address,
                receipt_data.interest,
                receipt_data.pool_debt,
            );

            if receipt_data.privileged_borrower.is_some()
                && receipt_data.status == CdpStatus::Healthy
            {
                let privileged_data: PrivilegedBorrowerData = self
                    .privileged_borrower_manager
                    .get_non_fungible_data(&receipt_data.privileged_borrower.unwrap());
                if privileged_data.liquidation_notice.is_some() {
                    liquidation_allowed = false;
                }
            }

            (liquidation_allowed, real_debt, receipt_data.collateral_address)
        }

        /// Calculates and applies accrued interest for a range of interest rate tiers for a specific collateral.
        ///
        /// Iterates through `InterestInfo` entries within the `start` and `end` bounds.
        /// For each tier, it calculates the interest accrued since `last_interest_charge` based on the
        /// `real_debt` and the applicable interest rate (using `interest_for_irredeemables` for the -420 tier).
        ///
        /// The calculated interest (in real fUSD) is added to the tier's `real_debt` and the global
        /// `circulating_fusd`. The method mints the total calculated interest across all tiers.
        /// It also takes any previously collected upfront interest fees (`uncharged_interest` vault) and adds
        /// them to the minted bucket.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral to charge interest for.
        /// * `start`: Optional `Decimal` start of the interest rate range (inclusive). Defaults to -420.
        /// * `end`: Optional `Decimal` end of the interest rate range (exclusive). Defaults to `max_interest + interest_interval`.
        /// * `interest_for_irredeemables`: The `Decimal` interest rate to apply to CDPs in the -420 (privileged/irredeemable) tier.
        ///
        /// # Returns
        /// * `(Bucket, Decimal)`: A tuple containing:
        ///     - `Bucket`: A bucket containing the newly minted fUSD representing the accrued interest for the period,
        ///               plus any previously collected upfront fees.
        ///     - `Decimal`: The lowest standard interest rate (>= 0) found for this collateral.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        pub fn charge_interest(
            &mut self,
            collateral_address: ResourceAddress,
            start: Option<Decimal>,
            end: Option<Decimal>,
            interest_for_irredeemables: Decimal,
        ) -> (Bucket, Decimal) {
            let start_interest = start.unwrap_or(dec!(-420));
            let lowest_interest = self.get_lowest_interest(collateral_address);

            let end_interest =
                end.unwrap_or(self.parameters.max_interest + self.parameters.interest_interval);

            let mut fusd_to_mint = Decimal::ZERO;

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .range_mut(start_interest..end_interest)
                .for_each(
                    |(interest, interest_info, _next_interest): (
                        &Decimal,
                        &mut InterestInfo,
                        Option<Decimal>,
                    )| {
                        let interest_to_use = if *interest == dec!(-420) {
                            interest_for_irredeemables
                        } else {
                            *interest
                        };
                        let time_passed = Clock::current_time_rounded_to_seconds()
                            .seconds_since_unix_epoch
                            - interest_info.last_interest_charge;
                        interest_info.last_interest_charge =
                            Clock::current_time_rounded_to_seconds().seconds_since_unix_epoch;
                        let fusd_to_mint_for_interest = (Decimal::ONE
                            + (interest_to_use / dec!(31_556_926)))
                        .checked_powi(time_passed)
                        .unwrap()
                            * interest_info.real_debt
                            - interest_info.real_debt;
                        interest_info.real_debt += fusd_to_mint_for_interest;

                        fusd_to_mint += fusd_to_mint_for_interest;

                        scrypto_avltree::IterMutControl::Continue
                    },
                );

            self.circulating_fusd += fusd_to_mint;
            self.add_debt_to_collateral(collateral_address, fusd_to_mint);

            let mut minted_fusd = self.fusd_manager.mint(fusd_to_mint);
            minted_fusd.put(
                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .uncharged_interest
                    .take_all(),
            );

            Runtime::emit_event(EventChargeInterest {
                collateral_address: collateral_address,
                start: start,
                end: end,
                interest_for_irredeemables: interest_for_irredeemables,
                fusd_minted: fusd_to_mint,
                total_charged: minted_fusd.amount(),
            });

            (minted_fusd, lowest_interest)
        }

        /// Changes the price of a collateral, which will also update the liquidation collateral ratio
        pub fn change_collateral_price(&mut self, collateral: ResourceAddress, new_price: Decimal) {
            self.collaterals.get_mut(&collateral).unwrap().usd_price = new_price;
            Runtime::emit_event(EventChangeCollateral {
                address: collateral,
                new_mcr: None,
                new_usd_price: Some(new_price),
            });
        }

        /// Add a possible collateral to the protocol
        pub fn new_collateral(
            &mut self,
            address: ResourceAddress,
            chosen_mcr: Decimal,
            initial_price: Decimal,
        ) {
            assert!(
                self.collaterals.get(&address).is_none(),
                "Collateral is already accepted."
            );

            let info = CollateralInfo {
                mcr: chosen_mcr,
                usd_price: initial_price,
                vault: Vault::new(address),
                leftovers: Vault::new(address),
                uncharged_interest: Vault::new(self.fusd_manager.address()),
                resource_address: address,
                accepted: true,
                total_debt: Decimal::ZERO,
                collateral_amount: Decimal::ZERO,
                ratios_by_interest: KeyValueStore::new_with_registered_type(),
                interests: AvlTree::new(),
                marked_cdps: KeyValueStore::new_with_registered_type(),
            };

            self.collaterals.insert(address, info);

            Runtime::emit_event(EventAddCollateral {
                address,
                mcr: chosen_mcr,
                usd_price: initial_price,
            });
        }

        /// Mint a controller badge
        pub fn mint_controller_badge(&self, amount: Decimal) -> Bucket {
            self.controller_badge_manager.mint(amount)
        }

        /// Edit a collateral's parameters
        pub fn edit_collateral(
            &mut self,
            address: ResourceAddress,
            new_mcr: Decimal,
            new_acceptance: bool,
        ) {
            self.collaterals.get_mut(&address).unwrap().accepted = new_acceptance;
            self.collaterals.get_mut(&address).unwrap().mcr = new_mcr;

            Runtime::emit_event(EventChangeCollateral {
                address,
                new_mcr: Some(new_mcr),
                new_usd_price: None,
            });
        }

        /// Sets parameters related to the dynamic redemption fee calculation.
        ///
        /// # Arguments
        /// * `new_max_redemption_fee`: The maximum possible redemption fee.
        /// * `new_min_redemption_fee`: The base minimum redemption fee.
        /// * `new_redemption_spike_k`: The sensitivity factor for redemption volume affecting the fee.
        /// * `new_redemption_halflife_k`: The decay factor for the base rate over time (value < 1).
        pub fn set_redemption_parameters(
            &mut self,
            new_max_redemption_fee: Decimal,
            new_min_redemption_fee: Decimal,
            new_redemption_spike_k: Decimal,
            new_redemption_halflife_k: Decimal,
        ) {
            self.parameters.maximum_redemption_fee = new_max_redemption_fee;
            self.parameters.minimum_redemption_fee = new_min_redemption_fee;
            self.parameters.redemption_spike_k = new_redemption_spike_k;
            self.parameters.redemption_halflife_k = new_redemption_halflife_k;
        }

        /// Sets global flags to enable or disable major protocol operations.
        ///
        /// This allows pausing specific functions like opening new loans, closing existing ones,
        /// liquidations, or redemptions system-wide.
        ///
        /// # Arguments
        /// * `liquidations`: `true` to allow liquidations, `false` to disable.
        /// * `openings`: `true` to allow opening new CDPs, `false` to disable.
        /// * `closings`: `true` to allow closing CDPs (including partial close, collateral removal), `false` to disable.
        /// * `redemption`: `true` to allow redemptions, `false` to disable.
        pub fn set_stops(
            &mut self,
            liquidations: bool,
            openings: bool,
            closings: bool,
            redemption: bool,
        ) {
            self.parameters.stop_closings = closings;
            self.parameters.stop_liquidations = liquidations;
            self.parameters.stop_openings = openings;
            self.parameters.stop_redemption = redemption;
        }

        /// Set the maximum vector length for the collateral ratios (to prevent state explosion, vectors are non-lazily loaded)
        pub fn set_max_vector_length(&mut self, new_max_length: u64) {
            self.parameters.max_vector_length = new_max_length;
        }

        /// Set the minimum mintable amount of fUSD (to prevent unprofitable liquidations)
        pub fn set_minimum_mint(&mut self, new_minimum_mint: Decimal) {
            self.parameters.minimum_mint = new_minimum_mint;
        }

        /// Set fines
        /// * a liquidator fine of 0.05 and protocol fine of 0.03 would mean a liquidation would result in 1 + 0.05 + 0.03 = 1.08 times the minted fUSD's value collateral being taken from the borrower.
        /// * `irredeemable_tag_fee`: The fUSD fee charged when marking a privileged CDP (interest = -420)
        ///                          as subject to normal redemptions (by changing its interest rate).
        /// * `liquidation_notice_fee`: The fUSD fee charged when marking a CDP for liquidation due to a
        ///                           privileged borrower's liquidation notice period.
        pub fn set_fines(&mut self, liquidation_fine: Decimal, liquidation_notice_fee: Decimal, irredeemable_tag_fee: Decimal) {
            self.parameters.liquidation_fine = liquidation_fine;
            self.parameters.irredeemable_tag_fee = irredeemable_tag_fee;
            self.parameters.liquidation_notice_fee = liquidation_notice_fee;
        }

        /// Sets parameters related to interest rates and upfront interest fees charged under certain conditions.
        ///
        /// # Arguments
        /// * `max_interest`: The maximum allowed interest rate (Decimal).
        /// * `interest_interval`: The allowed interval for interest rates (Decimal).
        /// * `feeless_interest_rate_change_cooldown`: The minimum number of days (`u64`) required between interest rate changes to avoid an extra interest fee.
        /// * `days_of_extra_interest_fee`: The number of days (`u64`) worth of interest charged upfront as a fee when applicable.
        pub fn set_interest_params(
            &mut self,
            max_interest: Decimal,
            interest_interval: Decimal,
            feeless_interest_rate_change_cooldown: u64,
            days_of_extra_interest_fee: u64,
        ) {
            self.parameters.max_interest = max_interest;
            self.parameters.interest_interval = interest_interval;
            self.parameters.feeless_interest_rate_change_cooldown = feeless_interest_rate_change_cooldown;
            self.parameters.days_of_extra_interest_fee = days_of_extra_interest_fee;
        }

        /// Mints a specified amount of fUSD without requiring collateral.
        ///
        /// This is a privileged operation intended for use by other protocol components
        /// that have minting authority (e.g., a Flash Loan component).
        /// It directly increases the fUSD supply but does not update internal CDP debt tracking.
        ///
        /// # Arguments
        /// * `amount`: The `Decimal` amount of fUSD to mint.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the newly minted fUSD.
        pub fn free_fusd(&mut self, amount: Decimal) -> Bucket {
            self.fusd_manager.mint(amount)
        }

        /// Burns a provided bucket of fUSD tokens.
        ///
        /// This is typically used by other protocol components (e.g., StabilityPools) that hold
        /// the burn authority for fUSD.
        ///
        /// # Arguments
        /// * `bucket`: A `Bucket` containing the fUSD tokens to be burned.
        ///
        /// # Panics
        /// * If the provided `bucket` does not contain fUSD managed by this component.
        pub fn burn_fusd(&mut self, bucket: Bucket) {
            assert!(
                bucket.resource_address() == self.fusd_manager.address(),
                "Can only burn fUSD, not another token."
            );
            bucket.burn();
        }

        /// Burns a CDP NFT (loan receipt) provided it meets the criteria for burning.
        ///
        /// A CDP NFT can only be burned if its associated loan is fully terminated
        /// (status is `Closed`, `Liquidated`, or `Redeemed`) AND all leftover collateral
        /// associated with it has been claimed (`collateral_amount` is zero).
        ///
        /// # Arguments
        /// * `receipt`: A `Bucket` containing the single CDP NFT (`fusdLOAN`) to be burned.
        ///
        /// # Panics
        /// * If the `receipt` bucket does not contain a CDP NFT managed by this component.
        /// * If the CDP NFT's status is not `Closed`, `Liquidated`, or `Redeemed`.
        /// * If the CDP NFT's `collateral_amount` is not zero (meaning leftover collateral hasn't been claimed).
        pub fn burn_loan_receipt(&self, receipt: Bucket) {
            let receipt_data: Cdp = receipt.as_non_fungible().non_fungible().data();
            assert!(
                self.cdp_manager.address() == receipt.resource_address(),
                "Can only burn loan receipts, not another token."
            );
            assert!(
                receipt_data.status == CdpStatus::Liquidated
                    || receipt_data.status == CdpStatus::Redeemed
                    || receipt_data.status == CdpStatus::Closed,
                "Loan not closed or liquidated"
            );
            assert!(
                receipt_data.collateral_amount == Decimal::ZERO,
                "Retrieve all collateral before burning!"
            );
            receipt.burn();
        }

        /// Mints a new Privileged Borrower NFT.
        ///
        /// These NFTs grant special permissions or features within the protocol,
        /// such as opting out of redemptions or receiving liquidation notices.
        ///
        /// # Arguments
        /// * `privileged_borrower`: The `PrivilegedBorrowerData` struct containing the configuration
        ///                        for the new privileged borrower.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the newly minted Privileged Borrower NFT.
        pub fn create_privileged_borrower(
            &mut self,
            mut privileged_borrower: PrivilegedBorrowerData,
        ) -> Bucket {
            privileged_borrower.key_image_url = Url::of("https://flux.ilikeitstable.com/flux-id.png");
            self.privileged_borrower_counter += 1;
            self.privileged_borrower_manager
                .mint_non_fungible(
                    &NonFungibleLocalId::integer(self.privileged_borrower_counter),
                    privileged_borrower,
                )
                .into()
        }

        /// Edits the data associated with an existing Privileged Borrower NFT.
        ///
        /// Allows updating fields like `redemption_opt_out`, `liquidation_notice`,
        /// and `max_coupled_loans` for a specific borrower identified by their ID.
        ///
        /// # Arguments
        /// * `privileged_borrower`: The `PrivilegedBorrowerData` struct containing the updated
        ///                        configuration for the borrower.
        /// * `borrower_id`: The `NonFungibleLocalId` of the Privileged Borrower NFT to modify.
        ///
        /// # Panics
        /// * If the `borrower_id` does not correspond to a Privileged Borrower NFT managed by this component.
        pub fn edit_privileged_borrower(
            &mut self,
            privileged_borrower: PrivilegedBorrowerData,
            borrower_id: NonFungibleLocalId,
        ) {
            self.privileged_borrower_manager.update_non_fungible_data(
                &borrower_id,
                "redemption_opt_out",
                privileged_borrower.redemption_opt_out,
            );
            self.privileged_borrower_manager.update_non_fungible_data(
                &borrower_id,
                "liquidation_notice",
                privileged_borrower.liquidation_notice,
            );
            self.privileged_borrower_manager.update_non_fungible_data(
                &borrower_id,
                "max_coupled_loans",
                privileged_borrower.max_coupled_loans,
            );
        }

        /// Links a specific CDP NFT to a Privileged Borrower NFT.
        ///
        /// This association allows the CDP to potentially inherit benefits or restrictions
        /// defined in the `PrivilegedBorrowerData`, such as redemption opt-out or
        /// liquidation notices (depending on other CDP parameters like interest rate).
        ///
        /// # Arguments
        /// * `privileged_borrower`: The `NonFungibleLocalId` of the Privileged Borrower NFT.
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP NFT to link.
        ///
        /// # Panics
        /// * If adding this CDP exceeds the `max_coupled_loans` limit set in the `PrivilegedBorrowerData`.
        /// * If the `privileged_borrower` ID is invalid.
        /// * If the `cdp_id` is invalid.
        pub fn link_cdp_to_privileged_borrower(
            &mut self,
            privileged_borrower: NonFungibleLocalId,
            cdp_id: NonFungibleLocalId,
            link_cdp_id: bool,
        ) {
            let mut privileged_data: PrivilegedBorrowerData = self
                .privileged_borrower_manager
                .get_non_fungible_data(&privileged_borrower);
            privileged_data.coupled_loans.push(cdp_id.clone());
            assert!(
                privileged_data.coupled_loans.len() <= privileged_data.max_coupled_loans as usize,
                "Max coupled loans reached"
            );
            self.privileged_borrower_manager.update_non_fungible_data(
                &privileged_borrower,
                "coupled_loans",
                privileged_data.coupled_loans,
            );
            if link_cdp_id {
                self.cdp_manager.update_non_fungible_data(
                    &cdp_id,
                    "privileged_borrower",
                    Some(privileged_borrower),
                );
            }
        }

        /// Removes the link between a specific CDP NFT and a Privileged Borrower NFT.
        ///
        /// This removes the CDP ID from the borrower's `coupled_loans` list and sets
        /// the CDP's `privileged_borrower` field back to `None`.
        ///
        /// # Arguments
        /// * `privileged_borrower`: The `NonFungibleLocalId` of the Privileged Borrower NFT.
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP NFT to unlink.
        ///
        /// # Panics
        /// * If the `privileged_borrower` ID is invalid.
        /// * If the `cdp_id` is invalid.
        pub fn unlink_cdp_from_privileged_borrower(
            &mut self,
            privileged_borrower: NonFungibleLocalId,
            cdp_id: NonFungibleLocalId,
        ) {
            let mut privileged_data: PrivilegedBorrowerData = self
                .privileged_borrower_manager
                .get_non_fungible_data(&privileged_borrower);
            privileged_data.coupled_loans.retain(|id| *id != cdp_id);
            self.privileged_borrower_manager.update_non_fungible_data(
                &privileged_borrower,
                "coupled_loans",
                privileged_data.coupled_loans,
            );
            self.cdp_manager.update_non_fungible_data(
                &cdp_id,
                "privileged_borrower",
                None::<NonFungibleLocalId>,
            );
        }

        //GETTERS

        /// Retrieves the potential fUSD redemption amounts for the next `amount` riskiest CDPs
        /// for a given collateral type.
        ///
        /// This method simulates the redemption order without performing actual redemptions.
        /// It starts from the lowest interest rate (including -420 if applicable and not opted out)
        /// and proceeds through CDPs in ascending order of their collateral ratio within each rate.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral to query.
        /// * `amount`: The maximum number (`u64`) of potential redemption amounts to return.
        ///
        /// # Returns
        /// * `Option<Vec<Decimal>>`: 
        ///     - `Some(Vec<Decimal>)` containing the real fUSD debt amounts of the next `amount` redeemable CDPs,
        ///       ordered from lowest CR / lowest interest to highest.
        ///     - `None` if the collateral address is invalid or if there are no redeemable CDPs for that collateral.
        pub fn get_next_redemptions(
            &self,
            collateral_address: ResourceAddress,
            amount: u64,
        ) -> Option<Vec<Decimal>> {
            let mut redemptions: Vec<Decimal> = vec![];

            // Return None if collateral isn't found.
            let collateral = self.collaterals.get(&collateral_address)?;
            let mut start_interest = Decimal::ZERO;
            let open_irredeemable_loans = collateral.interests.get(&dec!(-420)).is_some();

            // If there is no interest data at all, return None.
            if collateral.interests.range(Decimal::ZERO..).next().is_none() {
                if open_irredeemable_loans {
                    start_interest = dec!(-420);
                } else {
                    return None;
                }
            }

            // Iterate over all interest rates in ascending order.
            for (interest, interest_info, next_interest) in
                collateral.interests.range(start_interest..)
            {
                if collateral.ratios_by_interest.get(&interest).is_none() {
                    if next_interest.is_some() {
                        continue;
                    } else {
                        break;
                    }
                }

                if collateral
                    .ratios_by_interest
                    .get(&interest)
                    .unwrap()
                    .range(Decimal::ZERO..)
                    .next()
                    .is_none()
                {
                    continue;
                }

                let debt_multiplier = interest_info.real_debt / interest_info.pool_debt;

                for (_cr, cdp_ids, next_cr) in collateral
                    .ratios_by_interest
                    .get(&interest)
                    .unwrap()
                    .range(Decimal::ZERO..)
                {
                    for cdp_id in cdp_ids {
                        let cdp = self.cdp_manager.get_non_fungible_data::<Cdp>(&cdp_id);
                        // Here we compute a redemption value. (Adjust the computation as needed.)
                        redemptions.push(cdp.pool_debt * debt_multiplier);
                        if redemptions.len() as u64 >= amount {
                            return Some(redemptions);
                        }
                    }
                    // If there is no next CR, break out of the inner loop.
                    if next_cr.is_none() {
                        break;
                    }
                }
                // If there is no next interest, break out of the outer loop.
                if next_interest.is_none() {
                    break;
                }
            }

            if start_interest != dec!(-420) && open_irredeemable_loans {
                let interest_info = collateral.interests.get(&dec!(-420)).unwrap();

                if collateral.ratios_by_interest.get(&dec!(-420)).is_none() {
                    return Some(redemptions);
                }

                if collateral
                    .ratios_by_interest
                    .get(&dec!(-420))
                    .unwrap()
                    .range(Decimal::ZERO..)
                    .next()
                    .is_none()
                {
                    return Some(redemptions);
                }

                let debt_multiplier = interest_info.real_debt / interest_info.pool_debt;

                for (_cr, cdp_ids, next_cr) in collateral
                    .ratios_by_interest
                    .get(&dec!(-420))
                    .unwrap()
                    .range(Decimal::ZERO..)
                {
                    for cdp_id in cdp_ids {
                        let cdp = self.cdp_manager.get_non_fungible_data::<Cdp>(&cdp_id);
                        // Here we compute a redemption value. (Adjust the computation as needed.)
                        redemptions.push(cdp.pool_debt * debt_multiplier);
                        if redemptions.len() as u64 >= amount {
                            return Some(redemptions);
                        }
                    }
                    // If there is no next CR, break out of the inner loop.
                    if next_cr.is_none() {
                        break;
                    }
                }
            }
            
            Some(redemptions)
        }

        /// Retrieves the IDs of the next `amount` CDPs eligible for liquidation for a given collateral type,
        /// based on a provided price and optional filtering criteria.
        ///
        /// This method checks CDPs within the specified interest and CR ranges.
        /// A CDP is considered eligible if:
        /// - Its calculated CR (using `with_price`) is below its Liquidation CR (LCR).
        /// - It's not a privileged CDP with an active, unexpired liquidation notice period.
        /// - If it *was* marked for liquidation, the notice period has expired.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral to query.
        /// * `amount`: The maximum number (`u64`) of CDP IDs to return.
        /// * `interest_start`: Optional `Decimal` start of the interest rate range to check (inclusive).
        /// * `interest_end`: Optional `Decimal` end of the interest rate range to check (exclusive).
        /// * `cr_start`: Optional `Decimal` start of the collateral ratio range to check (inclusive).
        /// * `with_price`: The `Decimal` price of the collateral to use for CR calculations.
        ///
        /// # Returns
        /// * `Option<Vec<NonFungibleLocalId>>`: 
        ///     - `Some(Vec<NonFungibleLocalId>)` containing the IDs of the next liquidatable CDPs,
        ///       ordered by interest rate, then by collateral ratio.
        ///     - `None` if the collateral address is invalid or no eligible CDPs are found within the criteria.
        pub fn get_next_liquidations(
            &mut self,
            collateral_address: ResourceAddress,
            amount: u64,
            interest_start: Option<Decimal>,
            interest_end: Option<Decimal>,
            cr_start: Option<Decimal>,
            with_price: Decimal,
        ) -> Option<Vec<NonFungibleLocalId>> {
            let mut liquidations: Vec<NonFungibleLocalId> = vec![];

            // Return None immediately if collateral isn't found.
            let collateral = self.collaterals.get(&collateral_address)?;
            let start = interest_start.unwrap_or(Decimal::ZERO);
            let end = interest_end
                .unwrap_or(self.parameters.max_interest + self.parameters.interest_interval);

            // If there is no interest data in the requested range, return None.
            if collateral.interests.range(start..end).next().is_none() {
                return None;
            }

            // Iterate over interest rates in the given range.
            for (interest, interest_info, next_interest) in collateral.interests.range(start..end) {
                // Use ? to return None if no CR tree is found for this interest.
                let collateral_ratios = collateral.ratios_by_interest.get(&interest)?;

                // Iterate over CR entries.
                for (cr, cdp_ids, next_cr) in
                    collateral_ratios.range(cr_start.unwrap_or(Decimal::ZERO)..)
                {
                    let lcr = collateral.mcr
                        * ((interest_info.real_debt / interest_info.pool_debt) / with_price);
                    if cr < lcr {
                        for cdp_id in cdp_ids {
                            if collateral.marked_cdps.get(&cdp_id).is_some() {
                                let liquidation_time =
                                    *collateral.marked_cdps.get(&cdp_id).unwrap();
                                if Clock::current_time_is_strictly_after(
                                    liquidation_time,
                                    TimePrecision::Second,
                                ) {
                                    liquidations.push(cdp_id.clone());
                                }
                            }
                            if liquidations.len() as u64 >= amount {
                                return Some(liquidations);
                            }
                        }
                    }
                    if next_cr.is_none() {
                        break;
                    }
                }
                if next_interest.is_none() {
                    break;
                }
            }

            Some(liquidations)
        }

        /// Retrieves the `Cdp` data and current debt multiplier for a list of specified CDP IDs.
        ///
        /// The debt multiplier represents the current ratio of real fUSD debt to pool debt
        /// for the CDP's specific collateral and interest rate, accounting for accrued interest.
        ///
        /// # Arguments
        /// * `cdp_ids`: A `Vec<NonFungibleLocalId>` containing the IDs of the CDPs to query.
        ///
        /// # Returns
        /// * `Vec<(NonFungibleLocalId, Cdp, Decimal)>`: A vector of tuples, where each tuple contains:
        ///     - The `NonFungibleLocalId` of the CDP.
        ///     - The corresponding `Cdp` struct containing its data.
        ///     - The calculated `Decimal` debt multiplier for that CDP.
        ///
        /// # Panics
        /// * If any `cdp_id` in the input vector is invalid.
        pub fn get_cdps_info(
            &self,
            cdp_ids: Vec<NonFungibleLocalId>,
        ) -> Vec<(NonFungibleLocalId, Cdp, Decimal)> {
            //decimal is debt multiplier (or real debt? think about it)
            let mut cdp_infos: Vec<(NonFungibleLocalId, Cdp, Decimal)> = vec![];
            for cdp_id in cdp_ids {
                let cdp_info: Cdp = self.cdp_manager.get_non_fungible_data(&cdp_id);
                let debt_multiplier: Decimal =
                    self.get_debt_multiplier(cdp_info.collateral_address, cdp_info.interest);
                cdp_infos.push((cdp_id, cdp_info, debt_multiplier));
            }

            cdp_infos
        }

        /// Retrieves the `PrivilegedBorrower` data for a list of specified Privileged Borrower IDs.
        ///
        /// # Arguments
        /// * `privileged_borrower_ids`: A `Vec<NonFungibleLocalId>` containing the IDs of the Privileged Borrowers to query.
        ///
        /// # Returns
        /// * `Vec<(NonFungibleLocalId, PrivilegedBorrowerData)>`: A vector of tuples, where each tuple contains:
        ///     - The `NonFungibleLocalId` of the Privileged Borrower.
        ///     - The corresponding `PrivilegedBorrowerData` struct containing its data.
        pub fn get_privileged_borrower_info(
            &self,
            privileged_borrower_ids: Vec<NonFungibleLocalId>,
        ) -> Vec<(NonFungibleLocalId, PrivilegedBorrowerData)> {
            let mut privileged_borrower_infos: Vec<(NonFungibleLocalId, PrivilegedBorrowerData)> = vec![];
            for privileged_borrower_id in privileged_borrower_ids {
                let privileged_borrower_info: PrivilegedBorrowerData = self.privileged_borrower_manager.get_non_fungible_data(&privileged_borrower_id);
                privileged_borrower_infos.push((privileged_borrower_id, privileged_borrower_info));
            }

            privileged_borrower_infos
        }

        /// Retrieves summary information for a list of specified collateral resource addresses.
        ///
        /// This method provides a snapshot of key metrics for each requested collateral type,
        /// excluding the detailed interest rate and CR tree data.
        ///
        /// # Arguments
        /// * `collateral_addresses`: A `Vec<ResourceAddress>` of the collateral types to query.
        ///
        /// # Returns
        /// * `Vec<CollateralInfoReturn>`: A vector containing a `CollateralInfoReturn` struct
        ///   for each valid and found collateral address in the input vector. Addresses not managed
        ///   by the component are silently skipped.
        pub fn get_collateral_infos(
            &self,
            collateral_addresses: Vec<ResourceAddress>,
        ) -> Vec<CollateralInfoReturn> {
            collateral_addresses
                .iter()
                .filter_map(|collateral_address| self.collaterals.get(collateral_address))
                .map(|collateral_info| CollateralInfoReturn {
                    collateral_amount: collateral_info.collateral_amount,
                    total_debt: collateral_info.total_debt,
                    resource_address: collateral_info.resource_address,
                    mcr: collateral_info.mcr,
                    usd_price: collateral_info.usd_price,
                    vault: collateral_info.vault.amount(),
                    leftovers: collateral_info.leftovers.amount(),
                    uncharged_interest: collateral_info.uncharged_interest.amount(),
                    accepted: collateral_info.accepted,
                })
                .collect()
        }

        /// Retrieves detailed information for interest rate tiers within a specified range
        /// for a given collateral type.
        ///
        /// Each `InterestInfo` struct contains the aggregated pool debt, real debt, collateral amount,
        /// number of CR entries, and last interest charge timestamp for a specific interest rate.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral to query.
        /// * `start_interest`: Optional `Decimal` start of the interest rate range (inclusive, defaults to 0).
        /// * `end_interest`: Optional `Decimal` end of the interest rate range (exclusive, defaults to max_interest + interval).
        ///
        /// # Returns
        /// * `Vec<InterestInfo>`: A vector containing `InterestInfo` structs for each interest rate
        ///   found within the specified range for the given collateral.
        ///
        /// # Panics
        /// * If the `collateral_address` is invalid.
        pub fn get_interest_infos(
            &self,
            collateral_address: ResourceAddress,
            start_interest: Option<Decimal>,
            end_interest: Option<Decimal>,
        ) -> Vec<InterestInfo> {
            let mut interest_infos: Vec<InterestInfo> = vec![];

            let start = start_interest.unwrap_or(dec!(-420));
            let end = end_interest
                .unwrap_or(self.parameters.max_interest + self.parameters.interest_interval);

                self.collaterals
                    .get(&collateral_address)
                .unwrap()
                .interests
                .range(start..end)
                .for_each(
                    |(_interest, interest_info, _next_interest): (
                        Decimal,
                        InterestInfo,
                        Option<Decimal>,
                    )| {
                        interest_infos.push(interest_info.clone());
                    },
                );

            interest_infos
        }

        /// Retrieves Collateral Ratio (CR) entries and associated CDP IDs within a specified range
        /// for a given collateral and interest rate.
        ///
        /// Each CR entry maps a specific CR value (collateral_amount / pool_debt) to a vector of
        /// CDP IDs currently at that ratio for the given interest rate.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral.
        /// * `interest`: The specific `Decimal` interest rate tier to query.
        /// * `cr_start`: Optional `Decimal` start of the CR range (inclusive, defaults to 0).
        /// * `cr_end`: Optional `Decimal` end of the CR range (exclusive, defaults to unbounded).
        ///
        /// # Returns
        /// * `Vec<(Decimal, Vec<NonFungibleLocalId>)>`: A vector of tuples, where each tuple contains:
        ///     - A `Decimal` CR value.
        ///     - A `Vec<NonFungibleLocalId>` of CDP IDs at that CR.
        ///   The vector is ordered by CR value.
        ///
        /// # Panics
        /// * If the `collateral_address` is invalid.
        /// * If the `interest` rate does not exist for the given collateral.
        /// * If the CR tree for the specified interest rate is unexpectedly missing.
        pub fn get_crs(
            &self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            cr_start: Option<Decimal>,
            cr_end: Option<Decimal>,
        ) -> Vec<(Decimal, Vec<NonFungibleLocalId>)> {
            let start = cr_start.unwrap_or(Decimal::ZERO);

            if let Some(end) = cr_end {
                self.collaterals
                    .get(&collateral_address)
                    .unwrap()
                    .ratios_by_interest
                    .get(&interest)
                    .unwrap()
                    .range(start..end)
                    .map(|(cr, local_ids, _)| (cr, local_ids))
                    .collect()
            } else {
                self.collaterals
                    .get(&collateral_address)
                    .unwrap()
                    .ratios_by_interest
                    .get(&interest)
                    .unwrap()
                    .range(start..)
                    .map(|(cr, local_ids, _)| (cr, local_ids))
                    .collect()
            }
        }

        /// Calculates the total real fUSD debt held in interest rate tiers strictly lower
        /// than the specified `interest` rate for a given collateral.
        ///
        /// This is useful for determining the amount of debt that would be redeemed before
        /// any debt at the target `interest` rate during a redemption operation.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral.
        /// * `interest`: The `Decimal` interest rate to serve as the upper bound (exclusive).
        ///
        /// # Returns
        /// * `Decimal`: The sum of `real_debt` from all `InterestInfo` structs with an interest rate
        ///   greater than or equal to 0 and less than the provided `interest`.
        ///
        /// # Panics
        /// * If the `collateral_address` is invalid.
        pub fn get_debt_in_front(
            &self,
            collateral_address: ResourceAddress,
            interest: Decimal,
        ) -> Decimal {
            let mut debt_in_front: Decimal = Decimal::ZERO;

            self.collaterals
                .get(&collateral_address)
                .unwrap()
                .interests
                .range(Decimal::ZERO..interest)
                .for_each(
                    |(_interest, interest_info, _next_interest): (
                        Decimal,
                        InterestInfo,
                        Option<Decimal>,
                    )| {
                        debt_in_front += interest_info.real_debt;
                    },
                );

            debt_in_front
        }

        /// Returns the total amount of fUSD currently in circulation.
        ///
        /// This reflects the sum of all fUSD minted across all CDPs and any fUSD minted
        /// via `free_fusd`, minus any fUSD burned.
        ///
        /// # Returns
        /// * `Decimal`: The total circulating supply of fUSD.
        pub fn get_total_debt(&self) -> Decimal {
            self.circulating_fusd
        }

        /// Finds the lowest standard interest rate (>= 0) currently active for a given collateral.
        ///
        /// Active means there is at least one CDP or some non-zero debt associated with that rate.
        /// If no standard interest rates are active, it returns 0.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the collateral.
        ///
        /// # Returns
        /// * `Decimal`: The lowest active standard interest rate, or `Decimal::ZERO` if none exist.
        ///
        /// # Panics
        /// * If the `collateral_address` is invalid.
        pub fn get_lowest_interest(&self, collateral_address: ResourceAddress) -> Decimal {
            if self
                .collaterals
                .get(&collateral_address)
                .unwrap()
                .interests
                .range(Decimal::ZERO..)
                .next()
                .is_none()
            {
                Decimal::ZERO
            } else {
                for (interest, interest_info, _next_interest) in self
                    .collaterals
                    .get(&collateral_address)
                    .unwrap()
                    .interests
                    .range(Decimal::ZERO..)
                {
                    if interest_info.real_debt >= Decimal::ZERO {
                        return interest;
                    }
                }
                Decimal::ZERO
            }
        }

        /// Retrieves the timestamp when a marked CDP becomes eligible for liquidation.
        ///
        /// This timestamp is set when a CDP is marked, usually due to a privileged borrower's
        /// liquidation notice period being triggered.
        ///
        /// # Arguments
        /// * `collateral_address`: The `ResourceAddress` of the CDP's collateral.
        /// * `cdp_id`: The `NonFungibleLocalId` of the marked CDP.
        ///
        /// # Returns
        /// * `Instant`: The timestamp (UTC) after which the CDP can be liquidated.
        ///
        /// # Panics
        /// * If the `collateral_address` is invalid.
        /// * If the `cdp_id` does not correspond to a CDP currently marked for liquidation
        ///   for the given collateral (i.e., not found in the `marked_cdps` store).
        pub fn get_marked_liquidation_date(
            &self,
            collateral_address: ResourceAddress,
            cdp_id: NonFungibleLocalId,
        ) -> Instant {
            *self
                .collaterals
                .get(&collateral_address)
                .unwrap()
                .marked_cdps
                .get(&cdp_id)
                .unwrap()
        }

        /// Returns the resource address of the fUSD token managed by this component.
        pub fn get_fusd_address(&self) -> ResourceAddress {
            self.fusd_manager.address()
        }

        //HELPER METHODS

        /// Removes a CDP from the marked list if it exists there and updates its status to Healthy.
        ///
        /// This is called internally when an operation (like adding collateral or a price increase)
        /// brings a previously marked CDP back above the liquidation threshold.
        ///
        /// # Arguments
        /// * `collateral_address`: The ResourceAddress of the CDP's collateral.
        /// * `cdp_id`: The NonFungibleLocalId of the CDP to potentially unmark.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If `cdp_id` is invalid.
        fn unmark_if_marked(
            &mut self,
            collateral_address: ResourceAddress,
            cdp_id: NonFungibleLocalId,
        ) {
            if self
                .collaterals
                .get(&collateral_address)
                .unwrap()
                .marked_cdps
                .get(&cdp_id)
                .is_some()
            {
                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .marked_cdps
                    .remove(&cdp_id);
            }
            
            self.cdp_manager
                .update_non_fungible_data(&cdp_id, "status", CdpStatus::Healthy);
        }

        /// Increases the total recorded debt for a given collateral type.
        /// Used internally when fUSD is minted against this collateral.
        ///
        /// # Arguments
        /// * `collateral_address`: The ResourceAddress of the collateral.
        /// * `amount`: The Decimal amount of debt to add.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        fn add_debt_to_collateral(&mut self, collateral_address: ResourceAddress, amount: Decimal) {
            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .total_debt += amount;
        }

        /// Decreases the total recorded debt for a given collateral type.
        /// Used internally when fUSD debt backed by this collateral is repaid or redeemed.
        ///
        /// # Arguments
        /// * `collateral_address`: The ResourceAddress of the collateral.
        /// * `amount`: The Decimal amount of debt to remove.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        fn remove_debt_from_collateral(
            &mut self,
            collateral_address: ResourceAddress,
            amount: Decimal,
        ) {
            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .total_debt -= amount;
        }

        /// Internal helper to mint fUSD and update associated debt tracking.
        ///
        /// This function takes a `pool_amount` (debt in normalized pool units),
        /// converts it to the equivalent real fUSD amount based on the current debt multiplier
        /// for the given collateral/interest, mints the real fUSD, and updates the
        /// `pool_debt`, `real_debt`, and global `circulating_fusd` accordingly.
        /// It also ensures the `InterestInfo` struct exists for the target rate.
        ///
        /// # Arguments
        /// * `collateral_address`: The ResourceAddress of the collateral backing the debt.
        /// * `interest`: The Decimal interest rate associated with the debt.
        /// * `pool_amount`: The Decimal amount of debt to add, expressed in pool units.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the newly minted real fUSD.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        fn mint_fusd(
            &mut self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            pool_amount: Decimal,
        ) -> Bucket {
            self.insert_interest_info_if_absent(collateral_address, interest);

            let amount_to_mint = self.pool_to_real_debt(collateral_address, interest, pool_amount);

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&interest)
                .unwrap()
                .pool_debt += pool_amount;

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&interest)
                .unwrap()
                .real_debt += amount_to_mint;

            self.circulating_fusd += amount_to_mint;

            self.fusd_manager.mint(amount_to_mint)
        }

        /// Internal helper to burn fUSD and update associated debt tracking.
        ///
        /// This function takes a bucket of real fUSD to burn, converts the amount to the
        /// equivalent `pool_amount` (debt in normalized pool units) based on the current
        /// debt multiplier for the given collateral/interest, burns the fUSD, and updates the
        /// `pool_debt`, `real_debt`, and global `circulating_fusd` accordingly.
        ///
        /// # Arguments
        /// * `collateral_address`: The ResourceAddress of the collateral associated with the debt.
        /// * `interest`: The Decimal interest rate associated with the debt.
        /// * `unmint_bucket`: The Bucket containing the real fUSD to be burned.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If the `interest` rate does not exist for the collateral.
        /// * If the `unmint_bucket` resource address does not match the component's fUSD manager.
        fn unmint_fusd(
            &mut self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            unmint_bucket: Bucket,
        ) {
            let pool_amount_to_unmint =
                self.real_to_pool_debt(collateral_address, interest, unmint_bucket.amount());

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&interest)
                .unwrap()
                .pool_debt -= pool_amount_to_unmint;

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&interest)
                .unwrap()
                .real_debt -= unmint_bucket.amount();

            self.circulating_fusd -= unmint_bucket.amount();

            self.fusd_manager.burn(unmint_bucket);
        }

        /// Internal helper to adjust debt tracking when a CDP changes interest rates.
        ///
        /// This removes the specified `remove_amount` (real fUSD) and its equivalent pool debt
        /// from the `old_interest` rate's `InterestInfo` and adds the `add_amount` (real fUSD)
        /// and its equivalent pool debt (calculated using the *new* rate's multiplier) to the
        /// `new_interest` rate's `InterestInfo`.
        /// `add_amount` might differ from `remove_amount` if an interest change fee was applied.
        ///
        /// # Arguments
        /// * `collateral_address`: The ResourceAddress of the collateral.
        /// * `old_interest`: The Decimal interest rate the CDP is moving from.
        /// * `new_interest`: The Decimal interest rate the CDP is moving to.
        /// * `remove_amount`: The Decimal real fUSD amount to remove from the old rate's tracking.
        /// * `add_amount`: The Decimal real fUSD amount to add to the new rate's tracking.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If `old_interest` or `new_interest` rate does not exist for the collateral.
        fn move_fusd_interest(
            &mut self,
            collateral_address: ResourceAddress,
            old_interest: Decimal,
            new_interest: Decimal,
            to_move_amount: Decimal,
        ) {
            let pool_amount_to_remove =
                self.real_to_pool_debt(collateral_address, old_interest, to_move_amount);

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&old_interest)
                .unwrap()
                .pool_debt -= pool_amount_to_remove;

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&old_interest)
                .unwrap()
                .real_debt -= to_move_amount;

            let pool_amount_to_add =
                self.real_to_pool_debt(collateral_address, new_interest, to_move_amount);

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&new_interest)
                .unwrap()
                .pool_debt += pool_amount_to_add;

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&new_interest)
                .unwrap()
                .real_debt += to_move_amount;
        }

        /// Calculates the upfront interest fee for a specified number of days.
        ///
        /// This fee is applied when opening a loan, borrowing more, or changing interest
        /// rates within the cooldown period. It calculates the interest that would accrue
        /// on the specified `on_pool_debt` over `days` at the given `interest` rate.
        ///
        /// # Arguments
        /// * `collateral_address`: The ResourceAddress of the collateral (used for debt multiplier calculation).
        /// * `interest`: The Decimal interest rate to use for the fee calculation.
        /// * `on_pool_debt`: The Decimal pool debt amount the fee is based on.
        /// * `days`: The u64 number of days worth of interest to calculate.
        ///
        /// # Returns
        /// * `(Decimal, Decimal)`: A tuple containing:
        ///     - The calculated fee amount in real fUSD.
        ///     - The calculated fee amount in pool debt units.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If the `interest` rate does not exist for the collateral.
        fn extra_interest_days_fee(
            &self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            on_pool_debt: Decimal,
            days: u64,
        ) -> (Decimal, Decimal) {
            let pool_fusd_to_mint_for_interest = (Decimal::ONE + (interest / dec!(31_556_926)))
                .checked_powi(days as i64 * 86400)
                .unwrap()
                * on_pool_debt
                - on_pool_debt;

            (
                self.pool_to_real_debt(
                    collateral_address,
                    interest,
                    pool_fusd_to_mint_for_interest,
                ),
                pool_fusd_to_mint_for_interest,
            )
        }

        /// Calculates the current debt multiplier (real_debt / pool_debt) for a specific collateral and interest rate.
        ///
        /// This multiplier reflects the accrued interest since the last charge for that rate.
        /// It returns 1.0 if the interest rate tier doesn't exist or if either debt value is zero.
        ///
        /// # Arguments
        /// * `collater al_address`: The ResourceAddress of the collateral.
        /// * `interest`: The Decimal interest rate tier.
        ///
        /// # Returns
        /// * `Decimal`: The calculated debt multiplier.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        fn get_debt_multiplier(
            &self,
            collateral_address: ResourceAddress,
            interest: Decimal,
        ) -> Decimal {
            if self
                .collaterals
                .get(&collateral_address)
                .unwrap()
                .interests
                .get(&interest)
                .is_some()
            {
                let real_debt = self
                    .collaterals
                    .get(&collateral_address)
                    .unwrap()
                    .interests
                    .get(&interest)
                    .unwrap()
                    .real_debt;

                let pool_debt = self
                    .collaterals
                    .get(&collateral_address)
                    .unwrap()
                    .interests
                    .get(&interest)
                    .unwrap()
                    .pool_debt;

                if pool_debt == Decimal::ZERO || real_debt == Decimal::ZERO {
                    return Decimal::ONE;
                } else {
                    return real_debt / pool_debt;
                }
            } else {
                return Decimal::ONE;
            }
        }

        /// Calculates the Collateral Ratio (CR), asserts it's above the Liquidation CR (LCR),
        /// and potentially unmarks the CDP if provided.
        ///
        /// This is used when opening, topping up, or changing interest/collateral to ensure
        /// the CDP remains sufficiently collateralized.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `interest`: Decimal interest rate of the CDP.
        /// * `collateral_amount`: Decimal amount of collateral held by the CDP.
        /// * `pool_debt`: Decimal pool debt of the CDP.
        /// * `cdp_id`: Optional NonFungibleLocalId of the CDP. If provided and the CR check passes,
        ///             `unmark_if_marked` will be called for this ID.
        ///
        /// # Returns
        /// * `Decimal`: The calculated CR (collateral_amount / pool_debt).
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If the calculated real CR (collateral value / real debt) is below the MCR.
        fn get_and_check_cr(
            &mut self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            collateral_amount: Decimal,
            pool_debt: Decimal,
            cdp_id: Option<NonFungibleLocalId>,
        ) -> Decimal {
            let mcr: Decimal = self.collaterals.get(&collateral_address).unwrap().mcr;

            let debt_multiplier = self.get_debt_multiplier(collateral_address, interest);

            assert!(
                self.collaterals.get(&collateral_address).unwrap().usd_price * collateral_amount
                    >= pool_debt * debt_multiplier * mcr,
                "Collateral value too low."
            );

            if let Some(cdp_id) = cdp_id {
                self.unmark_if_marked(collateral_address, cdp_id);
            }

            collateral_amount / pool_debt
        }

        /// Calculates the raw Collateral Ratio (CR = collateral_amount / pool_debt).
        /// Does not perform any validation against MCR or LCR.
        fn get_cr(&self, collateral_amount: Decimal, pool_debt: Decimal) -> Decimal {
            collateral_amount / pool_debt
        }

        /// Calculates the Liquidation Collateral Ratio (LCR) for a given collateral and interest rate.
        /// LCR is the CR threshold (in pool units) below which a CDP can be liquidated.
        /// LCR = MCR * (debt_multiplier / usd_price)
        fn get_lcr(&self, collateral_address: ResourceAddress, interest: Decimal) -> Decimal {
            let mcr: Decimal = self.collaterals.get(&collateral_address).unwrap().mcr;

            let usd_price: Decimal = self.collaterals.get(&collateral_address).unwrap().usd_price;

            let debt_multiplier = self.get_debt_multiplier(collateral_address, interest);

            mcr * (debt_multiplier / usd_price)
        }

        /// Converts a real fUSD amount to its equivalent pool debt amount using the current debt multiplier.
        /// pool_debt = real_amount / debt_multiplier
        fn real_to_pool_debt(
            &self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            amount: Decimal,
        ) -> Decimal {
            let debt_multiplier = self.get_debt_multiplier(collateral_address, interest);

            amount / debt_multiplier
        }

        /// Converts a pool debt amount to its equivalent real fUSD amount using the current debt multiplier.
        /// real_amount = pool_amount * debt_multiplier
        fn pool_to_real_debt(
            &self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            amount: Decimal,
        ) -> Decimal {
            let debt_multiplier = self.get_debt_multiplier(collateral_address, interest);

            amount * debt_multiplier
        }

        /// Insert a collateral ratio into the AvlTree
        ///
        /// Ensures the necessary `InterestInfo` and CR `AvlTree` exist for the given interest rate.
        /// Finds the vector associated with the specific `cr` value (or creates it if it doesn't exist)
        /// and pushes the `cdp_id` into it. Updates the `number_of_crs` count in `InterestInfo`
        /// if a new CR entry was created.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `interest`: Decimal interest rate.
        /// * `cr`: Decimal collateral ratio (pool units) to insert the CDP under.
        /// * `cdp_id`: NonFungibleLocalId of the CDP to insert.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If the vector for the `cr` value is full (exceeds `parameters.max_vector_length`).
        fn insert_cr(
            &mut self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            cr: Decimal,
            cdp_id: NonFungibleLocalId,
        ) {
            self.insert_ratios_by_interest_if_absent(collateral_address, interest);
            self.insert_interest_info_if_absent(collateral_address, interest);

            if self
                .collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .ratios_by_interest
                .get_mut(&interest)
                .unwrap()
                .get_mut(&cr)
                .is_some()
            {
                let mut cdp_ids: Vec<NonFungibleLocalId> = self
                    .collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .ratios_by_interest
                    .get_mut(&interest)
                    .unwrap()
                    .get_mut(&cr)
                    .unwrap()
                    .to_vec();

            assert!(
                    cdp_ids.len() < self.parameters.max_vector_length.try_into().unwrap(),
                    "CR vector is full... Try a different collateral / debt ratio."
                );

                cdp_ids.push(cdp_id);

                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .ratios_by_interest
                    .get_mut(&interest)
                    .unwrap()
                    .insert(cr, cdp_ids);
            } else {
                let cdp_ids: Vec<NonFungibleLocalId> = vec![cdp_id];

                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .ratios_by_interest
                    .get_mut(&interest)
                    .unwrap()
                    .insert(cr, cdp_ids);

                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .interests
                    .get_mut(&interest)
                    .unwrap()
                    .number_of_crs += 1;
            }
        }

        /// Remove a collateral ratio from the AvlTree
        ///
        /// Finds the vector associated with the specific `cr` value for the given interest rate
        /// and removes the specified `cdp_id` from it. If the vector becomes empty after removal,
        /// the entire CR entry is removed from the `AvlTree`, and the `number_of_crs` count
        /// in `InterestInfo` is decremented.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `interest`: Decimal interest rate.
        /// * `cr`: Decimal collateral ratio (pool units) to remove the CDP from.
        /// * `cdp_id`: NonFungibleLocalId of the CDP to remove.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If the `interest` rate does not exist.
        /// * If the `cr` value does not exist within the interest rate's tree.
        /// * If the `cdp_id` is not found within the vector for the specified `cr`.
        fn remove_cr(
            &mut self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            cr: Decimal,
            cdp_id: NonFungibleLocalId,
        ) {
            let mut cdp_ids: Vec<NonFungibleLocalId> = self
                .collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .ratios_by_interest
                .get_mut(&interest)
                .unwrap()
                .get_mut(&cr)
                .unwrap()
                .to_vec();

            cdp_ids.retain(|id| id != &cdp_id);

            if cdp_ids.is_empty() {
                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .ratios_by_interest
                    .get_mut(&interest)
                    .unwrap()
                    .remove(&cr);

                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .interests
                    .get_mut(&interest)
                    .unwrap()
                    .number_of_crs -= 1;
            } else {
                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .ratios_by_interest
                    .get_mut(&interest)
                    .unwrap()
                    .insert(cr, cdp_ids);
            }
        }

        /// Take collateral out of the correct vault
        /// Also updates the total collateral amount and the amount tracked per interest rate.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `interest`: Decimal interest rate the collateral is associated with.
        /// * `amount`: Decimal amount to withdraw.
        ///
        /// # Returns
        /// * `Bucket`: Bucket containing the withdrawn collateral.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If `interest` rate does not exist.
        /// * If withdrawing `amount` exceeds available balance in the vault.
        fn take_collateral(
            &mut self,
            collateral_address: ResourceAddress,
            interest: Decimal,
            amount: Decimal,
        ) -> Bucket {
            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .collateral_amount -= amount;

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&interest)
                .unwrap()
                .collateral_amount -= amount;

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .vault
                .take_advanced(amount, WithdrawStrategy::Rounded(RoundingMode::ToZero))
        }

        /// Put collateral in the correct vault
        /// Also updates the total collateral amount and the amount tracked per interest rate.
        /// Ensures the `InterestInfo` struct exists for the given rate.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `collateral_bucket`: Bucket containing the collateral to deposit.
        /// * `interest`: Decimal interest rate the collateral is associated with.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If `collateral_bucket` resource does not match `collateral_address`.
        fn put_collateral(
            &mut self,
            collateral_address: ResourceAddress,
            collateral_bucket: Bucket,
            interest: Decimal,
        ) {
            self.insert_interest_info_if_absent(collateral_address, interest);

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .collateral_amount += collateral_bucket.amount();

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&interest)
                .unwrap()
                .collateral_amount += collateral_bucket.amount();

            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .vault
                .put(collateral_bucket);
        }

        /// Put collateral in the leftovers vault
        /// Used after liquidations or full redemptions where some collateral might remain.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `collateral_bucket`: Bucket containing the leftover collateral.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If `collateral_bucket` resource does not match `collateral_address`.
        fn put_collateral_in_leftovers(
            &mut self,
            collateral_address: ResourceAddress,
            collateral_bucket: Bucket,
        ) {
            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .leftovers
                .put(collateral_bucket);
        }

        /// Take collateral from the leftovers
        /// Allows the original owner of a liquidated/redeemed CDP to claim remaining collateral.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `collateral_amount`: Decimal amount to withdraw.
        ///
        /// # Returns
        /// * `Bucket`: Bucket containing the withdrawn leftover collateral.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If withdrawing `collateral_amount` exceeds available balance in the leftovers vault.
        fn take_collateral_from_leftovers(
            &mut self,
            collateral_address: ResourceAddress,
            collateral_amount: Decimal,
        ) -> Bucket {
            self.collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .leftovers
                .take(collateral_amount)
        }

        /// Creates and inserts a default `InterestInfo` struct for a given collateral and interest rate
        /// if one does not already exist.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `interest`: Decimal interest rate tier.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        fn insert_interest_info_if_absent(
            &mut self,
            collateral_address: ResourceAddress,
            interest: Decimal,
        ) {
            if self
                .collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .interests
                .get_mut(&interest)
                .is_none()
            {
                let interest_info = InterestInfo {
                    pool_debt: Decimal::ZERO,
                    real_debt: Decimal::ZERO,
                    collateral_amount: Decimal::ZERO,
                    number_of_crs: 0,
                    last_interest_charge: Clock::current_time_rounded_to_seconds()
                        .seconds_since_unix_epoch,
                    interest,
                };

                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .interests
                    .insert(interest, interest_info);
            }
        }

        /// Creates and inserts an empty `AvlTree` for collateral ratios into the `ratios_by_interest`
        /// KeyValueStore for a given collateral and interest rate, if one does not already exist.
        ///
        /// # Arguments
        /// * `collateral_address`: ResourceAddress of the collateral.
        /// * `interest`: Decimal interest rate tier.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        fn insert_ratios_by_interest_if_absent(
            &mut self,
            collateral_address: ResourceAddress,
            interest: Decimal,
        ) {
            if self
                .collaterals
                .get_mut(&collateral_address)
                .unwrap()
                .ratios_by_interest
                .get_mut(&interest)
                .is_none()
            {
                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .ratios_by_interest
                    .insert(interest, AvlTree::new());
            }
        }

        /// Removes the `InterestInfo` entry for a specific interest rate if it no longer
        /// has any associated Collateral Ratio (CR) entries (`number_of_crs <= 0`).
        ///
        /// This is called after operations like closing a CDP or changing its interest rate
        /// to prevent empty interest rate tiers from persisting.
        ///
        /// # Arguments
        /// * `interest`: The Decimal interest rate tier to potentially clean up.
        /// * `collateral_address`: The ResourceAddress of the associated collateral.
        ///
        /// # Panics
        /// * If `collateral_address` is invalid.
        /// * If the `interest` rate does not exist for the collateral.
        fn clean_up_interest_info(
            &mut self,
            interest: Decimal,
            collateral_address: ResourceAddress,
        ) {
            if self
                .collaterals
                .get(&collateral_address)
                .unwrap()
                .interests
                .get(&interest)
                .unwrap()
                .number_of_crs
                <= 0
            {
                self.collaterals
                    .get_mut(&collateral_address)
                    .unwrap()
                    .interests
                    .remove(&interest);
            }
        }
        
        /// Checks if a Decimal `value` is perfectly divisible by another Decimal `divisor`.
        /// Works by comparing the underlying atto units.
        fn is_divisible_by(&self, value: Decimal, divisor: Decimal) -> bool {
            let value_attos = value.attos();
            let divisor_attos = divisor.attos();

            value_attos % divisor_attos == I192::from(0)
        }
    }
}

#[derive(ScryptoSbor)]
/// All info about a collateral used by the protocol
pub struct CollateralInfo {
    pub collateral_amount: Decimal,
    pub total_debt: Decimal,
    pub resource_address: ResourceAddress,
    pub mcr: Decimal,
    pub usd_price: Decimal,
    pub vault: Vault,
    pub leftovers: Vault,
    pub uncharged_interest: Vault,
    pub accepted: bool,
    pub ratios_by_interest: KeyValueStore<Decimal, AvlTree<Decimal, Vec<NonFungibleLocalId>>>,
    pub interests: AvlTree<Decimal, InterestInfo>,
    pub marked_cdps: KeyValueStore<NonFungibleLocalId, Instant>,
}

#[derive(ScryptoSbor, Clone)]
/// All info about a collateral with specific interest used by the protocol
pub struct InterestInfo {
    pub pool_debt: Decimal,
    pub real_debt: Decimal,
    pub collateral_amount: Decimal,
    pub number_of_crs: u64,
    pub last_interest_charge: i64,
    pub interest: Decimal,
}

#[derive(ScryptoSbor)]
pub struct ProtocolParameters {
    pub minimum_mint: Decimal,
    pub liquidation_fine: Decimal,
    pub stop_liquidations: bool,
    pub stop_openings: bool,
    pub stop_closings: bool,
    pub stop_redemption: bool,
    pub max_interest: Decimal,
    pub interest_interval: Decimal,
    pub max_vector_length: u64,
    pub days_of_extra_interest_fee: u64,
    pub feeless_interest_rate_change_cooldown: u64,
    pub redemption_halflife_k: Decimal,
    pub redemption_spike_k: Decimal,
    pub minimum_redemption_fee: Decimal,
    pub maximum_redemption_fee: Decimal,
    pub irredeemable_tag_fee: Decimal,
    pub liquidation_notice_fee: Decimal,
}

#[derive(ScryptoSbor)]
/// Internal state representation for a collateral type during the optimal redemption route calculation.
/// Used within the `get_optimal_redemption_route` method.
struct CollateralState {
    /// The resource address of the collateral.
    address: ResourceAddress,
    /// The target fUSD amount to redeem for this collateral.
     target: Decimal,
    /// The currently calculated fUSD amount redeemed for this collateral during simulation.
    current_amount: Decimal,
    /// The number of simulated redemption steps performed for this collateral.
    redemption_count: u64,
    /// The starting index within the `all_redemptions` vector for this collateral's pre-fetched redemption amounts.
     redemptions_start_idx: usize,
 }
