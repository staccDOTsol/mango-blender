use anchor_lang::prelude::*;
use blender::instructions::*;

mod blender;
mod helpers;

declare_id!("6iwLfHKbjvrmQ9jSWAvVu1C8zdWReX2s8XX9yFmvM6p5");

#[program]
pub mod mango_blender {
    use super::*;

    pub fn create_pool(
        ctx: Context<CreatePool>,
        pool_name: String,
        pool_bump: u8,
        iou_mint_bump: u8,
        fee_basis: u8
    ) -> ProgramResult {
        blender::instructions::create_pool::handler(
            ctx,
            pool_name,
            pool_bump,
            iou_mint_bump,
            fee_basis
        )
    }

    pub fn buy_into_pool(ctx: Context<BuyIntoPool>, quantity: u64) -> ProgramResult {
        blender::instructions::buy_into_pool::handler(ctx, quantity)
    }

    pub fn withdraw_from_pool<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, WithdrawFromPool<'info>>,
        quantity: u64,
    ) -> ProgramResult {
        blender::instructions::withdraw_from_pool::handler(ctx, quantity)
    }
}
