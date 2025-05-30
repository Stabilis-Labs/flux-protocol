#![allow(deprecated)]

//! # fUSD Flash Loan Blueprint
//!
//! This blueprint defines a component that allows users to take out flash loans of fUSD tokens
//! from the main Flux protocol component.
//! Flash loans are uncollateralized loans that must be borrowed and repaid within the same transaction.
//!
//! ## Functionality
//! - Users borrow fUSD and receive a transient `LoanReceipt` NFT.
//! - This `LoanReceipt` NFT can only be burned by this `FlashLoans` component.
//! - The borrower *must* call the `pay_back` method within the same transaction, providing sufficient fUSD
//!   (borrowed amount + interest) to repay the loan and allow the `LoanReceipt` to be burned.
//! - Failure to repay within the same transaction results in the transaction failing.
//! - Collected interest can be retrieved by an authorized party.

use crate::flux_component::flux_component::*;
use scrypto::prelude::*;

/// Represents the non-fungible data associated with a flash loan receipt.
/// This NFT is transient and must be returned (burned) in the same transaction it was minted.
#[derive(ScryptoSbor, NonFungibleData)]
pub struct LoanReceipt {
    /// The amount of fUSD originally borrowed. This field is mutable, though not modified in the current implementation.
    #[mutable]
    pub borrowed_amount: Decimal,
    /// The interest rate (e.g., 0.05 for 5%) applicable to this loan at the time of borrowing.
    pub interest: Decimal,
}

#[blueprint]
#[types(LoanReceipt)]
mod flash_loans {
    enable_method_auth! {
        methods {
            borrow => restrict_to: [OWNER];
            settings => restrict_to: [OWNER];
            pay_back => restrict_to: [OWNER];
            retrieve_interest => restrict_to: [OWNER];
        }
    }

    /// Contains the state and logic for the fUSD Flash Loan component.
    struct FlashLoans {
        /// Holds the controller badge required to authorize minting and burning fUSD via the main `Flux` component.
        badge_vault: FungibleVault,
        /// Manages the creation and burning of the transient `LoanReceipt` NFTs.
        loan_receipt_manager: ResourceManager,
        /// An optional vault holding the accumulated fUSD interest payments. Created on first interest payment.
        interest_vault: Option<Vault>,
        /// A counter used to generate unique IDs for each `LoanReceipt` NFT.
        loan_receipt_counter: u64,
        /// The current interest rate charged on new flash loans (e.g., 0.05 represents 5%).
        interest: Decimal,
        /// A global reference to the main `Flux` component, used for minting/burning fUSD.
        flux: Global<Flux>,
        /// A flag indicating whether flash loans are currently enabled.
        enabled: bool,
        /// Tracks the total amount of fUSD currently loaned out via active flash loans (within transactions).
        /// Note: This might not be strictly necessary given the transient nature but can be useful for monitoring.
        amount_loaned: Decimal,
    }

