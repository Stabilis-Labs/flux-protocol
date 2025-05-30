#![allow(dead_code)]

use flux_protocol::flux_component::flux_component_test::*;
use flux_protocol::flash_loans::flash_loans_test::*;
use flux_protocol::stability_pools::stability_pools_test::*;
use flux_protocol::proxy::proxy_test::*;
use flux_protocol::shared_structs::*;
use dummy_oracle_component::oracle_test::*;
use scrypto_test::prelude::*;
use scrypto::prelude::Url;
use flux_protocol::payout_component::payout_component_test::*;

pub struct Helper {
    pub env: TestEnvironment<InMemorySubstateDatabase>,
    pub package_address: PackageAddress,
    pub admin: Bucket,
    pub usdc: Bucket,
    pub xrd: Bucket,
    pub lsulp: Bucket,
    pub lsulp_address: ResourceAddress,
    pub admin_address: ResourceAddress,
    pub xrd_address: ResourceAddress,
    pub usdc_address: ResourceAddress,
    pub proxy: Proxy,
    pub flux: Flux,
    pub flash_loans: FlashLoans,
    pub stability_pools: StabilityPools,
    pub dummy_oracle: Oracle,
    pub payout_component: PayoutComponent,
}

impl Helper {
    pub fn new() -> Result<Self, RuntimeError> {
        let mut env = TestEnvironmentBuilder::new()
            .build();

        let lsulp = ResourceBuilder::new_fungible(OwnerRole::None)
            .divisibility(18)
            .mint_initial_supply(1000000, &mut env)?;
        let xrd = ResourceBuilder::new_fungible(OwnerRole::None)
            .divisibility(18)
            .mint_initial_supply(1000000, &mut env)?;
        let usdc = ResourceBuilder::new_fungible(OwnerRole::None)
            .divisibility(18)
            .mint_initial_supply(1000000, &mut env)?;
        let admin = ResourceBuilder::new_fungible(OwnerRole::None)
            .divisibility(18)
            .mint_initial_supply(1000000, &mut env)?;

        let lsulp_address = lsulp.resource_address(&mut env)?;
        let xrd_address = xrd.resource_address(&mut env)?;
        let usdc_address = usdc.resource_address(&mut env)?;
        let admin_address = admin.resource_address(&mut env)?;

        let dummy_oracle_package_address = PackageFactory::compile_and_publish(
            "./dummy_oracle_component",
            &mut env,
            CompileProfile::Standard,
        )?;

        let dummy_oracle = Oracle::instantiate_oracle(
            xrd_address,
            lsulp_address,
            dummy_oracle_package_address,
            &mut env
        )?;

        let package_address = PackageFactory::compile_and_publish(
            this_package!(),
            &mut env,
            CompileProfile::Standard,
        )?;

        let (
            mut proxy,
            flux,
            flash_loans,
            stability_pools,
            payout_component,
        ) = Proxy::new(
            admin_address,
            ComponentAddress::try_from(dummy_oracle.0.clone()).unwrap(),
            usdc_address,
            xrd_address,
            dec!(100),
            xrd_address,
            package_address,
            &mut env,
        )?;

        env.disable_auth_module();

        proxy.new_collateral(
            xrd_address,
            dec!(2),
            dec!(1),
            None,
            None,
            None,
            true,
            None,
            "XRD".to_string(),
            "XRD".to_string(),
            Url::of("https://ilikeitstable.com"),
            "XRDfUSD".to_string(),
            &mut env
        )?;

        proxy.new_collateral(
            lsulp_address,
            dec!(2),
            dec!(1),
            None,
            None,
            None,
            true,
            None,
            "LSULP".to_string(),
            "LSULP".to_string(),
            Url::of("https://ilikeitstable.com"),
            "LSULPfUSD".to_string(),
            &mut env
        )?;

        proxy.set_panic_mode_parameters(
            1440,
            1440,
            10,
            30,
            &mut env
        );

        env.enable_auth_module();

        Ok(Self {
            env,
            package_address,
            admin: admin.into(),
            usdc: usdc.into(),
            xrd: xrd.into(),
            lsulp: lsulp.into(),
            lsulp_address,
            admin_address,
            xrd_address,
            usdc_address,
            proxy,
            flux: Flux(*flux.as_node_id()),
            flash_loans: FlashLoans(*flash_loans.as_node_id()),
            stability_pools: StabilityPools(*stability_pools.handle.as_node_id()),
            dummy_oracle: Oracle(dummy_oracle.0),
            payout_component: PayoutComponent(*payout_component.as_node_id()),
        })
    }

