#![allow(warnings)]
use std::str::FromStr;

use ethcontract::{H256, U256};
use evm_ekubo_sdk::quoting::{base_pool::BasePoolState, types::{Config, NodeKey, Pool, QuoteParams, TokenAmount}};

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

    let pool = serde_json::json!({
        "poolKey": {
          "token0": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
          "token1": "0xdAC17F958D2ee523a2206206994597C13D831ec7",
          "config": "0x00000000000000000000000000000000000000000001a36e2eb1c43200000032"
        },
        "poolId": "0x0e647f6d174aa84c22fddeef0af92262b878ba6f86094e54dbec558c0a53ab79",
        "tick": 0,
        "sqrtRatio": "39614081261743854815199363072",
        "extension": "Base"
    });

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

    let (tick, sqrt_ratio_float, liquidity, min_tick, max_tick, ticks) = vec.into_iter().next().unwrap();
    let sqrt_ratio = float_sqrt_ratio_to_fixed(sqrt_ratio_float);

    let mut sorted_ticks = ticks.into_iter().map(|(index, liquidity_delta)| {
        evm_ekubo_sdk::quoting::types::Tick {
            index: index.into(),
            liquidity_delta,
        }
    }).collect::<Vec<_>>();

    let key = NodeKey {
        token0: pool["poolKey"]["token0"].as_str().unwrap().parse().unwrap(),
        token1: pool["poolKey"]["token1"].as_str().unwrap().parse().unwrap(),
        config: to_parsed_config(config),
    };
    let state = BasePoolState {
        sqrt_ratio,
        liquidity,
        active_tick_index: None, // ? Is it correct/
    };
    let pool = evm_ekubo_sdk::quoting::base_pool::BasePool::new(key, state, sorted_ticks).unwrap();
    let amount_out = pool.quote(QuoteParams {
        token_amount: TokenAmount { 
            token: key.token0,
            amount: 100_000_000.into() 
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
