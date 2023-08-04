// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This module defines the operations indexer plugin.

use crate::types::IndexerError;
use async_graphql::{Object, SimpleObject};
use linera_base::{
    crypto::{BcsHashable, CryptoHash},
    data_types::BlockHeight,
    identifiers::ChainId,
};
use linera_chain::data_types::HashedValue;
use linera_execution::Operation;
use linera_views::{
    common::Context,
    map_view::MapView,
    views::{RootView, View, ViewError},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

#[derive(Deserialize, Serialize, Clone, SimpleObject, Debug)]
pub struct BlockOperation {
    block: CryptoHash,
    index: usize,
    content: linera_execution::Operation,
}

impl BcsHashable for BlockOperation {}

#[derive(Deserialize, Serialize, Clone, SimpleObject, Debug)]
pub struct OperationView {
    operation: BlockOperation,
    height: BlockHeight,
    previous_operation: Option<CryptoHash>,
}

#[derive(RootView)]
pub struct OperationsPluginInternal<C> {
    last: MapView<C, ChainId, (CryptoHash, BlockHeight, usize)>,
    count: MapView<C, ChainId, u32>,
    /// OperationView MapView indexed by their hash
    operations: MapView<C, CryptoHash, OperationView>,
}

#[derive(Clone)]
pub struct OperationsPlugin<C>(Arc<Mutex<OperationsPluginInternal<C>>>);

impl<C> OperationsPlugin<C>
where
    C: Context + Send + Sync + 'static + Clone,
    ViewError: From<C::Error>,
{
    /// Registers an operation and update count and last entries for this chain ID
    async fn register_operation(
        &self,
        content: Operation,
        block: CryptoHash,
        height: BlockHeight,
        index: usize,
        chain_id: ChainId,
    ) -> Result<(), IndexerError> {
        let mut plugin = self.0.lock().await;
        let last_operation = plugin.last.get(&chain_id).await?;
        match last_operation {
            Some((_last_hash, last_height, last_index))
                if ((last_height > height) || (last_height == height) && (last_index > index)) =>
            {
                Ok(())
            }
            last_operation => {
                let previous_operation = last_operation.map(|last| last.0);
                let operation = BlockOperation {
                    block,
                    index,
                    content,
                };
                let key = CryptoHash::new(&operation);
                let operation_view = OperationView {
                    operation,
                    previous_operation,
                    height,
                };
                info!(
                    "register operation for {:?}:\n{:?}",
                    chain_id, operation_view
                );
                plugin.operations.insert(&key, operation_view.clone())?;
                let count = *plugin.count.get_mut(&chain_id).await?.unwrap_or(&mut 0);
                plugin.count.insert(&chain_id, count + 1)?;
                plugin.last.insert(&chain_id, (key, height, index))?;
                plugin.save().await.map_err(IndexerError::ViewError)
            }
        }
    }

    /// Main function of plugin: registers the operations for a hashed value
    pub async fn register(&self, value: &HashedValue) -> Result<(), IndexerError> {
        let block = &value.inner().executed_block().block;
        let chain_id = value.inner().chain_id();
        for (i, op) in block.operations.iter().enumerate() {
            match self
                .register_operation(op.clone(), value.hash(), block.height, i, chain_id)
                .await
            {
                Err(e) => return Err(e),
                Ok(()) => continue,
            }
        }
        Ok(())
    }

    /// Loads the plugin view from context
    pub async fn load(context: C) -> Result<Self, IndexerError> {
        let plugin = OperationsPluginInternal::load(context).await?;
        Ok(OperationsPlugin(Arc::new(Mutex::new(plugin))))
    }
}

#[Object(name = "OperationsRoot")]
impl<C> OperationsPlugin<C>
where
    C: Context + Send + Sync + 'static + Clone,
    ViewError: From<C::Error>,
{
    /// Gets the operation associated to its hash
    pub async fn operation(&self, key: CryptoHash) -> Result<Option<OperationView>, IndexerError> {
        self.0
            .lock()
            .await
            .operations
            .get(&key)
            .await
            .map_err(IndexerError::ViewError)
    }

    /// Gets the operations in downard order from a operation hash
    pub async fn operations(
        &self,
        from: CryptoHash,
        limit: u32,
    ) -> Result<Vec<OperationView>, IndexerError> {
        let mut key = Some(from);
        let mut result = Vec::new();
        for _ in 0..limit {
            let Some(next_key) = key else { break };
            let operation = self.0.lock().await.operations.get(&next_key).await?;
            match operation {
                None => break,
                Some(op) => {
                    key = op.previous_operation;
                    result.push(op)
                }
            }
        }
        Ok(result)
    }

    /// Gets the number of operations registered for a chain
    pub async fn count(&self, chain_id: ChainId) -> Result<u32, IndexerError> {
        let plugin = self.0.lock().await;
        plugin
            .count
            .get(&chain_id)
            .await
            .map_err(IndexerError::ViewError)
            .map(|opt| opt.unwrap_or(0))
    }

    /// Gets the hash of the last operation registered for a chain
    pub async fn last(&self, chain_id: ChainId) -> Result<Option<CryptoHash>, IndexerError> {
        let plugin = self.0.lock().await;
        plugin
            .last
            .get(&chain_id)
            .await
            .map_err(IndexerError::ViewError)
            .map(|opt| opt.map(|last| last.0))
    }
}