    /////////////////////////////////////////////////
    //////////////////// PROXY///////////////////////
    /////////////////////////////////////////////////

    pub fn proxy_open_cdp(
        &mut self,
        privileged_borrower_proof: Option<NonFungibleProof>,
        collateral: Bucket,
        amount: Decimal,
        interest: Decimal,
    ) -> Result<(Bucket, Bucket), RuntimeError> {
        let (fusd, cdp_receipt) = self.proxy.open_cdp(
            privileged_borrower_proof,
            collateral,
            amount,
            interest,
            "".to_string(),
            "".to_string(),
            &mut self.env
        )?;

        Ok((fusd, cdp_receipt))
    }

    /////////////////////////////////////////////////
    ///////////////// ERSATZ GETTERS ////////////////
    /////////////////////////////////////////////////

    pub fn get_cdp_info(&mut self, cdp_id: NonFungibleLocalId) -> Result<(NonFungibleLocalId, Cdp, Decimal), RuntimeError> {
        let cdp_infos = self.flux.get_cdps_info(vec![cdp_id], &mut self.env)?;
        
        Ok(cdp_infos.first().unwrap().clone())
    }

    pub fn get_cdps_info(&mut self, cdp_ids: Vec<NonFungibleLocalId>) -> Result<Vec<(NonFungibleLocalId, Cdp, Decimal)>, RuntimeError> {
        Ok(self.flux.get_cdps_info(cdp_ids, &mut self.env)?)
    }

    /////////////////////////////////////////////////
    //////////////////// TEST HELPERS ///////////////
    /////////////////////////////////////////////////

    pub fn set_allow_multiple_actions(&mut self, allow: bool) -> Result<(), RuntimeError> {
        self.env.disable_auth_module();
        self.stability_pools.set_allow_multiple_actions(allow, &mut self.env)?;
        self.env.enable_auth_module();

        Ok(())
    }

    pub fn change_collateral_price(&mut self, collateral_identifier: String, price: Decimal) -> Result<(), RuntimeError> {
        self.env.disable_auth_module();
        self.dummy_oracle.set_price(collateral_identifier, price, &mut self.env)?;
        self.env.enable_auth_module();

        Ok(())
    }

    pub fn create_account(&mut self) -> Result<Reference, RuntimeError> {
        let account = self
            .env
            .call_function_typed::<_, AccountCreateOutput>(
                ACCOUNT_PACKAGE,
                ACCOUNT_BLUEPRINT,
                ACCOUNT_CREATE_IDENT,
                &AccountCreateInput {},
            )?
            .0;
        Ok(account.0.into())
    }

    pub fn withdraw_from_account(
        &mut self,
        account: Reference,
        resource_address: ResourceAddress,
        amount: Decimal,
    ) -> Result<Bucket, RuntimeError> {
        let bucket = self.env.call_method_typed::<_, _, AccountWithdrawOutput>(
            account.as_node_id().clone(),
            ACCOUNT_WITHDRAW_IDENT,
            &AccountWithdrawInput {
                resource_address,
                amount,
            },
        )?;

        Ok(bucket)
    }

    pub fn withdraw_nft_from_account(
        &mut self,
        account: Reference,
        resource_address: ResourceAddress,
        id: NonFungibleLocalId,
    ) -> Result<Bucket, RuntimeError> {
        let mut ids: IndexSet<NonFungibleLocalId> = IndexSet::new();
        ids.insert(id);
        let bucket = self
            .env
            .call_method_typed::<_, _, AccountWithdrawNonFungiblesOutput>(
                account.as_node_id().clone(),
                ACCOUNT_WITHDRAW_NON_FUNGIBLES_IDENT,
                &AccountWithdrawNonFungiblesInput {
                    resource_address,
                    ids,
                },
            )?;

        Ok(bucket)
    }

    pub fn assert_bucket_eq(
        &mut self,
        bucket: &Bucket,
        address: ResourceAddress,
        amount: Decimal,
    ) -> Result<(), RuntimeError> {
        assert_eq!(bucket.resource_address(&mut self.env)?, address);
        assert_eq!(bucket.amount(&mut self.env)?, amount);

        Ok(())
    }

    pub fn free_fusd(&mut self, amount: Decimal) -> Result<Bucket, RuntimeError> {
        let free_fusd = self.flux.free_fusd(amount, &mut self.env)?;
        Ok(free_fusd)
    }
}
