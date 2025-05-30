use scrypto::prelude::*;
// Import StabilityPools if you need its type, otherwise AnyComponent might suffice for calling
use crate::stability_pools::stability_pools::StabilityPools;
use crate::events::*;

#[blueprint]
#[events(
    PayoutClaimEvent,
    PayoutFetchRewardsEvent,
    PayoutRequirementUpdateEvent,
)]
mod payout_component {
    enable_method_auth! {
        methods {
            // Public methods
            claim_rewards => PUBLIC;
            fetch_rewards_from_stability_pools => PUBLIC;
            receive_badges => PUBLIC;
            receive_rewards => PUBLIC;
            // Restricted methods
            take_payout_component_rewards => restrict_to: [OWNER];
            set_parameters => restrict_to: [OWNER];
            send_badges => restrict_to: [OWNER];
            take_payments => restrict_to: [OWNER];
        }
    }

    /// Manages the distribution of accumulated fUSD rewards from the protocol.
    /// Allows claiming rewards by paying a specified token amount or direct withdrawal via badge auth.
    struct PayoutComponent {
        /// Vault holding the controller badge, granting admin privileges.
        controller_badge_vault: FungibleVault,
        /// Vault holding the accumulated fUSD rewards.
        fusd_vault: FungibleVault,
        /// The resource address of the token required as payment to claim rewards.
        payment_token_vault: FungibleVault,
        /// The amount of the payment token required to claim rewards.
        required_payment_amount: Decimal,
        /// The resource address of the stability pools component.
        stability_pools_address: ComponentAddress,
        /// Whether to burn the payment token after claiming rewards.
        burn: bool,
    }

    impl PayoutComponent {
        /// Instantiates the PayoutComponent.
        ///
        /// # Arguments
        /// * `controller_badge`: A bucket containing the controller badge for authorization.
        /// * `payment_token_address`: The resource address of the token required for payment.
        /// * `initial_required_payment_amount`: The initial amount of the payment token required.
        /// * `fusd_address`: The resource address of the fUSD token.
        /// * `owner_role`: The OwnerRole for the component (should be rule!(require(controller_badge.resource_address()))).
        /// * `dapp_def_address`: The DApp definition address for metadata.
        ///
        /// # Returns
        /// * `Global<PayoutComponent>`: A global reference to the new component.
        pub fn instantiate(
            controller_badge: Bucket,
            payment_token_address: ResourceAddress,
            initial_required_payment_amount: Decimal,
            fusd_address: ResourceAddress,
            stability_pools_address: ComponentAddress,
            owner_role: OwnerRole, // Expecting rule!(require(controller_badge.resource_address()))
            dapp_def_address: GlobalAddress,
        ) -> Global<PayoutComponent> {
            let (address_reservation, _component_address) =
                Runtime::allocate_component_address(PayoutComponent::blueprint_id());
            

            assert!(
                initial_required_payment_amount > Decimal::ZERO,
                "Required payment amount must be positive"
            );

            Self {
                controller_badge_vault: FungibleVault::with_bucket(controller_badge.as_fungible()),
                fusd_vault: FungibleVault::new(fusd_address),
                payment_token_vault: FungibleVault::new(payment_token_address),
                required_payment_amount: initial_required_payment_amount,
                stability_pools_address,
                burn: true,
            }
            .instantiate()
            .prepare_to_globalize(owner_role)
            .with_address(address_reservation)
            .metadata(metadata! {
                init {
                    "name" => "Flux Payout Component".to_string(), updatable;
                    "description" => "Handles distribution of protocol fUSD rewards.".to_string(), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                    "info_url" => Url::of("https://flux.ilikeitstable.com"), updatable;
                    "icon_url" => Url::of("https://flux.ilikeitstable.com/flux-logo.png"), updatable;
                }
            })
            .globalize()
        }

