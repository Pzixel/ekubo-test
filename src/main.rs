#![allow(warnings)]
use std::str::FromStr;

use ethcontract::{Http, H256, U256};
use evm_ekubo_sdk::quoting::{base_pool::{BasePool, BasePoolState}, types::{Config, NodeKey, Pool, QuoteParams, Tick, TokenAmount}, util::find_nearest_initialized_tick_index};
use web3::Web3;

ethcontract::contract!("EkuboCore.json", contract = EkuboCore);
ethcontract::contract!("EkuboDataFetcher.json", contract = EkuboDataFetcher);

#[derive(serde::Deserialize, Debug, Clone)]
pub struct EnvConfig {
    pub mainnet_rpc_url: String,
}

const MIN_TICK_SPACINGS_PER_POOL: u32 = 2;

#[tokio::main]
async fn main() {
    dotenvy::dotenv_override().unwrap();
    let env_config: EnvConfig = envy::from_env().unwrap();
    let web3 = web3::Web3::new(web3::transports::Http::new(&env_config.mainnet_rpc_url).unwrap());

    let pools = [
        serde_json::json!({
            "poolKey": {
            "token0": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "token1": "0xdAC17F958D2ee523a2206206994597C13D831ec7",
            "config": "0x00000000000000000000000000000000000000000001a36e2eb1c43200000032"
            },
            "poolId": "0x0e647f6d174aa84c22fddeef0af92262b878ba6f86094e54dbec558c0a53ab79",
            "tick": 0,
            "sqrtRatio": "39614081261743854815199363072",
            "extension": "Base",
            "amount": "100000000",
        }),
        serde_json::json!({
            "poolKey": {
              "token0": "0x0000000000000000000000000000000000000000",
              "token1": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
              "config": "0x00000000000000000000000000000000000000000020c49ba5e353f7000003e8"
            },
            "poolId": "0x7bc09681ee7056bc1fbf1ef479a99f1e89c106a4e20e2214f56bcc36ccc911bd",
            "tick": -20074520,
            "sqrtRatio": "19807906982078646688166797756",
            "extension": "Base",
            "amount": "100000000000000000",
          }),
    ];
    for pool in pools.into_iter() {
        test_pool(&web3, pool.clone(),pool["amount"].as_str().unwrap().parse::<i128>().unwrap(),).await;
    }
}

async fn test_pool(web3: &Web3<Http>, pool: serde_json::Value, amount: i128) {
    let data_fetcher = EkuboDataFetcher::at(&web3, "0x91cB8a896cAF5e60b1F7C4818730543f849B408c".parse().unwrap());

    let config: H256 = pool["poolKey"]["config"].as_str().unwrap().parse().unwrap();
    let vec = data_fetcher
        .get_quote_data(
            vec![
                (
                    pool["poolKey"]["token0"].as_str().unwrap().parse().unwrap(),
                    pool["poolKey"]["token1"].as_str().unwrap().parse().unwrap(),
                    ethcontract::Bytes(config.0),
                ),
            ],
            MIN_TICK_SPACINGS_PER_POOL,
        )
        .call()
        .await
        .unwrap();

    let data = vec.into_iter().next().unwrap();
    let key = NodeKey {
        token0: pool["poolKey"]["token0"].as_str().unwrap().parse().unwrap(),
        token1: pool["poolKey"]["token1"].as_str().unwrap().parse().unwrap(),
        config: to_parsed_config(config),
    };
    let pool = create_base_pool(key, data);

    let amount_out = pool.quote(QuoteParams {
        token_amount: TokenAmount { 
            token: key.token0,
            amount: amount
        },
        sqrt_ratio_limit: None,
        override_state: None,
        meta: (),
    }).unwrap();

    println!("Swapping {} token {:x} for {} token {:x}", 
        amount_out.consumed_amount,
        key.token0,
        amount_out.calculated_amount,
        key.token1,
    );
}

