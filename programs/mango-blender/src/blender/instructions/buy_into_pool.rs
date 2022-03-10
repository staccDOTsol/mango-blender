use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, MintTo, Token, TokenAccount};
use fixed::types::I80F48;
use mango::declare_check_assert_macros;
use mango::error::{check_assert, MangoErrorCode, SourceFileId};
use mango::instruction as MangoInstructions;
use mango::state::{
    AssetType, MangoAccount, MangoCache, MangoGroup, UserActiveAssets, QUOTE_INDEX,
};
use solana_program::program::invoke_signed;
use std::convert::TryFrom;

use crate::blender::state::Pool;
use crate::helpers::*;

declare_check_assert_macros!(SourceFileId::Processor);

#[derive(Accounts)]
pub struct BuyIntoPool<'info> {
    pub mango_program: UncheckedAccount<'info>, // TODO
    #[account(mut, seeds = [pool.pool_name.as_ref(), pool.admin.as_ref()], bump)]
    pub pool: Account<'info, Pool>, // Validation??
    pub mango_group: UncheckedAccount<'info>,   // TODO
    #[account(mut)]
    pub mango_account: UncheckedAccount<'info>, // TODO
    #[account(signer)]
    pub depositor: AccountInfo<'info>,
    pub mango_cache: UncheckedAccount<'info>, // TODO
    pub root_bank: UncheckedAccount<'info>,   // TODO
    #[account(mut)]
    pub node_bank: UncheckedAccount<'info>, // TODO
    #[account(mut)]
    pub vault: UncheckedAccount<'info>, // TODO
    #[account(mut, constraint = depositor_quote_token_account.owner == depositor.key())]
    pub depositor_quote_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [pool.pool_name.as_ref(), pool.admin.as_ref(), b"iou"],
        bump = pool.iou_mint_bump,
    )]
    pub pool_iou_mint: Account<'info, Mint>,

    #[account(mut,
        associated_token::authority = depositor,
        associated_token::mint = pool_iou_mint
    )]
    pub depositor_iou_token_account: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

/// A user "buys a percentage" of the mango pool by depositing quote token into the mango pool
pub fn handler(ctx: Context<BuyIntoPool>, quantity: u64) -> ProgramResult {
    // handle deposit
    let deposit_instruction = MangoInstructions::deposit(
        ctx.accounts.mango_program.key,
        ctx.accounts.mango_group.key,
        ctx.accounts.mango_account.key,
        ctx.accounts.depositor.key,
        ctx.accounts.mango_cache.key,
        ctx.accounts.root_bank.key,
        ctx.accounts.node_bank.key,
        ctx.accounts.vault.key,
        ctx.accounts
            .depositor_quote_token_account
            .to_account_info()
            .key,
        u64::try_from(quantity).unwrap(),
    )
    .unwrap();

    let seeds = &[
        &ctx.accounts.pool.pool_name.as_ref(),
        ctx.accounts.pool.admin.as_ref(),
        &[ctx.accounts.pool.pool_bump],
    ];
    let cpi_seed = &[&seeds[..]];

    invoke_signed(
        &deposit_instruction,
        &[
            ctx.accounts.mango_program.to_account_info().clone(),
            ctx.accounts.mango_group.to_account_info().clone(),
            ctx.accounts.mango_account.to_account_info().clone(),
            ctx.accounts.depositor.to_account_info().clone(),
            ctx.accounts.mango_cache.to_account_info().clone(),
            ctx.accounts.root_bank.to_account_info().clone(),
            ctx.accounts.node_bank.to_account_info().clone(),
            ctx.accounts.vault.to_account_info().clone(),
            ctx.accounts
                .depositor_quote_token_account
                .to_account_info()
                .clone(),
        ],
        cpi_seed,
    )?;

    // load mango account, group, cache
    let mango_account_ai = ctx.accounts.mango_account.to_account_info();
    let mango_group_ai = ctx.accounts.mango_group.to_account_info();
    let mango_cache_ai = ctx.accounts.mango_cache.to_account_info();

    let mango_account = MangoAccount::load_checked(
        &mango_account_ai,
        ctx.accounts.mango_program.key,
        ctx.accounts.mango_group.key,
    )?;
    let mango_group =
        MangoGroup::load_checked(&mango_group_ai, ctx.accounts.mango_program.key).unwrap();
    let mango_cache = MangoCache::load_checked(
        &mango_cache_ai,
        ctx.accounts.mango_program.key,
        &mango_group,
    )?;

    // check that cache is valid
    let active_assets = UserActiveAssets::new(
        &mango_group,
        &mango_account,
        vec![(AssetType::Token, QUOTE_INDEX)],
    );
    let clock = Clock::get()?;
    let now_ts = clock.unix_timestamp as u64;
    mango_cache.check_valid(&mango_group, &active_assets, now_ts)?;

    //check that user is buying into pool with QUOTE
    check!(
        mango_group.tokens[QUOTE_INDEX].mint == ctx.accounts.depositor_quote_token_account.mint,
        MangoErrorCode::InvalidToken
    )?;
    let deposit_value_quote = I80F48::from_num(quantity);

    //load open orders
    let open_orders_ais =
        mango_account.checked_unpack_open_orders(&mango_group, &ctx.remaining_accounts)?;

    // calculate total value of pool at current price (including open orders)
    let pool_value_quote = calculate_pool_value(&mango_account, &mango_cache, &mango_group, open_orders_ais, &active_assets);

    // prepare iou mint 
    let mint_accounts = MintTo {
        to: ctx.accounts.depositor_iou_token_account.to_account_info(),
        mint: ctx.accounts.pool_iou_mint.to_account_info(),
        authority: ctx.accounts.pool.to_account_info(),
    };
    let token_program_ai = ctx.accounts.token_program.to_account_info();
    let iou_mint_ctx = CpiContext::new_with_signer(token_program_ai, mint_accounts, cpi_seed);

    // in case of first deposit
    if ctx.accounts.pool_iou_mint.supply == 0 {
        let mint_amount: u64 = deposit_value_quote
            .checked_floor()
            .unwrap()
            .checked_to_num()
            .unwrap();
        msg!("mint_amount: {:?}", mint_amount);

        token::mint_to(iou_mint_ctx, mint_amount)?;
    } else {
        let outstanding_iou_tokens = I80F48::from_num(ctx.accounts.pool_iou_mint.supply);
        // note that pool_value_quote is always >= deposit_value_quote, since we already deposited above
        let mint_amount: u64 = ((deposit_value_quote * outstanding_iou_tokens)
            / (pool_value_quote - deposit_value_quote))
            .checked_floor()
            .unwrap()
            .checked_to_num()
            .unwrap();
        msg!("mint_amount: {:?}", mint_amount);

        token::mint_to(iou_mint_ctx, mint_amount)?;
    }

    Ok(())
}
