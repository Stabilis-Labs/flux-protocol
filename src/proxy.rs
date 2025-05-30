#![allow(deprecated)]

//! # Flux Protocol Proxy Blueprint
//!
//! This blueprint defines the `Proxy` component, which serves as the primary user-facing entry point
//! for interacting with the Flux protocol.
//!
//! ## Responsibilities
//! - **Routing:** Directs user calls to the appropriate underlying components (`Flux`, `FlashLoans`, `StabilityPools`).
//! - **Authorization:** Manages controller badges and uses them to authorize calls to protected methods
//!   on the underlying components.
//! - **Oracle Interaction:** Fetches collateral prices from a configured oracle component and passes them
//!   to the `Flux` component when needed for operations like opening CDPs, borrowing more, liquidations, etc.
//! - **Proof Handling:** Checks user-provided proofs (e.g., CDP NFT proofs, privileged borrower proofs) before
//!   forwarding calls that require them.
//! - **Admin Functions:** Provides administrative methods for managing the protocol, such as adding new collateral types,
//!   setting parameters, managing controller badges, and updating component links.
//! - **DApp Definition Management:** Creates and manages the DApp Definition associated with the Flux protocol.
//!
//! By acting as an intermediary, the Proxy enhances security, simplifies user interaction (by abstracting
//! away the need to call multiple components directly), and potentially facilitates easier upgrades
//! of the underlying components.

use crate::flux_component::flux_component::*;
use crate::flash_loans::flash_loans::*;
use crate::shared_structs::*;
use crate::stability_pools::stability_pools::*;
use crate::payout_component::payout_component::*;
use scrypto::prelude::*;

#[blueprint]

mod proxy {
    enable_method_auth! {
        methods {
            // Public User Actions (Routed to underlying components)
            open_cdp => PUBLIC;
            close_cdp => PUBLIC;
            top_up_cdp => PUBLIC;
            remove_collateral => PUBLIC;
            change_cdp_interest => PUBLIC;
            partial_close_cdp => PUBLIC;
            retrieve_leftover_collateral => PUBLIC;
            borrow_more => PUBLIC;
            flash_borrow => PUBLIC;
            flash_pay_back => PUBLIC;
            burn_loan_receipt => PUBLIC; // Allows burning finalized CDP NFTs
            tag_irredeemable => PUBLIC;
            unmark => PUBLIC;
            link_cdp_to_privileged_borrower => PUBLIC;
            unlink_cdp_from_privileged_borrower => PUBLIC;

            // Owner/Admin Actions (Require Owner Badge for Proxy, often use Controller Badge for underlying calls)
            receive_badges => restrict_to: [OWNER]; // Receive controller badges
            set_oracle => restrict_to: [OWNER];
            send_badges => restrict_to: [OWNER]; // Send controller badges
            flash_retrieve_interest => restrict_to: [OWNER];
            add_claimed_website => restrict_to: [OWNER];
            change_collateral_price => restrict_to: [OWNER]; // Directly set Flux price (admin override)
            set_max_vector_length => restrict_to: [OWNER]; // Set Flux parameter
            edit_collateral => restrict_to: [OWNER]; // Edit Flux collateral params
            mint_controller_badge => restrict_to: [OWNER]; // Mint more Flux controller badges
            set_stops => restrict_to: [OWNER]; // Set Flux stops
            set_minimum_mint => restrict_to: [OWNER]; // Set Flux parameter
            set_fines => restrict_to: [OWNER]; // Set Flux parameter
            set_interest_params => restrict_to: [OWNER]; // Set Flux parameter
            new_collateral => restrict_to: [OWNER]; // Add new collateral type to Flux & StabilityPools
            send_stability_pool_badges => restrict_to: [OWNER]; // Send controller badges to StabilityPools
            edit_stability_pool => restrict_to: [OWNER]; // Edit StabilityPools params for a specific pool
            set_stability_pools_parameters => restrict_to: [OWNER]; // Set global StabilityPools params
            set_redemption_parameters => restrict_to: [OWNER]; // Set Flux redemption params
            create_privileged_borrower => restrict_to: [OWNER]; // Create Flux privileged borrower NFT
            edit_privileged_borrower => restrict_to: [OWNER]; // Edit Flux privileged borrower NFT data
            payout_set_parameters => restrict_to: [OWNER]; // Set PayoutComponent parameters
            set_panic_mode_parameters => restrict_to: [OWNER]; // Set StabilityPools panic mode parameters
        }
    }

    /// Acts as the main entry point and authorization layer for the Flux protocol.
    /// Routes calls to `Flux`, `FlashLoans`, and `StabilityPools` components.
    struct Proxy {
        /// Vault holding controller badges used to authorize calls to other protocol components.
        badge_vault: FungibleVault,
        /// Global reference to the price oracle component.
        oracle: Global<AnyComponent>,
        /// Global reference to the core `Flux` logic component.
        flux: Global<Flux>,
        /// Global reference to the `StabilityPools` component.
        stability_pools: Global<StabilityPools>,
        /// Global reference to the `PayoutComponent` component.
        payout_component: Global<PayoutComponent>,
        /// The method name expected by the `oracle` component for single price lookups.
        oracle_method_name: String,
        /// Global reference to the `FlashLoans` component.
        flash_loans: Global<FlashLoans>,
        /// The resource manager for the CDP NFT receipts.
        cdp_receipt_manager: ResourceManager,
        /// The resource manager for the Privileged Borrower NFTs.
        privileged_borrower_manager: ResourceManager,
        /// Global reference to the DApp Definition account associated with the protocol.
        dapp_def_account: Global<Account>,
    }