    impl FlashLoans {
        /// Instantiates the `FlashLoans` component.
        ///
        /// # Arguments
        /// * `controller_badge`: A `Bucket` containing the controller badge NFT from the `Flux` component.
        ///                      This badge authorizes actions like minting/burning fUSD on the `Flux` component.
        /// * `flux`: A `Global<Flux>` reference to the deployed `Flux` component instance.
        /// * `dapp_def_address`: The `GlobalAddress` of the DApp Definition account for metadata linkage.
        ///
        /// # Returns
        /// * `Global<FlashLoans>`: A global reference to the newly instantiated `FlashLoans` component.
        ///
        /// # Logic
        /// 1. Allocates a component address.
        /// 2. Creates a `ResourceManager` for the `LoanReceipt` NFTs.
        ///    - Configures the resource manager roles:
        ///      - `OwnerRole`: Requires the Flux controller badge for potential future ownership actions.
        ///      - `non_fungible_data_updater`: Only this component can update the (mutable) data.
        ///      - `minter`: Only this component can mint new receipts.
        ///      - `burner`: Only this component can burn receipts.
        ///      - `depositor`: Set to `rule!(deny_all)` to make the receipts **transient**. They cannot be deposited
        ///        into user accounts and must be dealt with (i.e., returned to `pay_back`) within the same transaction.
        /// 3. Sets up metadata for the `LoanReceipt` resource (name, symbol, description, etc.).
        /// 4. Instantiates the `FlashLoans` component state with initial values (interest=0, disabled=false, counter=0, etc.).
        /// 5. Sets component metadata (name, description, info_url, dapp_definition).
        /// 6. Globalizes the component.
        pub fn instantiate(
            controller_badge: Bucket,
            flux: Global<Flux>,
            dapp_def_address: GlobalAddress,
        ) -> Global<FlashLoans> {
            let (address_reservation, component_address) =
                Runtime::allocate_component_address(FlashLoans::blueprint_id());

            let loan_receipt_manager: ResourceManager =
                <scrypto::prelude::ResourceBuilder as flash_loans::flash_loans::FlashLoansResourceBuilder>::new_integer_non_fungible_with_registered_type::<LoanReceipt>(OwnerRole::Fixed(rule!(
                    require_amount(dec!("0.75"), controller_badge.resource_address())
                )))
                .metadata(metadata!(
                    init {
                        "name" => "fUSD Flash Loan Receipt", locked;
                        "symbol" => "fusdFLASH", locked;
                        "description" => "A receipt for your fUSD flash loan", locked;
                        "info_url" => "https://flux.ilikeitstable.com", updatable;
                    }
                ))
                .non_fungible_data_update_roles(non_fungible_data_update_roles!(
                    non_fungible_data_updater => rule!(require(global_caller(component_address)));
                    non_fungible_data_updater_updater => rule!(deny_all);
                ))
                .mint_roles(mint_roles!(
                    minter => rule!(require(global_caller(component_address)));
                    minter_updater => rule!(deny_all);
                ))
                .burn_roles(burn_roles!(
                    burner => rule!(require(global_caller(component_address)));
                    burner_updater => rule!(deny_all);
                ))
                .deposit_roles(deposit_roles!(
                    depositor => rule!(deny_all);
                    depositor_updater => rule!(deny_all);
                ))
                .create_with_no_initial_supply()
                .into();

            let controller_address: ResourceAddress = controller_badge.resource_address();

            //create the flash loan component
            Self {
                badge_vault: FungibleVault::with_bucket(controller_badge.as_fungible()),
                loan_receipt_manager,
                interest: dec!(0),
                interest_vault: None,
                flux,
                loan_receipt_counter: 0,
                enabled: true,
                amount_loaned: dec!(0),
            }
            .instantiate()
            .prepare_to_globalize(OwnerRole::Fixed(rule!(require(controller_address))))
            .with_address(address_reservation)
            .metadata(metadata! {
                init {
                    "name" => "fUSD Flash Loans".to_string(), updatable;
                    "description" => "A component for flash loans of fUSD tokens".to_string(), updatable;
                    "info_url" => Url::of("https://flux.ilikeitstable.com"), updatable;
                    "icon_url" => Url::of("https://flux.ilikeitstable.com/flux-logo.png"), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                }
            })
            .globalize()
        }

        /// Alters the settings of the FlashLoans component.
        /// Allows updating the interest rate and enabling/disabling flash loans.
        ///
        /// # Arguments
        /// * `interest`: The new interest rate to be charged on flash loans (e.g., 0.05 for 5%).
        /// * `enabled`: A boolean flag to enable (`true`) or disable (`false`) the flash loan functionality.
        pub fn settings(&mut self, interest: Decimal, enabled: bool) {
            self.interest = interest;
            self.enabled = enabled;
        }

        /// Takes out a flash loan of fUSD tokens.
        ///
        /// This method allows a user (authorized via OWNER role) to borrow fUSD. It mints the requested fUSD
        /// and a transient `LoanReceipt` NFT which must be returned to the `pay_back` method in the same transaction.
        ///
        /// # Arguments
        /// * `amount`: The `Decimal` amount of fUSD tokens to borrow.
        ///
        /// # Returns
        /// * `(Bucket, Bucket)`: A tuple containing:
        ///     1. The borrowed fUSD tokens in a `Bucket`.
        ///     2. The transient `LoanReceipt` NFT in a `Bucket`.
        ///
        /// # Panics
        /// * If flash loans are currently disabled (`self.enabled == false`).
        ///
        /// # Logic
        /// 1. Asserts that flash loans are enabled.
        /// 2. Increments `amount_loaned` (for tracking).
        /// 3. Creates the `LoanReceipt` data with the borrowed amount and current interest rate.
        /// 4. Mints a new `LoanReceipt` NFT using the `loan_receipt_manager` and the current `loan_receipt_counter`.
        /// 5. Increments the `loan_receipt_counter`.
        /// 6. Authorizes the `Flux` component (using the controller badge) to mint the requested `amount` of fUSD via `free_fusd`.
        /// 7. Returns the bucket of borrowed fUSD and the bucket containing the `LoanReceipt` NFT.
        pub fn borrow(&mut self, amount: Decimal) -> (Bucket, Bucket) {
            assert!(self.enabled, "Flash loans are disabled.");
            self.amount_loaned += amount;
            let loan_receipt = LoanReceipt {
                borrowed_amount: amount,
                interest: self.interest,
            };

            let receipt: Bucket = self.loan_receipt_manager.mint_non_fungible(
                &NonFungibleLocalId::integer(self.loan_receipt_counter),
                loan_receipt,
            );
            self.loan_receipt_counter += 1;

            let loan_bucket: Bucket = self
                .badge_vault
                .authorize_with_amount(dec!("0.75"), || self.flux.free_fusd(amount));

            (loan_bucket, receipt)
        }

