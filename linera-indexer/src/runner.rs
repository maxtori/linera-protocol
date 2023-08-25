// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the trait for indexer runners.

use crate::{common::IndexerError, indexer::Indexer, plugin::Plugin, service::Listener};
use axum::Server;
use linera_base::identifiers::ChainId;
use linera_views::{
    common::KeyValueStoreClient, value_splitting::DatabaseConsistencyError, views::ViewError,
};
use structopt::{StructOpt, StructOptInternal};
use tokio::select;
use tracing::{info, warn};

#[derive(StructOpt, Debug, Clone)]
pub enum IndexerCommand {
    Schema {
        plugin: Option<String>,
    },
    Run {
        #[structopt(flatten)]
        listener: Listener,
        /// The port of the indexer server
        #[structopt(long, default_value = "8081")]
        port: u16,
        /// Chains to index (default: the ones on the service wallet)
        chains: Vec<ChainId>,
    },
}

#[derive(StructOpt, Debug, Clone)]
pub struct IndexerConfig<C: StructOpt> {
    #[structopt(flatten)]
    pub client: C,
    #[structopt(subcommand)]
    pub command: IndexerCommand,
}

pub struct RunnerContent<DB>(Indexer<DB>);

#[async_trait::async_trait]
pub trait Runner<DB, CONFIG>
where
    Self: Send,
    CONFIG: Clone + std::fmt::Debug + Send + Sync + StructOptInternal,
    DB: KeyValueStoreClient + Clone + Send + Sync + 'static,
    DB::Error: From<bcs::Error>
        + From<DatabaseConsistencyError>
        + Send
        + Sync
        + std::error::Error
        + 'static,
    ViewError: From<DB::Error>,
{
    /// Creates a new runner
    fn new(client: DB, config: IndexerConfig<CONFIG>, indexer: Indexer<DB>) -> Self;
    /// Creates a client
    fn make_client(config: &CONFIG) -> DB;
    /// Getter for the indexer
    fn indexer(&mut self) -> &mut Indexer<DB>;
    /// Getter for the configuration
    fn config(&self) -> &IndexerConfig<CONFIG>;
    /// Getter for the client
    fn client(&self) -> &DB;

    /// Loads a new runner
    async fn load() -> Result<Self, IndexerError>
    where
        Self: Sized,
    {
        let config = IndexerConfig::<CONFIG>::from_args();
        let client = Self::make_client(&config.client);
        let indexer = Indexer::load(client.clone()).await?;
        Ok(Self::new(client, config, indexer))
    }

    /// Registers a new plugin to the indexer
    async fn add_plugin(&mut self, plugin: impl Plugin<DB> + 'static) -> Result<(), IndexerError> {
        self.indexer().add_plugin(plugin).await
    }

    /// Runs a server from the indexer and the plugins
    async fn server(port: u16, indexer: &Indexer<DB>) -> Result<(), IndexerError> {
        let mut app = indexer.route(None);
        for plugin in indexer.plugins.values() {
            app = plugin.route(app);
        }
        Server::bind(&format!("127.0.0.1:{}", port).parse()?)
            .serve(app.into_make_service())
            .await?;
        Ok(())
    }

    /// Runs a server and the chains listener
    async fn run(&mut self) -> Result<(), IndexerError> {
        let config = self.config().clone();
        let indexer = self.indexer();
        match config.clone().command {
            IndexerCommand::Schema { plugin } => {
                println!("{}", indexer.sdl(plugin)?);
                Ok(())
            }
            IndexerCommand::Run {
                chains,
                listener,
                port,
            } => {
                info!("config: {:?}", config);
                let chains = if chains.is_empty() {
                    listener.service.get_chains().await?
                } else {
                    chains
                };
                let connections = {
                    chains
                        .into_iter()
                        .map(|chain_id| listener.listen(indexer, chain_id))
                };
                select! {
                    result = Self::server(port, indexer) => {
                        result.map(|()| warn!("GraphQL server stopped"))
                    }
                    (result, _, _) = futures::future::select_all(connections.map(Box::pin)) => {
                        result.map(|chain_id| {
                            warn!("Connection to {:?} notifications websocket stopped", chain_id)
                        })
                    }
                }
            }
        }
    }
}
