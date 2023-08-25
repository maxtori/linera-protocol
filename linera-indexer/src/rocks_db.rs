// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    indexer::Indexer,
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

pub struct RocksDbRunner {
    client: RocksDbClient,
    config: IndexerConfig<RocksDbConfig>,
    indexer: Indexer<RocksDbClient>,
}

#[async_trait::async_trait]
impl Runner<RocksDbClient, RocksDbConfig> for RocksDbRunner {
    fn make_client(config: &RocksDbConfig) -> RocksDbClient {
        RocksDbClient::new(
            &config.storage,
            config.max_stream_queries,
            config.cache_size,
        )
    }

    fn new(
        client: RocksDbClient,
        config: IndexerConfig<RocksDbConfig>,
        indexer: Indexer<RocksDbClient>,
    ) -> Self {
        Self {
            client,
            config,
            indexer,
        }
    }

    fn indexer(&mut self) -> &mut Indexer<RocksDbClient> {
        &mut self.indexer
    }

    fn config(&self) -> &IndexerConfig<RocksDbConfig> {
        &self.config
    }

    fn client(&self) -> &RocksDbClient {
        &self.client
    }
}
