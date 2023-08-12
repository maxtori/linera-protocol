// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the Linera indexer.

pub mod application;
pub mod applications;
pub mod graphql;
pub mod messages;
pub mod operations;
pub mod plugin;
pub mod types;

use crate::{
    graphql::{block, Block},
    plugin::Plugin,
    types::IndexerError,
};
use async_recursion::async_recursion;
use graphql_client::reqwest::post_graphql;
use linera_base::{crypto::CryptoHash, data_types::BlockHeight, identifiers::ChainId};
use linera_chain::data_types::HashedValue;
use linera_views::{
    common::{ContextFromDb, KeyValueStoreClient},
    map_view::MapView,
    value_splitting::DatabaseConsistencyError,
    views::{RootView, View, ViewError},
};
use operations::OperationsPlugin;
use std::{cmp::Ordering, sync::Arc};
use tokio::sync::Mutex;
use tracing::info;

#[derive(RootView)]
struct StateView<C> {
    chains: MapView<C, ChainId, CryptoHash>,
}

pub struct Indexer<C> {
    start: BlockHeight,
    node: String,
    state: Arc<Mutex<StateView<C>>>,
    pub operations: Option<OperationsPlugin<C>>,
    pub plugins: Vec<Box<dyn plugin::Plugin<C = C> + Send + Sync>>,
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
        plugins: &[&str],
        client: DB,
        start: BlockHeight,
        node: String,
    ) -> Result<Self, IndexerError> {
        let _plugins: Vec<
            Option<Box<dyn plugin::Plugin<C = ContextFromDb<(), DB>> + Send + Sync>>,
        > = futures::future::join_all(plugins.iter().enumerate().map(|(i, s)| async {
            if s == &"operation" {
                let base: u8 = (i + 1).try_into().unwrap();
                let context = ContextFromDb::create(client.clone(), vec![base], ())
                    .await
                    .map_err(|e| IndexerError::ViewError(e.into()))
                    .ok()?;
                Some(Box::new(OperationsPlugin::load(context).await.ok()?))
            } else {
                None
            }
        }))
        .await;

        let mut _plugins: Vec<Box<dyn plugin::Plugin<C = ContextFromDb<(), DB>> + Send + Sync>> =
            Vec::new();

        let operations = if plugins.contains(&"operations") {
            let context = ContextFromDb::create(client.clone(), vec![1], ())
                .await
                .map_err(|e| IndexerError::ViewError(e.into()))?;
            _plugins.push(Box::new(OperationsPlugin::load(context).await?));
            None
            // Some(OperationsPlugin::load(context).await?)
        } else {
            None
        };

        let context = ContextFromDb::create(client, vec![0], ())
            .await
            .map_err(|e| IndexerError::ViewError(e.into()))?;
        let state = Arc::new(Mutex::new(StateView::load(context).await?));
        Ok(Indexer {
            operations,
            state,
            start,
            node,
            plugins: Vec::new(),
        })
    }

    /// Processes one hashed value
    pub async fn process_value(&self, value: &HashedValue) -> Result<(), IndexerError> {
        if let Some(plugin) = &self.operations {
            plugin.register(value).await?
        };
        let chain_id = value.inner().chain_id();
        info!("update state of chain {:?}: {:?}", chain_id, value.hash());
        let mut state = self.state.lock().await;
        state.chains.insert(&chain_id, value.hash())?;
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
                Some(block) => Ok(block.into()),
                None => Err(IndexerError::NotFound(hash)),
            },
        }
    }

    /// Processes one hashed value doing recursion to until the last state registered is reached
    #[async_recursion]
    pub async fn process(&self, value: &HashedValue) -> Result<(), IndexerError> {
        let chain_id = value.inner().chain_id();
        let last_block_registered = self.state.lock().await.chains.get(&chain_id).await?;
        match value.inner().height().cmp(&self.start) {
            Ordering::Equal => self.process_value(value).await,
            Ordering::Greater => match value.inner().executed_block().block.previous_block_hash {
                None => Ok(()),
                Some(hash) => {
                    if Some(hash) != last_block_registered {
                        let previous_value = self.get_value(chain_id, hash).await?;
                        self.process(&previous_value).await?
                    }
                    self.process_value(value).await
                }
            },
            _ => Ok(()),
        }
    }

    pub fn sdl(&self, plugin: &str) -> Result<String, IndexerError> {
        match plugin {
            "operations" => match &self.operations {
                None => Err(IndexerError::UnloadedPlugin(plugin.to_string())),
                Some(plugin) => Ok(plugin.clone().schema().sdl()),
            },
            _ => Err(IndexerError::UnknownPlugin(plugin.to_string())),
        }
    }
}