fn create_base_pool(
    key: NodeKey,
    (tick, sqrt_ratio_float, liquidity, min_tick, max_tick, ticks): (i32, u128, u128, i32, i32, Vec<(i32, i128)>),
) -> BasePool {
    let sqrt_ratio = float_sqrt_ratio_to_fixed(sqrt_ratio_float);

    let ticks = ticks.into_iter().map(|(index, liquidity_delta)| {
        evm_ekubo_sdk::quoting::types::Tick {
            index: index.into(),
            liquidity_delta,
        }
    }).collect::<Vec<_>>();

    // dbg!(&sorted_ticks, liquidity);

    // let mut state = BasePoolState {
    //     sqrt_ratio,
    //     liquidity,
    //     active_tick_index: find_nearest_initialized_tick_index(&sorted_ticks, tick),
    // };
    // add_liquidity_cutoffs(
    //     &mut sorted_ticks,
    //     &mut state.active_tick_index,
    //     tick,
    //     liquidity,
    //     min_tick,
    //     max_tick,
    // );
    // dbg!(&state);
    let pool = evm_ekubo_sdk::quoting::base_pool::BasePool::from_partial_data(key, sqrt_ratio, ticks, min_tick, max_tick, liquidity, tick)
        .unwrap();
    pool
}

pub fn to_parsed_config(config: H256) -> evm_ekubo_sdk::quoting::types::Config {
    let compact_config = config.to_fixed_bytes();
    // first 20 bytes are the extension
    let extension = evm_ekubo_sdk::math::uint::U256::from_big_endian(&compact_config[0..20]);
    // next 8 bytes are the fee
    let fee = u64::from_be_bytes(compact_config[20..28].try_into().unwrap());
    // next 4 bytes are the tick spacing
    let tick_spacing = u32::from_be_bytes(compact_config[28..32].try_into().unwrap());
    evm_ekubo_sdk::quoting::types::Config {
        tick_spacing,
        fee,
        extension,
    }
}


fn float_sqrt_ratio_to_fixed(sqrt_ratio_float: u128) -> evm_ekubo_sdk::math::uint::U256 {
    let BIT_MASK = U256::from_str("0xc00000000000000000000000").unwrap();
    let NOT_BIT_MASK = U256::from_str("0x3fffffffffffffffffffffff").unwrap();

    // export function floatSqrtRatioToFixed(sqrtRatioFloat: bigint): bigint {
    //     return (
    //       (sqrtRatioFloat & NOT_BIT_MASK) <<
    //       (2n + ((sqrtRatioFloat & BIT_MASK) >> 89n))
    //     );
    //   }
    let sqrt_ratio_float = U256::from(sqrt_ratio_float);
    // format just the same as the js version
    evm_ekubo_sdk::math::uint::U256((
        (sqrt_ratio_float & NOT_BIT_MASK) <<
        (U256::from(2) + ((sqrt_ratio_float & BIT_MASK) >> 89))
    ).0)
}

fn add_liquidity_cutoffs(
    sorted_ticks: &mut Vec<evm_ekubo_sdk::quoting::types::Tick>,
    active_tick_index: &mut Option<usize>,
    active_tick: i32,
    liquidity: u128,
    min_tick: i32,
    max_tick: i32,
) {
    // const { sortedTicks, liquidity, activeTick } = state;

    // let activeTickIndex = undefined;
    // let currentLiquidity = 0n;

    // // The liquidity added/removed by out-of-range initialized ticks (i.e. lower than minCheckedTickNumber)
    // let liquidityDeltaMin = 0n;

    *active_tick_index = None;
    let mut current_liquidity = 0i128;
    let mut liquidity_delta_min = 0i128;

    // for (let i = 0; i < sortedTicks.length; i++) {
    //     const tick = sortedTicks[i];

    //     if (typeof activeTickIndex === 'undefined' && activeTick < tick.number) {
    //       activeTickIndex = i === 0 ? null : i - 1;

    //       liquidityDeltaMin = liquidity - currentLiquidity;

    //       // We now need to switch to tracking the liquidity that needs to be cut off at maxCheckedTickNumber, therefore reset to the actual liquidity
    //       currentLiquidity = liquidity;
    //     }

    //     currentLiquidity += tick.liquidityDelta;
    //   }

    for i in 0..sorted_ticks.len() {
        let tick = &sorted_ticks[i];

        if active_tick_index.is_none() && active_tick < tick.index {
            *active_tick_index = if i == 0 { None } else { Some(i - 1) };

            liquidity_delta_min = i128::try_from(liquidity).unwrap() - current_liquidity;

            // We now need to switch to tracking the liquidity that needs to be cut off at maxCheckedTickNumber, therefore reset to the actual liquidity
            current_liquidity = i128::try_from(liquidity).unwrap();
        }

        current_liquidity += tick.liquidity_delta;
    }

    // if (typeof activeTickIndex === 'undefined') {
    //     activeTickIndex = sortedTicks.length > 0 ? sortedTicks.length - 1 : null;
    //     liquidityDeltaMin = liquidity - currentLiquidity;
    //     currentLiquidity = liquidity;
    //   }

    //   state.activeTickIndex = activeTickIndex;

    if active_tick_index.is_none() {
        *active_tick_index = if sorted_ticks.len() > 0 {
            Some(sorted_ticks.len() - 1)
        } else {
            None
        };
        liquidity_delta_min = i128::try_from(liquidity).unwrap() - current_liquidity;
        current_liquidity = i128::try_from(liquidity).unwrap();
    }

    update_tick(
        sorted_ticks,
        active_tick,
        active_tick_index,
        min_tick,
        liquidity_delta_min,
        false,
        true,
    );
    update_tick(
        sorted_ticks,
        active_tick,
        active_tick_index,
        max_tick,
        current_liquidity,
        true,
        true,
    );
}