    impl Proxy {
        /// Instantiates the entire Flux protocol stack: Proxy, Flux (core), FlashLoans, and StabilityPools.
        /// Creates the necessary resources (fUSD, CDP NFTs, Privileged Borrower NFTs, Controller Badge)
        /// and the DApp Definition account.
        ///
        /// # Arguments
        /// * `owner_role_address`: The `ResourceAddress` of the badge required for OWNER actions on the Proxy itself.
        /// * `oracle_address`: The `ComponentAddress` of the price oracle.
        /// * `payout_address`: The `ComponentAddress` of the payout component for stability pool rewards.
        /// * `centralized_stablecoin_address`: The `ResourceAddress` of the initial stablecoin for panic mode.
        ///
        /// # Returns
        /// * `(Global<Proxy>, Global<Flux>, Global<FlashLoans>, Global<StabilityPools>, Bucket)`:
        ///   Global references to the newly instantiated Proxy, Flux, FlashLoans, and StabilityPools components.
        ///   The bucket is the controller badge to be used by the instantiator and to be returned to this component after setup is complete.
        ///
        /// # Logic
        /// 1. Defines the owner role for the Proxy.
        /// 2. Allocates the Proxy component address.
        /// 3. Creates the DApp Definition account.
        /// 4. Instantiates the core `Flux` component, obtaining its global reference, controller badge, and resource addresses.
        /// 5. Sets initial metadata on the `Flux` component.
        /// 6. Instantiates the `FlashLoans` component, providing it with a controller badge and links.
        /// 7. Instantiates the `StabilityPools` component, providing it with a controller badge and links.
        /// 8. Sets metadata on the DApp Definition account (name, description, URLs, claimed entities/websites).
        /// 9. Sets the owner role for the DApp Definition account.
        /// 10. Instantiates the `Proxy` component state with links to other components and resource managers.
        /// 11. Globalizes the `Proxy` component with its owner role and metadata.
        /// 12. Returns the global references to the four main components.
        pub fn new(
            dao_owner_role_address: ResourceAddress,
            oracle_address: ComponentAddress,
            centralized_stablecoin_address: ResourceAddress,
            payout_token_address: ResourceAddress,
            payout_initial_required_amount: Decimal,
            manual_liquidity_airdropper_address: ResourceAddress,
        ) -> (Global<Proxy>, Global<Flux>, Global<FlashLoans>, Global<StabilityPools>, Global<PayoutComponent>, Bucket) {
            let (address_reservation, component_address) =
                Runtime::allocate_component_address(Proxy::blueprint_id());

            let dapp_def_account =
                Blueprint::<Account>::create_advanced(OwnerRole::Updatable(rule!(allow_all)), None); // will reset owner role after dapp def metadata has been set
            let dapp_def_address = GlobalAddress::from(dapp_def_account.address());

            let (flux, mut controller_badge, privileged_borrower_address, cdp_receipt_address, fusd_address) = Flux::instantiate(dapp_def_address);
            let controller_badge_address = controller_badge.resource_address();
            let controller_badge_to_return = controller_badge.take(Decimal::ONE);

            let owner_role_access_rule = rule!(require_amount(dec!("0.75"), dao_owner_role_address) || require_amount(dec!("0.75"), controller_badge_address));
            let owner_role = OwnerRole::Fixed(owner_role_access_rule.clone());

            controller_badge.authorize_with_all(|| {
                flux.set_metadata("dapp_definition", dapp_def_address);
                flux.set_metadata("info_url", Url::of("https://flux.ilikeitstable.com"));
                flux.set_metadata("name", "Flux Protocol".to_string());
                flux.set_metadata("description", "Borrow fUSD on your own terms.".to_string());
            });

            let flash_loans = FlashLoans::instantiate(
                controller_badge.take(1),
                Global::from(flux),
                dapp_def_address,
            );

            let manual_liquidity_airdropper_access_rule: AccessRule = rule!(require(manual_liquidity_airdropper_address));

            let stability_pools = StabilityPools::instantiate(
                controller_badge.take(1),
                fusd_address,
                oracle_address,
                flux.address(),
                dapp_def_address,
                cdp_receipt_address,
                centralized_stablecoin_address,
                manual_liquidity_airdropper_access_rule,
            );

            let payout_component = PayoutComponent::instantiate(
                controller_badge.take(1),
                payout_token_address,
                payout_initial_required_amount,
                fusd_address,
                stability_pools.address(),
                owner_role.clone(),
                dapp_def_address,
            );

            dapp_def_account.set_metadata("account_type", String::from("dapp definition"));
            dapp_def_account.set_metadata("name", "Flux Protocol".to_string());
            dapp_def_account
                .set_metadata("description", "Flux is a decentralized stablecoin borrowing protocol, governed by the ILIS DAO.".to_string());
            dapp_def_account.set_metadata("info_url", Url::of("https://flux.ilikeitstable.com"));
            dapp_def_account.set_metadata(
                "icon_url",
                Url::of("https://flux.ilikeitstable.com/flux-logo.png"),
            );
            dapp_def_account.set_metadata(
                "claimed_websites",
                vec![
                    UncheckedOrigin::of("https://flux.ilikeitstable.com"),
                    UncheckedOrigin::of("https://ilikeitstable.com"),
                    UncheckedOrigin::of("https://dao.ilikeitstable.com"),
                    UncheckedOrigin::of("https://stab.ilikeitstable.com"),
                    UncheckedOrigin::of("https://docs.ilikeitstable.com"),
                ],
            );
            dapp_def_account.set_metadata(
                "tags",
                vec![
                    String::from("defi"),
                    String::from("stablecoin"),
                    String::from("borrowing"),
                    String::from("usd"),
                ],
            );
            dapp_def_account.set_metadata("dapp_category", String::from("defi"));
            dapp_def_account.set_metadata(
                "claimed_entities",
                vec![
                    GlobalAddress::from(component_address.clone()),
                    GlobalAddress::from(flux.address()),
                    GlobalAddress::from(flash_loans.address()),
                    GlobalAddress::from(stability_pools.address()),
                    GlobalAddress::from(payout_component.address()),
                    GlobalAddress::from(oracle_address),
                    GlobalAddress::from(fusd_address),
                    GlobalAddress::from(cdp_receipt_address),
                    GlobalAddress::from(privileged_borrower_address),
                ],
            );

            dapp_def_account.set_owner_role(owner_role_access_rule);

            controller_badge
                .authorize_with_all(|| flux.set_metadata("dapp_definition", dapp_def_address));

            let proxy = Self {
                flash_loans,
                badge_vault: FungibleVault::with_bucket(controller_badge.as_fungible()),
                flux,
                stability_pools,
                payout_component,
                oracle: Global::from(oracle_address),
                oracle_method_name: "check_price_input".to_string(),
                cdp_receipt_manager: ResourceManager::from_address(cdp_receipt_address),
                privileged_borrower_manager: ResourceManager::from_address(privileged_borrower_address),
                dapp_def_account,
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .with_address(address_reservation)
            .metadata(metadata! {
                init {
                    "name" => "Flux Protocol Proxy".to_string(), updatable;
                    "description" => "A proxy component for the Flux Protocol".to_string(), updatable;
                    "info_url" => Url::of("https://flux.ilikeitstable.com"), updatable;
                    "icon_url" => Url::of("https://flux.ilikeitstable.com/flux-logo.png"), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                }
            })
            .globalize();

            (proxy, flux, flash_loans, stability_pools, payout_component, controller_badge_to_return)
        }

        //==================================================================
        //                         ADMIN METHODS
        //==================================================================

        /// Allows the Proxy component (specifically its OWNER) to receive controller badges.
        /// These badges are likely minted by the `Flux` component and sent here for the Proxy
        /// to use when authorizing calls to other components.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `badge_bucket`: A `Bucket` containing controller badges (`fusdCTRL`).
        pub fn receive_badges(&mut self, badge_bucket: Bucket) {
            self.badge_vault.put(badge_bucket.as_fungible());
        }

        /// Updates the oracle component address and method names used by the Proxy and StabilityPools components.
        /// Requires OWNER authorization on the Proxy.
        /// Uses a controller badge from its vault to authorize the call to `StabilityPools::set_oracle`.
        ///
        /// # Arguments
        /// * `oracle_address`: The `ComponentAddress` of the new oracle.
        /// * `single_method_name`: The method name for single price lookups on the new oracle.
        /// * `batch_method_name`: The method name for batch price lookups on the new oracle.
        pub fn set_oracle(
            &mut self,
            oracle_address: ComponentAddress,
            single_method_name: String,
            batch_method_name: String,
        ) {
            self.oracle = Global::from(oracle_address);
            self.oracle_method_name = single_method_name.clone();

            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.stability_pools.set_oracle(
                    oracle_address,
                    single_method_name,
                    batch_method_name,
                );
            })
        }

