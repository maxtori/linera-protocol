// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    common::IndexerError,
    runner::{IndexerConfig, Runner},
};
use linera_views::rocks_db::RocksDbClient;
use structopt::StructOpt;

#[derive(StructOpt, Clone, Debug)]
pub struct RocksDbConfig {
    /// RocksDB storage path
    #[structopt(long, default_value = "./indexer.db")]
    pub storage: String,
    /// The maximal number of simultaneous stream queries to the database
    #[structopt(long, default_value = "10")]
    pub max_stream_queries: usize,
    /// Cache size for the RocksDB storage
    #[structopt(long, default_value = "1000")]
    pub cache_size: usize,
}

pub type RocksDbRunner = Runner<RocksDbClient, RocksDbConfig>;

impl RocksDbRunner {
    pub async fn load() -> Result<Self, IndexerError> {
        let config = IndexerConfig::<RocksDbConfig>::from_args();
        let client = RocksDbClient::new(
            &config.client.storage,
            config.client.max_stream_queries,
            config.client.cache_size,
        );
        Self::new(config, client).await
    }
}