fn update_tick(
    sorted_ticks: &mut Vec<evm_ekubo_sdk::quoting::types::Tick>,
    active_tick: i32,
    active_tick_index: &mut Option<usize>,
    mut updated_tick_number: i32,
    mut liquidity_delta: i128,
    mut upper: bool,
    mut force_insert: bool,
) {
    if upper {
        liquidity_delta = -liquidity_delta;
    }

    let nearest_tick_index = find_nearest_initialized_tick_index(
        &sorted_ticks,
        updated_tick_number,
    );

    let nearest_tick = if nearest_tick_index.is_none() {
        None
    } else {
        Some(sorted_ticks[nearest_tick_index.unwrap()])
    };
    let nearest_tick_number = nearest_tick.map(|tick| tick.index);
    let new_tick_referenced = nearest_tick_number != Some(updated_tick_number);

    if new_tick_referenced {
        if !force_insert && nearest_tick_index.is_none() {
            sorted_ticks[0].liquidity_delta += liquidity_delta;
        } else if !force_insert && nearest_tick_index == Some(sorted_ticks.len() - 1) {
            let last = sorted_ticks.len() - 1;
            sorted_ticks[last].liquidity_delta += liquidity_delta;
        } else {
            sorted_ticks.insert(
                nearest_tick_index.map_or(0, |i| i + 1),
                Tick {
                    index: updated_tick_number,
                    liquidity_delta,
                },
            );

            if active_tick >= updated_tick_number {
                // state.activeTickIndex =
                // state.activeTickIndex === null ? 0 : state.activeTickIndex + 1;
                *active_tick_index = active_tick_index.map(|i| i + 1).or(Some(0));
            }
        }
    } else {
        // const newDelta = nearestTick!.liquidityDelta + liquidityDelta;

        // if (
        //   newDelta === 0n &&
        //   !state.checkedTicksBounds.includes(nearestTickNumber)
        // ) {
        //   sortedTicks.splice(nearestTickIndex!, 1);
  
        //   if (state.activeTick >= updatedTickNumber) {
        //     state.activeTickIndex!--;
        //   }
        // } else {
        //   nearestTick!.liquidityDelta = newDelta;
        // }
        let new_delta = nearest_tick.unwrap().liquidity_delta + liquidity_delta;
        if new_delta == 0 && !sorted_ticks.iter().any(|tick| tick.index == nearest_tick_number.unwrap()) {
            sorted_ticks.retain(|tick| tick.index != nearest_tick_number.unwrap());
            if active_tick >= updated_tick_number {
                *active_tick_index = active_tick_index.map(|i| i - 1);
            }
        } else {
            sorted_ticks[nearest_tick_index.unwrap()].liquidity_delta = new_delta;
        }
    }
}