        /// Allows a user to claim all available fUSD rewards by providing the required payment token amount.
        ///
        /// The provided payment tokens are burned.
        ///
        /// # Arguments
        /// * `payment_bucket`: A bucket containing the payment tokens.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing:
        ///     1. A bucket with all the claimed fUSD rewards.
        ///     2. A bucket with any remaining payment tokens (if more than required was provided).
        ///
        /// # Panics
        /// * If the `payment_bucket` resource does not match the configured `payment_token_address`.
        /// * If the `payment_bucket` amount is less than `required_payment_amount`.
        pub fn claim_rewards(&mut self, mut payment_bucket: Bucket) -> (Bucket, Bucket) {
            assert!(
                payment_bucket.amount() >= self.required_payment_amount,
                "Insufficient payment amount provided. Required: {}",
                self.required_payment_amount
            );

            if self.burn {
                payment_bucket.take(self.required_payment_amount).burn();
            } else {
                self.payment_token_vault.put(payment_bucket.take(self.required_payment_amount).as_fungible());
            }

            self.fetch_rewards_from_stability_pools();

            // Take all accumulated fUSD rewards
            let fusd_rewards = self.fusd_vault.take_all();

            // Emit claim event
            Runtime::emit_event(PayoutClaimEvent {
                amount: fusd_rewards.amount(),
            });

            (fusd_rewards.into(), payment_bucket)
        }

        /// Allows the component owner (holding the badge) to withdraw all accumulated fUSD rewards directly.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing all the fUSD rewards from the vault.
        pub fn take_payout_component_rewards(&mut self) -> Bucket {
            self.fusd_vault.take_all().into()
        }

        /// Allows the component owner (holding the badge) to withdraw all accumulated payment tokens directly.
        pub fn take_payments(&mut self) -> Bucket {
            self.payment_token_vault.take_all().into()
        }

        /// Sets the required payment token and amount needed to claim rewards.
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `new_payment_token_address`: The new `ResourceAddress` of the required payment token.
        /// * `new_required_payment_amount`: The new `Decimal` amount required.
        ///
        /// # Panics
        /// * If `new_required_payment_amount` is not positive.
        pub fn set_parameters(
            &mut self,
            new_required_payment_amount: Decimal,
            burn: bool,
        ) {
            assert!(
                new_required_payment_amount > Decimal::ZERO,
                "Required payment amount must be positive"
            );
            self.required_payment_amount = new_required_payment_amount;
            self.burn = burn;
            // Emit requirement update event
            Runtime::emit_event(PayoutRequirementUpdateEvent {
                new_requirement: new_required_payment_amount,
                burn,
            });
        }

        /// Sends controller badges to another component.
        ///
        /// # Arguments
        /// * `amount`: The `Decimal` amount of controller badges to send.
        /// * `receiver_address`: The `ComponentAddress` of the recipient.
        pub fn send_badges(&mut self, amount: Decimal, receiver_address: ComponentAddress) {
            let receiver: Global<AnyComponent> = Global::from(receiver_address);
            let badge_bucket: Bucket = self.controller_badge_vault.take(amount).into();
            receiver.call_raw("receive_badges", scrypto_args!(badge_bucket))
        }

        /// Allows the Proxy component (specifically its OWNER) to receive controller badges.
        /// These badges are likely minted by the `Flux` component and sent here for the Proxy
        /// to use when authorizing calls to other components.
        /// Requires OWNER authorization on the Proxy.
        ///
        /// # Arguments
        /// * `badge_bucket`: A `Bucket` containing controller badges (`fusdCTRL`).
        pub fn receive_badges(&mut self, badge_bucket: Bucket) {
            self.controller_badge_vault.put(badge_bucket.as_fungible());
        }


        /// Fetches accumulated fUSD rewards from the StabilityPools component.
        /// Requires OWNER authorization (controller badge).
        ///
        /// # Arguments
        /// * `stability_pools_address`: The `ComponentAddress` of the StabilityPools component.
        pub fn fetch_rewards_from_stability_pools(&mut self) {
            let stability_pools: Global<StabilityPools> = Global::from(self.stability_pools_address);

            let rewards_bucket: Bucket = self.controller_badge_vault.authorize_with_amount(Decimal::ONE, || {
                stability_pools.claim_payout_rewards()
            });

            // Emit fetch rewards event
            Runtime::emit_event(PayoutFetchRewardsEvent {
                amount: rewards_bucket.amount(),
            });

            self.fusd_vault.put(rewards_bucket.as_fungible());
        }

        /// Receives fUSD rewards directly. Used primarily for testing.
        /// 
        /// # Arguments
        /// * `rewards`: A bucket containing fUSD rewards to be added to the component.
        pub fn receive_rewards(&mut self, rewards: Bucket) {
            assert!(
                rewards.resource_address() == self.fusd_vault.resource_address(),
                "Invalid rewards token"
            );
            self.fusd_vault.put(rewards.as_fungible());
        }
    }
} 