        /// Sends controller badges held by the Proxy to another component.
        /// Requires OWNER authorization on the Proxy.
        /// Assumes the receiving component has a `receive_badges` method.
        ///
        /// # Arguments
        /// * `amount`: The `Decimal` amount of controller badges to send.
        /// * `receiver_address`: The `ComponentAddress` of the recipient.
        pub fn send_badges(&mut self, amount: Decimal, receiver_address: ComponentAddress) {
            let receiver: Global<AnyComponent> = Global::from(receiver_address);
            let badge_bucket: Bucket = self.badge_vault.take(amount).into();
            receiver.call_raw("receive_badges", scrypto_args!(badge_bucket))
        }

        /// Adds a website origin to the list of claimed websites in the protocol's DApp Definition metadata.
        /// Requires OWNER authorization on the Proxy (which controls the DApp Definition account).
        ///
        /// # Arguments
        /// * `website`: The `UncheckedOrigin` of the website to add.
        pub fn add_claimed_website(&mut self, website: UncheckedOrigin) {
            match self.dapp_def_account.get_metadata("claimed_websites") {
                Ok(Some(claimed_websites)) => {
                    let mut claimed_websites: Vec<UncheckedOrigin> = claimed_websites;
                    claimed_websites.push(website);
                    self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                        self.dapp_def_account
                            .set_metadata("claimed_websites", claimed_websites);
                    });
                }
                Ok(None) | Err(_) => {
                    self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                        self.dapp_def_account
                            .set_metadata("claimed_websites", vec![website]);
                    });
                }
            }
        }

        //==================================================================
        //    PROXY FUNCTIONALITY FROM HERE (CONTROL OTHER COMPONENTS)
        //==================================================================

        //==================================================================
        //                         FLUX COMPONENT
        //==================================================================

        /// Borrows more fUSD against an existing CDP.
        /// Fetches the collateral price from the oracle and calls `Flux::borrow_more`.
        /// Requires proof of ownership of the CDP NFT.
        ///
        /// # Arguments
        /// * `receipt_proof`: A `NonFungibleProof` of the CDP NFT.
        /// * `amount`: The `Decimal` amount of fUSD to borrow.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the newly borrowed fUSD.
        pub fn borrow_more(
            &mut self,
            receipt_proof: NonFungibleProof,
            amount: Decimal,
            message: String,
            signature: String,
        ) -> Bucket {
            let receipt_proof = receipt_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );
            let receipt = receipt_proof.non_fungible::<Cdp>();
            let receipt_id: NonFungibleLocalId = receipt.local_id().clone();
            let collateral = receipt.data().collateral_address;

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_method_name,
                scrypto_args!(collateral, message, signature),
            );

            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux
                    .borrow_more(receipt_id, amount, true, true, Some(price))
            })
        }

        /// Opens a new CDP.
        /// Fetches the collateral price from the oracle and calls `Flux::open_cdp`.
        /// Optionally accepts proof of a privileged borrower NFT.
        ///
        /// # Arguments
        /// * `privileged_borrower_proof`: Optional `NonFungibleProof` of a Privileged Borrower NFT.
        /// * `collateral_bucket`: A `Bucket` containing the initial collateral deposit.
        /// * `amount`: The `Decimal` amount of fUSD to mint.
        /// * `interest`: The desired annual interest rate for the CDP.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing the minted fUSD and the new CDP NFT.
        pub fn open_cdp(
            &mut self,
            privileged_borrower_proof: Option<NonFungibleProof>,
            collateral_bucket: Bucket,
            amount: Decimal,
            interest: Decimal,
            message: String,
            signature: String,
        ) -> (Bucket, Bucket) {
            let collateral = collateral_bucket.resource_address();

            let borrower_id: Option<NonFungibleLocalId> =
                if let Some(proof) = privileged_borrower_proof {
                    let borrower_proof = proof.check_with_message(
                        self.privileged_borrower_manager.address(),
                        "Incorrect proof! Are you sure this is a privileged borrower NFT?",
                    );
                    let borrower = borrower_proof.non_fungible::<PrivilegedBorrowerData>();
                    Some(borrower.local_id().clone())
                } else {
                    None
                };

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_method_name,
                scrypto_args!(collateral, message, signature),
            );

            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux.open_cdp(
                    collateral_bucket,
                    amount,
                    interest,
                    borrower_id,
                    Some(price),
                )
            })
        }

        /// Closes a CDP by repaying the full debt.
        /// Calls `Flux::close_cdp`.
        /// Requires proof of ownership of the CDP NFT.
        ///
        /// # Arguments
        /// * `receipt_proof`: A `NonFungibleProof` of the CDP NFT.
        /// * `fusd_payment`: A `Bucket` containing the fUSD repayment (must cover the full debt).
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing the withdrawn collateral and any leftover fUSD payment.
        pub fn close_cdp(
            &mut self,
            receipt_proof: NonFungibleProof,
            fusd_payment: Bucket,
        ) -> (Bucket, Bucket) {
            let receipt_proof = receipt_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );
            let receipt = receipt_proof.non_fungible::<Cdp>();
            let receipt_id: NonFungibleLocalId = receipt.local_id().clone();

            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux.close_cdp(receipt_id, fusd_payment)
            })
        }

        /// Partially closes a CDP by repaying some of the debt.
        /// Calls `Flux::partial_close_cdp`.
        /// Requires proof of ownership of the CDP NFT.
        /// If the repayment covers the full remaining debt, it effectively closes the CDP.
        ///
        /// # Arguments
        /// * `receipt_proof`: A `NonFungibleProof` of the CDP NFT.
        /// * `fusd_payment`: A `Bucket` containing the fUSD repayment.
        ///
        /// # Returns
        /// * `(Option<Bucket>, Option<Bucket>)`: Tuple containing optional collateral and leftover payment
        ///                                      buckets *only if* the CDP was fully closed by the repayment.
        ///                                      Returns `(None, None)` for a partial repayment.
        pub fn partial_close_cdp(
            &mut self,
            receipt_proof: NonFungibleProof,
            fusd_payment: Bucket,
        ) -> (Option<Bucket>, Option<Bucket>) {
            let receipt_proof = receipt_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );
            let receipt = receipt_proof.non_fungible::<Cdp>();
            let receipt_id: NonFungibleLocalId = receipt.local_id().clone();

            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux.partial_close_cdp(receipt_id, fusd_payment)
            })
        }

        /// Adds more collateral to an existing CDP.
        /// Fetches the collateral price from the oracle and calls `Flux::top_up_cdp`.
        /// Requires proof of ownership of the CDP NFT.
        ///
        /// # Arguments
        /// * `receipt_proof`: A `NonFungibleProof` of the CDP NFT.
        /// * `collateral`: A `Bucket` containing the additional collateral.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        pub fn top_up_cdp(
            &mut self,
            receipt_proof: NonFungibleProof,
            collateral: Bucket,
            message: String,
            signature: String,
        ) {
            let receipt_proof = receipt_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );
            let receipt = receipt_proof.non_fungible::<Cdp>();
            let receipt_id: NonFungibleLocalId = receipt.local_id().clone();
            let collateral_address = receipt.data().collateral_address;

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_method_name,
                scrypto_args!(collateral_address, message, signature),
            );

            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux.top_up_cdp(receipt_id, collateral, Some(price))
            })
        }

        /// Removes collateral from an existing CDP, provided it remains sufficiently collateralized.
        /// Fetches the collateral price from the oracle and calls `Flux::remove_collateral`.
        /// Requires proof of ownership of the CDP NFT.
        ///
        /// # Arguments
        /// * `receipt_proof`: A `NonFungibleProof` of the CDP NFT.
        /// * `amount`: The `Decimal` amount of collateral to remove.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the removed collateral.
        pub fn remove_collateral(
            &mut self,
            receipt_proof: NonFungibleProof,
            amount: Decimal,
            message: String,
            signature: String,
        ) -> Bucket {
            let receipt_proof = receipt_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );
            let receipt = receipt_proof.non_fungible::<Cdp>();
            let receipt_id: NonFungibleLocalId = receipt.local_id().clone();
            let collateral = receipt.data().collateral_address;

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_method_name,
                scrypto_args!(collateral, message, signature),
            );

            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux
                    .remove_collateral(receipt_id, amount, Some(price))
            })
        }

        /// Retrieves leftover collateral from a closed or liquidated CDP.
        /// Calls `Flux::retrieve_leftover_collateral`.
        /// Requires proof of ownership of the CDP NFT.
        ///
        /// # Arguments
        /// * `receipt_proof`: A `NonFungibleProof` of the CDP NFT (must be in a closed/liquidated/redeemed state).
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the leftover collateral.
        pub fn retrieve_leftover_collateral(&mut self, receipt_proof: NonFungibleProof) -> Bucket {
            let receipt_proof = receipt_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );
            let receipt = receipt_proof.non_fungible::<Cdp>();
            let receipt_id: NonFungibleLocalId = receipt.local_id().clone();

            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux.retrieve_leftover_collateral(receipt_id)
            })
        }

        /// Changes the interest rate on an existing CDP.
        /// Fetches the collateral price from the oracle and calls `Flux::change_cdp_interest`.
        /// Requires proof of ownership of the CDP NFT and optionally proof of a privileged borrower NFT.
        ///
        /// # Arguments
        /// * `receipt_proof`: A `NonFungibleProof` of the CDP NFT.
        /// * `privileged_borrower_proof`: Optional `NonFungibleProof` of a Privileged Borrower NFT (required for interest rate -420).
        /// * `interest`: The new desired annual interest rate.
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        pub fn change_cdp_interest(
            &mut self,
            receipt_proof: NonFungibleProof,
            privileged_borrower_proof: Option<NonFungibleProof>,
            interest: Decimal,
            message: String,
            signature: String,
        ) {
            let receipt_proof = receipt_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );
            let receipt = receipt_proof.non_fungible::<Cdp>();
            let receipt_id: NonFungibleLocalId = receipt.local_id().clone();
            let collateral = receipt.data().collateral_address;

            let borrower_id: Option<NonFungibleLocalId> =
                if let Some(proof) = privileged_borrower_proof {
                    let borrower_proof = proof.check_with_message(
                        self.privileged_borrower_manager.address(),
                        "Incorrect proof! Are you sure this is a privileged borrower NFT?",
                    );
                    let borrower = borrower_proof.non_fungible::<PrivilegedBorrowerData>();
                    Some(borrower.local_id().clone())
                } else {
                    None
                };

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_method_name,
                scrypto_args!(collateral, message, signature),
            );

            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux
                    .change_cdp_interest(receipt_id, interest, borrower_id, Some(price))
            })
        }

        /// Attempts to unmark a CDP that was previously marked for liquidation.
        /// Fetches the collateral price from the oracle and calls `Flux::unmark`.
        /// Requires proof of ownership of the CDP NFT.
        /// This typically succeeds if the CDP's collateral ratio is now above the threshold.
        ///
        /// # Arguments
        /// * `receipt_proof`: A `NonFungibleProof` of the CDP NFT (must be in `Marked` state).
        /// * `message`: Oracle message for price verification.
        /// * `signature`: Oracle signature for price verification.
        pub fn unmark(
            &mut self,
            receipt_proof: NonFungibleProof,
            message: String,
            signature: String,
        ) {
            let receipt_proof = receipt_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );
            let receipt = receipt_proof.non_fungible::<Cdp>();
            let receipt_id: NonFungibleLocalId = receipt.local_id().clone();
            let collateral_address = receipt.data().collateral_address;

            let price: Decimal = self.oracle.call_raw(
                &self.oracle_method_name,
                scrypto_args!(collateral_address, message, signature),
            );

            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux.unmark(receipt_id, Some(price))
            })
        }

        /// Tags a privileged CDP as irredeemable, charging a fee.
        /// Calls `Flux::tag_irredeemable`.
        ///
        /// # Arguments
        /// * `cdp_id`: The `NonFungibleLocalId` of the CDP to tag.
        ///
        /// # Returns
        /// * `Bucket`: Bucket containing the fUSD fee charged for tagging.
        pub fn tag_irredeemable(&mut self, cdp_id: NonFungibleLocalId) -> Bucket {
            self.badge_vault.authorize_with_amount(dec!(0.75), || {
                self.flux.tag_irredeemable(cdp_id)
            })
        }

        /// ADMIN: Directly changes the stored price for a collateral type in the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the collateral.
        /// * `new_price`: The new `Decimal` price.
        pub fn change_collateral_price(&self, collateral: ResourceAddress, new_price: Decimal) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.change_collateral_price(collateral, new_price)
            });
        }

        /// ADMIN: Sets the maximum length of the CDP ID vector stored per CR node in the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `new_flux_length`: The new maximum length (`u64`).
        pub fn set_max_vector_length(&mut self, new_flux_length: u64) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.set_max_vector_length(new_flux_length)
            });
        }

        /// ADMIN: Edits the MCR and acceptance status for a collateral type in the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `address`: The `ResourceAddress` of the collateral to edit.
        /// * `new_mcr`: The new `Decimal` Minimum Collateral Ratio.
        /// * `new_acceptance`: `bool` indicating if the collateral is accepted for new CDPs.
        pub fn edit_collateral(
            &mut self,
            address: ResourceAddress,
            new_mcr: Decimal,
            new_acceptance: bool,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux
                    .edit_collateral(address, new_mcr, new_acceptance)
            });
        }

        /// ADMIN: Mints new controller badges from the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `amount`: The `Decimal` amount of controller badges to mint.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing the newly minted controller badges.
        pub fn mint_controller_badge(&mut self, amount: Decimal) -> Bucket {
            self.badge_vault
                .authorize_with_amount(dec!("0.75"), || self.flux.mint_controller_badge(amount))
        }

        /// ADMIN: Sets the operational stops (pauses) in the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `liquidations`: `bool` - Stop liquidations if true.
        /// * `openings`: `bool` - Stop opening/borrowing more if true.
        /// * `closings`: `bool` - Stop closing/repaying/removing collateral if true.
        /// * `redemption`: `bool` - Stop redemptions if true.
        pub fn set_stops(
            &mut self,
            liquidations: bool,
            openings: bool,
            closings: bool,
            redemption: bool,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux
                    .set_stops(liquidations, openings, closings, redemption)
            });
        }

        /// ADMIN: Sets the minimum fUSD mint amount in the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `new_minimum_mint`: The new minimum mint amount (`Decimal`).
        pub fn set_minimum_mint(&mut self, new_minimum_mint: Decimal) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.set_minimum_mint(new_minimum_mint)
            });
        }

        /// ADMIN: Sets the liquidation fine percentage in the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `liquidation_fine`: The new liquidation fine (`Decimal`).
        /// * `liquidation_notice_fee`: The new liquidation notice fee (`Decimal`).
        /// * `irredeemable_tag_fee`: The new irredeemable tag fee (`Decimal`).
        pub fn set_fines(&mut self, liquidation_fine: Decimal, liquidation_notice_fee: Decimal, irredeemable_tag_fee: Decimal) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.set_fines(liquidation_fine, liquidation_notice_fee, irredeemable_tag_fee)
            });
        }

        /// ADMIN: Sets the interest rate and extra interest fee parameters in the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `max_interest`: The maximum allowed interest rate (Decimal).
        /// * `interest_interval`: The allowed interval for interest rates (Decimal).
        /// * `feeless_interest_rate_change_cooldown`: The cooldown period (days) for free interest rate changes.
        /// * `days_of_extra_interest_fee`: The number of days' interest charged as a fee for actions within the cooldown.
        pub fn set_interest_params(
            &mut self,
            max_interest: Decimal,
            interest_interval: Decimal,
            feeless_interest_rate_change_cooldown: u64,
            days_of_extra_interest_fee: u64,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.set_interest_params(
                    max_interest,
                    interest_interval,
                    feeless_interest_rate_change_cooldown,
                    days_of_extra_interest_fee,
                )
            });
        }

        /// ADMIN: Sets the redemption fee parameters in the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `new_max_redemption_fee`: New maximum redemption fee.
        /// * `new_min_redemption_fee`: New minimum redemption fee.
        /// * `new_redemption_spike_k`: New sensitivity factor for redemption volume.
        /// * `new_redemption_halflife_k`: New decay factor for the redemption base rate.
        pub fn set_redemption_parameters(
            &mut self,
            new_max_redemption_fee: Decimal,
            new_min_redemption_fee: Decimal,
            new_redemption_spike_k: Decimal,
            new_redemption_halflife_k: Decimal,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.set_redemption_parameters(
                    new_max_redemption_fee,
                    new_min_redemption_fee,
                    new_redemption_spike_k,
                    new_redemption_halflife_k,
                )
            });
        }

        /// Burns a finalized CDP NFT receipt.
        /// Calls `Flux::burn_loan_receipt`.
        /// Requires the receipt to be in a terminal state (Closed, Liquidated, Redeemed) with zero collateral amount.
        ///
        /// # Arguments
        /// * `receipt`: A `Bucket` containing the CDP NFT to burn.
        pub fn burn_loan_receipt(&mut self, receipt: Bucket) {
            self.badge_vault
                .authorize_with_amount(dec!("0.75"), || self.flux.burn_loan_receipt(receipt));
        }

        /// ADMIN: Creates a new Privileged Borrower NFT via the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `privileged_borrower`: `PrivilegedBorrowerData` for the new NFT.
        ///
        /// # Returns
        /// * `Bucket`: Bucket containing the newly minted Privileged Borrower NFT.
        pub fn create_privileged_borrower(&mut self, privileged_borrower: PrivilegedBorrowerData) -> Bucket {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || self.flux.create_privileged_borrower(privileged_borrower))
        }

        /// ADMIN: Edits the data of an existing Privileged Borrower NFT via the Flux component.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `privileged_borrower`: `PrivilegedBorrowerData` containing the new data.
        /// * `borrower_id`: The `NonFungibleLocalId` of the NFT to edit.
        pub fn edit_privileged_borrower(&mut self, privileged_borrower: PrivilegedBorrowerData, borrower_id: NonFungibleLocalId) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || self.flux.edit_privileged_borrower(privileged_borrower, borrower_id));
        }

        /// Links a CDP NFT to a Privileged Borrower NFT.
        /// Calls `Flux::link_cdp_to_privileged_borrower`.
        /// Requires proofs for both NFTs.
        ///
        /// # Arguments
        /// * `privileged_borrower_proof`: `NonFungibleProof` of the Privileged Borrower NFT.
        /// * `cdp_proof`: `NonFungibleProof` of the CDP NFT.
        pub fn link_cdp_to_privileged_borrower(&mut self, privileged_borrower_proof: NonFungibleProof, cdp_proof: NonFungibleProof) {
            let privileged_borrower_proof = privileged_borrower_proof.check_with_message(
                self.privileged_borrower_manager.address(),
                "Incorrect proof! Are you sure this is a privileged borrower NFT?",
            );

            let cdp_proof = cdp_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );

            let cdp = cdp_proof.non_fungible::<Cdp>();
            let cdp_id: NonFungibleLocalId = cdp.local_id().clone();

            let privileged_borrower = privileged_borrower_proof.non_fungible::<PrivilegedBorrowerData>();
            let privileged_borrower_id: NonFungibleLocalId = privileged_borrower.local_id().clone();

            self.badge_vault.authorize_with_amount(dec!("0.75"), || self.flux.link_cdp_to_privileged_borrower(privileged_borrower_id, cdp_id, true));
        }

        /// Unlinks a CDP NFT from a Privileged Borrower NFT.
        /// Calls `Flux::unlink_cdp_from_privileged_borrower`.
        /// Requires proofs for both NFTs.
        ///
        /// # Arguments
        /// * `privileged_borrower_proof`: `NonFungibleProof` of the Privileged Borrower NFT.
        /// * `cdp_proof`: `NonFungibleProof` of the CDP NFT.
        pub fn unlink_cdp_from_privileged_borrower(&mut self, privileged_borrower_proof: NonFungibleProof, cdp_proof: NonFungibleProof) {
            let privileged_borrower_proof = privileged_borrower_proof.check_with_message(
                self.privileged_borrower_manager.address(),
                "Incorrect proof! Are you sure this is a privileged borrower NFT?",
            );

            let cdp_proof = cdp_proof.check_with_message(
                self.cdp_receipt_manager.address(),
                "Incorrect proof! Are you sure this loan is yours?",
            );

            let cdp = cdp_proof.non_fungible::<Cdp>();
            let cdp_id: NonFungibleLocalId = cdp.local_id().clone();

            let privileged_borrower = privileged_borrower_proof.non_fungible::<PrivilegedBorrowerData>();
            let privileged_borrower_id: NonFungibleLocalId = privileged_borrower.local_id().clone();

            self.badge_vault.authorize_with_amount(dec!("0.75"), || self.flux.unlink_cdp_from_privileged_borrower(privileged_borrower_id, cdp_id));
        }

        //==================================================================
        //                      FLASH LOANS COMPONENT
        //==================================================================

        /// Borrows fUSD via the FlashLoans component.
        /// Calls `FlashLoans::borrow`.
        ///
        /// # Arguments
        /// * `amount`: The `Decimal` amount of fUSD to borrow.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: Tuple containing the borrowed fUSD and the transient LoanReceipt NFT.
        pub fn flash_borrow(&mut self, amount: Decimal) -> (Bucket, Bucket) {
            self.badge_vault
                .authorize_with_amount(dec!("0.75"), || self.flash_loans.borrow(amount))
        }

        /// Pays back a flash loan.
        /// Calls `FlashLoans::pay_back`.
        ///
        /// # Arguments
        /// * `receipt_bucket`: The `Bucket` containing the transient LoanReceipt NFT.
        /// * `payment_bucket`: A `Bucket` containing the fUSD repayment (principal + interest).
        ///
        /// # Returns
        /// * `Bucket`: Any leftover fUSD from the `payment_bucket`.
        pub fn flash_pay_back(&mut self, receipt_bucket: Bucket, payment_bucket: Bucket) -> Bucket {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flash_loans.pay_back(receipt_bucket, payment_bucket)
            })
        }

        /// ADMIN: Retrieves accumulated interest from the FlashLoans component.
        /// Calls `FlashLoans::retrieve_interest`.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Returns
        /// * `Bucket`: Bucket containing the accumulated flash loan interest (fUSD).
        pub fn flash_retrieve_interest(&mut self) -> Bucket {
            self.badge_vault
                .authorize_with_amount(dec!("0.75"), || self.flash_loans.retrieve_interest())
        }

        //==================================================================
        //                    STABILITY POOL COMPONENT
        //==================================================================

        /// ADMIN: Sends controller badges held by the Proxy to the StabilityPools component.
        /// Requires OWNER authorization on the Proxy.
        /// Calls `StabilityPools::send_badges`.
        ///
        /// # Arguments
        /// * `amount`: The `Decimal` amount of controller badges to send.
        /// * `receiver_address`: The `ComponentAddress` of the StabilityPools component (or potentially another recipient).
        pub fn send_stability_pool_badges(
            &self,
            amount: Decimal,
            receiver_address: ComponentAddress,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.stability_pools.send_badges(amount, receiver_address);
            })
        }

        /// ADMIN: Edits the parameters for a specific stability pool in the StabilityPools component.
        /// Requires OWNER authorization on the Proxy.
        /// Calls `StabilityPools::edit_pool`.
        ///
        /// # Arguments
        /// * `collateral`: The `ResourceAddress` of the pool to edit.
        /// * `payout_split`: Optional new payout split ratio.
        /// * `liquidity_rewards_split`: Optional new liquidity rewards split ratio.
        /// * `stability_pool_split`: Optional new stability pool split ratio.
        /// * `allow_pool_buys`: New boolean value for allowing direct pool buys.
        /// * `pool_buy_price_modifier`: Optional new price modifier for direct pool buys.
        pub fn edit_stability_pool(
            &self,
            collateral: ResourceAddress,
            payout_split: Option<Decimal>,
            liquidity_rewards_split: Option<Decimal>,
            stability_pool_split: Option<Decimal>,
            allow_pool_buys: bool,
            pool_buy_price_modifier: Option<Decimal>,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.stability_pools.edit_pool(
                    collateral,
                    payout_split,
                    liquidity_rewards_split,
                    stability_pool_split,
                    allow_pool_buys,
                    pool_buy_price_modifier,
                );
            })
        }

        /// ADMIN: Sets the global default parameters for the StabilityPools component.
        /// Requires OWNER authorization on the Proxy.
        /// Calls `StabilityPools::set_parameters`.
        ///
        /// # Arguments
        /// * `default_payout_split`: Default payout split ratio.
        /// * `default_liquidity_rewards_split`: Default liquidity rewards split ratio.
        /// * `default_stability_pool_split`: Default stability pool split ratio.
        /// * `default_pool_buy_price_modifier`: Default modifier for direct pool buys.
        /// * `pool_contribution_flat_fee`: Flat fee for pool contributions.
        /// * `pool_contribution_percentage_fee`: Percentage fee for pool contributions.
        /// * `lowest_interest_history_length`: Length of the lowest interest rate history to keep.
        pub fn set_stability_pools_parameters(
            &self,
            default_payout_split: Decimal,
            default_liquidity_rewards_split: Decimal,
            default_stability_pool_split: Decimal,
            default_pool_buy_price_modifier: Decimal,
            pool_contribution_flat_fee: Decimal,
            pool_contribution_percentage_fee: Decimal,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.stability_pools.set_parameters(
                    default_payout_split,
                    default_liquidity_rewards_split,
                    default_stability_pool_split,
                    default_pool_buy_price_modifier,
                    pool_contribution_flat_fee,
                    pool_contribution_percentage_fee,
                );
            })
        }

        /// ADMIN: Sets the panic mode parameters for the StabilityPools component.
        /// Requires OWNER authorization on the Proxy.
        /// Calls `StabilityPools::set_panic_mode_parameters`.
        ///
        /// # Arguments
        /// * `wait_period`: The duration (minutes) a CDP must be pending liquidation before panic mode can be activated.
        /// * `cooldown_period`: The duration (minutes) after the last panic mode liquidation before panic mode automatically deactivates.
        /// * `lowest_interest_history_length`: Number of lowest interest rate samples to keep for history.
        /// * `lowest_interest_interval`: The interval (in minutes) between updates to the lowest interest rate history.
        pub fn set_panic_mode_parameters(
            &self,
            wait_period: i64,
            cooldown_period: i64,
            lowest_interest_history_length: u64,
            lowest_interest_interval: i64,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.stability_pools.set_panic_mode_parameters(
                    wait_period,
                    cooldown_period,
                    lowest_interest_history_length,
                    lowest_interest_interval,
                );
            })
        }

        //==================================================================
        //                    Payout Component
        //==================================================================

        /// Sets the payment details (token address and required amount) on the PayoutComponent.
        /// Requires OWNER authorization (Proxy's admin badge).
        ///
        /// # Arguments
        /// * `new_payment_token_address`: The new `ResourceAddress` for payment.
        /// * `new_required_payment_amount`: The new `Decimal` amount required for payment.
        pub fn payout_set_parameters(
            &mut self,
            new_required_payment_amount: Decimal,
            burn: bool,
        ) {
            self.badge_vault.authorize_with_amount(dec!(1.0), || {
                 self.payout_component.set_parameters(
                    new_required_payment_amount,
                    burn
                )
            });
        }
        //                    COMBINATIONS OF COMPONENTS
        //==================================================================

        /// ADMIN: Adds a new collateral type to both the Flux and StabilityPools components.
        /// Requires OWNER authorization on the Proxy.
        /// Calls `Flux::new_collateral` and `StabilityPools::new_pool`.
        ///
        /// # Arguments
        /// * `address`: The `ResourceAddress` of the new collateral token.
        /// * `chosen_mcr`: The Minimum Collateral Ratio (`Decimal`) for the new collateral in Flux.
        /// * `initial_price`: The initial USD price (`Decimal`) for the new collateral in Flux.
        /// * `payout_split`: Optional payout split override for the new stability pool.
        /// * `liquidity_rewards_split`: Optional liquidity rewards split override for the new stability pool.
        /// * `stability_pool_split`: Optional stability pool split override for the new stability pool.
        /// * `allow_pool_buys`: Whether to allow direct buys from the new stability pool.
        /// * `pool_buy_price_modifier`: Optional price modifier for direct buys from the new stability pool.
        /// * `pool_name`: Name for the new stability pool's unit token.
        /// * `pool_description`: Description for the new stability pool's unit token.
        /// * `pool_icon_url`: Icon URL for the new stability pool's unit token.
        /// * `pool_token_symbol`: Symbol for the new stability pool's unit token.
        pub fn new_collateral(
            &mut self,
            address: ResourceAddress,
            chosen_mcr: Decimal,
            initial_price: Decimal,
            payout_split: Option<Decimal>,
            liquidity_rewards_split: Option<Decimal>,
            stability_pool_split: Option<Decimal>,
            allow_pool_buys: bool,
            pool_buy_price_modifier: Option<Decimal>,
            pool_name: String,
            pool_description: String,
            pool_icon_url: Url,
            pool_token_symbol: String,
        ) {
            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux
                    .new_collateral(address, chosen_mcr, initial_price)
            });

            let pool_unit_resource_address = self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.stability_pools.new_pool(
                    address,
                    payout_split,
                    liquidity_rewards_split,
                    stability_pool_split,
                    allow_pool_buys,
                    pool_buy_price_modifier,
                    pool_name,
                    pool_description,
                    pool_icon_url,
                    pool_token_symbol,
                    GlobalAddress::from(self.dapp_def_account.address()),
                )
            });

            if let Ok(Some(mut claimed_entities)) = self.dapp_def_account.get_metadata::<&str, Vec<GlobalAddress>>("claimed_entities") {
                claimed_entities.push(GlobalAddress::from(pool_unit_resource_address));
                let _ = self.dapp_def_account.set_metadata("claimed_entities", claimed_entities);
            }
        }
    }
}
