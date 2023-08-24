// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the Linera indexer.

pub mod graphql;
pub mod operations;
pub mod plugin;
pub mod types;

use crate::{
    graphql::{block, Block},
    types::IndexerError,
};
use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use async_recursion::async_recursion;
use graphql_client::reqwest::post_graphql;
use linera_base::{crypto::CryptoHash, data_types::BlockHeight, identifiers::ChainId};
use linera_chain::data_types::HashedValue;
use linera_views::{
    common::{Context, ContextFromDb, KeyValueStoreClient},
    map_view::MapView,
    register_view::RegisterView,
    set_view::SetView,
    value_splitting::DatabaseConsistencyError,
    views::{RootView, View, ViewError},
};
use operations::OperationsPlugin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

#[derive(RootView)]
pub struct StateView<C> {
    chains: MapView<C, ChainId, (CryptoHash, BlockHeight)>,
    plugins: SetView<C, String>,
    initiated: RegisterView<C, bool>,
}

#[derive(Clone)]
pub struct State<C>(Arc<Mutex<StateView<C>>>);

#[derive(Clone)]
pub struct Indexer<C> {
    start: BlockHeight,
    node: String,
    pub state: State<C>,
    pub operations: Option<OperationsPlugin<C>>,
}

pub enum IndexerCommand {
    Run,
    Schema,
}

impl<DB> Indexer<ContextFromDb<(), DB>>
where
    DB: KeyValueStoreClient + Clone + Send + Sync + 'static,
    DB::Error: From<bcs::Error>
        + From<DatabaseConsistencyError>
        + Send
        + Sync
        + std::error::Error
        + 'static,
    ViewError: From<DB::Error>,
{
    /// Loads all indexer plugins
    pub async fn load(
        plugins: Vec<String>,
        client: DB,
        start: BlockHeight,
        node: String,
        command: IndexerCommand,
    ) -> Result<Self, IndexerError> {
        let context = ContextFromDb::create(client.clone(), "indexer".as_bytes().to_vec(), ())
            .await
            .map_err(|e| IndexerError::ViewError(e.into()))?;
        let state = State(Arc::new(Mutex::new(StateView::load(context).await?)));
        let (plugins, initiated) = {
            let state = state.0.lock().await;
            if *state.initiated.get() {
                (state.plugins.indices().await?, true)
            } else {
                (plugins, false)
            }
        };
        let operations = if plugins.is_empty() {
            None
        } else if plugins == ["operations"] {
            let context = ContextFromDb::create(client, "operations".as_bytes().to_vec(), ())
                .await
                .map_err(|e| IndexerError::ViewError(e.into()))?;
            Some(OperationsPlugin::load(context).await?)
        } else {
            return Err(IndexerError::UnknownPlugin(plugins[0].to_string()));
        };
        if let (IndexerCommand::Run, false) = (command, initiated) {
            let mut state = state.0.lock().await;
            state.initiated.set(true);
            plugins.iter().for_each(|pl| {
                let _ = state.plugins.insert(pl);
            });
            state.save().await?;
        }
        Ok(Indexer {
            operations,
            state,
            start,
            node,
        })
    }

    /// Processes one hashed value
    pub async fn process_value(
        &self,
        state: &mut StateView<ContextFromDb<(), DB>>,
        value: &HashedValue,
    ) -> Result<(), IndexerError> {
        if let Some(plugin) = &self.operations {
            plugin.register(value).await?
        };
        let chain_id = value.inner().chain_id();
        let hash = value.hash();
        let height = value.inner().height();
        info!("save {:?}: {:?} ({})", chain_id, hash, height);
        state
            .chains
            .insert(&chain_id, (value.hash(), value.inner().height()))?;
        state.save().await.map_err(IndexerError::ViewError)
    }

    /// Gets one hashed value from the node service
    pub async fn get_value(
        &self,
        chain_id: ChainId,
        hash: CryptoHash,
    ) -> Result<HashedValue, IndexerError> {
        let client = reqwest::Client::new();
        let variables = block::Variables {
            hash: Some(hash),
            chain_id,
        };
        let response = post_graphql::<Block, _>(&client, &self.node, variables).await?;
        match response.data {
            None => Err(IndexerError::NullData(response.errors)),
            Some(data) => match data.block {
                Some(block) => block.try_into(),
                None => Err(IndexerError::NotFound(hash)),
            },
        }
    }

    /// Processes one hashed value doing recursion to until the last state registered is reached
    #[async_recursion]
    async fn process_rec(
        &self,
        state: &mut StateView<ContextFromDb<(), DB>>,
        value: &HashedValue,
        last_hash: Option<CryptoHash>,
    ) -> Result<(), IndexerError> {
        let chain_id = value.inner().chain_id();
        let Some(executed_block) = value.inner().executed_block() else { return Ok(()) };
        match executed_block.block.previous_block_hash {
            None => self.process_value(state, value).await,
            Some(hash) => {
                if Some(hash) != last_hash {
                    let previous_value = self.get_value(chain_id, hash).await?;
                    self.process_rec(state, &previous_value, last_hash).await?
                }
                self.process_value(state, value).await
            }
        }
    }

    pub async fn process(&self, value: &HashedValue) -> Result<(), IndexerError> {
        let chain_id = value.inner().chain_id();
        let hash = value.hash();
        let height = value.inner().height();
        let state = &mut self.state.0.lock().await;
        if height < self.start {
            return Ok(());
        };
        let last_hash = match state.chains.get(&chain_id).await? {
            None => None,
            Some((last_hash, last_height)) => {
                if last_hash == hash || last_height >= height {
                    return Ok(());
                }
                Some(last_hash)
            }
        };
        info!("process {:?}: {:?} ({})", chain_id, hash, height);
        self.process_rec(state, value, last_hash).await
    }

    pub fn sdl(&self, plugin: &str) -> Result<String, IndexerError> {
        match plugin {
            "" => Ok(self.state.clone().schema().sdl()),
            "operations" => match &self.operations {
                None => Err(IndexerError::UnloadedPlugin(plugin.to_string())),
                Some(plugin) => Ok(plugin.clone().schema().sdl()),
            },
            _ => Err(IndexerError::UnknownPlugin(plugin.to_string())),
        }
    }
}

#[derive(SimpleObject)]
pub struct LastBlock {
    chain: ChainId,
    block: Option<CryptoHash>,
    height: Option<BlockHeight>,
}

#[Object]
impl<C> State<C>
where
    C: Context + Clone + Send + Sync + 'static,
    ViewError: From<C::Error>,
{
    pub async fn plugins(&self) -> Result<Vec<String>, IndexerError> {
        let state = self.0.lock().await;
        Ok(state.plugins.indices().await?)
    }

    pub async fn state(&self) -> Result<Vec<LastBlock>, IndexerError> {
        let state = self.0.lock().await;
        let chains = state.chains.indices().await?;
        let mut result = Vec::new();
        for chain in chains {
            let block = state.chains.get(&chain).await?;
            result.push(LastBlock {
                chain,
                block: block.map(|b| b.0),
                height: block.map(|b| b.1),
            });
        }
        Ok(result)
    }
}

impl<C> State<C>
where
    C: Context + Clone + Send + Sync + 'static,
    ViewError: From<C::Error>,
{
    pub fn schema(self) -> Schema<Self, EmptyMutation, EmptySubscription> {
        Schema::build(self, EmptyMutation, EmptySubscription).finish()
    }
}