        /// Pays back the fUSD tokens borrowed in a flash loan.
        ///
        /// This method **must** be called in the same transaction as the `borrow` method that issued the `receipt_bucket`.
        /// It verifies the payment, burns the principal amount of fUSD, collects interest, burns the transient `LoanReceipt`,
        /// and returns any excess payment.
        ///
        /// # Arguments
        /// * `receipt_bucket`: The `Bucket` containing the transient `LoanReceipt` NFT received from the `borrow` method.
        /// * `payment`: A `Bucket` containing the fUSD tokens intended to pay back the loan principal plus interest.
        ///
        /// # Returns
        /// * `Bucket`: Any remaining fUSD from the `payment` bucket after the loan and interest have been settled.
        ///
        /// # Panics
        /// * If the `receipt_bucket` does not contain a valid `LoanReceipt` managed by this component.
        /// * If the `payment` amount is less than the required amount (borrowed amount + interest).
        ///
        /// # Logic
        /// 1. Asserts the `receipt_bucket`'s resource address matches the `loan_receipt_manager`'s address.
        /// 2. Retrieves the `LoanReceipt` data associated with the NFT in the `receipt_bucket`.
        /// 3. Calculates the total required payment (principal + interest).
        /// 4. Asserts that the `payment` bucket contains at least the required amount.
        /// 5. Authorizes the `Flux` component (using the controller badge) to burn the principal amount (`receipt.borrowed_amount`) from the `payment` bucket via `burn_fusd`.
        /// 6. Checks if interest is due (`receipt.interest > 0`).
        ///    - If yes, takes the calculated interest amount (`receipt.interest * receipt.borrowed_amount`) from the `payment` bucket.
        ///    - Initializes the `interest_vault` if it doesn't exist, or puts the interest payment into the existing vault.
        /// 7. Burns the `receipt_bucket` (containing the transient `LoanReceipt` NFT). This is allowed because the component is the designated burner.
        /// 8. Returns the `payment` bucket, which now contains only the excess fUSD provided by the user (if any).
        pub fn pay_back(&mut self, receipt_bucket: Bucket, mut payment: Bucket) -> Bucket {
            assert!(
                receipt_bucket.resource_address() == self.loan_receipt_manager.address(),
                "Invalid receipt"
            );

            let receipt: LoanReceipt = self
                .loan_receipt_manager
                .get_non_fungible_data(&receipt_bucket.as_non_fungible().non_fungible_local_id());

            assert!(
                payment.amount() >= receipt.borrowed_amount * (dec!(1) + receipt.interest),
                "Not enough fUSD paid back."
            );

            self.badge_vault.authorize_with_amount(dec!("0.75"), || {
                self.flux.burn_fusd(payment.take(receipt.borrowed_amount))
            });

            if receipt.interest > dec!(0) {
                if self.interest_vault.is_none() {
                    self.interest_vault = Some(Vault::with_bucket(
                        payment.take(receipt.interest * receipt.borrowed_amount),
                    ));
                } else {
                    self.interest_vault
                        .as_mut()
                        .unwrap()
                        .put(payment.take(receipt.interest * receipt.borrowed_amount));
                }
            }

            receipt_bucket.burn();

            payment
        }

        /// Allows an authorized user (OWNER) to retrieve the accumulated interest from the `interest_vault`.
        ///
        /// # Returns
        /// * `Bucket`: A bucket containing all the fUSD collected as interest payments.
        ///
        /// # Panics
        /// * If the `interest_vault` has not been initialized yet (i.e., no interest-bearing loans have been paid back).
        pub fn retrieve_interest(&mut self) -> Bucket {
            self.interest_vault.as_mut().expect("Interest vault not initialized.").take_all()
        }
    }
}
