use pchain_runtime::BlockchainParams;
use pchain_types::blockchain::Transaction;

pub const EXPECTED_CBI_VERSION: u32 = 0;
pub const MIN_BASE_FEE: u64 = 8;

// Origin Account.
pub const ORIGIN_SECRET_KEY_BASE64: &str = "W16XnCJuPYHIKq92aoInCstzSdiBVHXoYPsdM4D_xrk";
pub const ORIGIN_PUBLIC_KEY_BASE64: &str = "L5nPRvEpnomFVB88dd2mSN_kDxIYaLXUyI75F710xfI";
// Target Account.
pub const TARGET_PUBLIC_KEY_BASE64: &str = "WU-d7VzIVVgKiNM2CM_2j-BVY1JCjPFQBowsJKBt4aQ";
pub const TREASURY_PUBLIC_KEY_BASE64: &str = "HLW3Lch72b2m9snDwDF8pHgm_0mwzyTgnM_VtRRQfg4";

pub const CONTRACT_CACHE_FOLDER: &str = "tests/sc_cache";

pub struct TestData {}

impl TestData {
    pub fn transaction() -> Transaction {
        Transaction {
            signer: [1u8; 32],
            commands: Vec::new(),
            priority_fee_per_gas: 0,
            gas_limit: 1_000_000,
            max_base_fee_per_gas: MIN_BASE_FEE,
            nonce: 0,
            hash: [0u8; 32],
            signature: [0u8; 64],
        }
    }

    pub fn block_params() -> BlockchainParams {
        BlockchainParams {
            this_block_number: 1,
            prev_block_hash: [3u8; 32],
            this_base_fee: 1,
            timestamp: 1665370157,
            random_bytes: [255u8; 32],
            proposer_address: [0u8; 32],
            treasury_address: [100u8; 32],
            cur_view: 0,
            validator_performance: None,
        }
    }

    pub fn get_test_contract_code(name: &str) -> Vec<u8> {
        let mut sc_filepath = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        sc_filepath.push(format!("./tests/contracts/{name}.wasm"));
        std::fs::read(sc_filepath).unwrap()
    }

    pub fn get_origin_address() -> pchain_types::cryptography::PublicAddress {
        base64url::decode(ORIGIN_PUBLIC_KEY_BASE64)
            .unwrap()
            .try_into()
            .unwrap()
    }
}
