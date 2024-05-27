use crate::amm;
use crate::common;

use amm::{utils::AmmKeys, utils::CalculateResult, MarketPubkeys};
use common::rpc;

use anyhow::Result;
use arrayref::array_ref;
use raydium_amm::{
    math::Calculator,
    processor,
    processor::Processor,
    state::{AmmInfo, AmmStatus, TargetOrders},
};
use safe_transmute::{to_bytes::transmute_to_bytes, transmute_one_pedantic};
use solana_client::rpc_client::RpcClient;
use solana_program::{
    account_info::{AccountInfo, IntoAccountInfo},
    program_pack::Pack,
};
use solana_sdk::pubkey::Pubkey;
use spl_token::state::Account;

pub const TEN_THOUSAND: u64 = 10000;

pub fn calculate_pool_vault_amounts(
    client: &RpcClient,
    amm_program_key: &Pubkey,
    amm_pool_key: &Pubkey,
    amm_keys: &AmmKeys,
    market_keys: &MarketPubkeys,
) -> Result<CalculateResult> {
    let load_pubkeys: Vec<Pubkey> = vec![
        *amm_pool_key,
        amm_keys.amm_target,
        amm_keys.amm_pc_vault,
        amm_keys.amm_coin_vault,
        amm_keys.amm_open_order,
        amm_keys.market,
        *market_keys.event_q,
    ];
    let rsps = rpc::get_multiple_accounts(client, &load_pubkeys)?;
    let accounts = array_ref![rsps, 0, 7];
    let [_, amm_target_account, amm_pc_vault_account, amm_coin_vault_account, amm_open_orders_account, market_account, market_event_q_account] =
        accounts;
    let amm = rpc::get_account::<AmmInfo>(client, &amm_pool_key)?.unwrap();
    let _amm_target: TargetOrders = transmute_one_pedantic::<TargetOrders>(transmute_to_bytes(
        &amm_target_account.as_ref().unwrap().clone().data,
    ))
    .map_err(|e| e.without_src())?;
    let amm_pc_vault =
        Account::unpack(&amm_pc_vault_account.as_ref().unwrap().clone().data).unwrap();
    let amm_coin_vault =
        Account::unpack(&amm_coin_vault_account.as_ref().unwrap().clone().data).unwrap();
    let (amm_pool_key_pc_vault_amount, amm_pool_key_coin_vault_amount) =
        if AmmStatus::from_u64(amm.status).orderbook_permission() {
            let amm_open_orders_account = &mut amm_open_orders_account.as_ref().unwrap().clone();
            let market_account = &mut market_account.as_ref().unwrap().clone();
            let market_event_q_account = &mut market_event_q_account.as_ref().unwrap().clone();

            let amm_open_orders_info =
                (&amm.open_orders, amm_open_orders_account).into_account_info();
            let market_account_info = (&amm.market, market_account).into_account_info();
            let market_event_queue_info =
                (&(*market_keys.event_q), market_event_q_account).into_account_info();

            let amm_authority =
                Pubkey::find_program_address(&[processor::AUTHORITY_AMM], &amm_program_key).0;
            let lamports = &mut 0;
            let data = &mut [0u8];
            let owner = Pubkey::default();
            let amm_authority_info = AccountInfo::new(
                &amm_authority,
                false,
                false,
                lamports,
                data,
                &owner,
                false,
                0,
            );
            let (market_state, open_orders) = Processor::load_serum_market_order(
                &market_account_info,
                &amm_open_orders_info,
                &amm_authority_info,
                &amm,
                false,
            )?;
            let (amm_pool_key_pc_vault_amount, amm_pool_key_coin_vault_amount) =
                Calculator::calc_total_without_take_pnl(
                    amm_pc_vault.amount,
                    amm_coin_vault.amount,
                    &open_orders,
                    &amm,
                    &market_state,
                    &market_event_queue_info,
                    &amm_open_orders_info,
                )?;
            (amm_pool_key_pc_vault_amount, amm_pool_key_coin_vault_amount)
        } else {
            let (amm_pool_key_pc_vault_amount, amm_pool_key_coin_vault_amount) =
                Calculator::calc_total_without_take_pnl_no_orderbook(
                    amm_pc_vault.amount,
                    amm_coin_vault.amount,
                    &amm,
                )?;
            (amm_pool_key_pc_vault_amount, amm_pool_key_coin_vault_amount)
        };
    Ok(CalculateResult {
        pool_pc_vault_amount: amm_pool_key_pc_vault_amount,
        pool_coin_vault_amount: amm_pool_key_coin_vault_amount,
        pool_lp_amount: amm.lp_amount,
        swap_fee_numerator: amm.fees.swap_fee_numerator,
        swap_fee_denominator: amm.fees.swap_fee_denominator,
    })
}
